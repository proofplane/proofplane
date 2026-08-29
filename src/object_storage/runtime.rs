use std::{path::Path, sync::Arc};

use async_trait::async_trait;
use bytes::Bytes;
use futures_core::Stream;

use crate::config::ObjectStorageConfig;

use super::{
    stream_copy_object, FilesystemObjectStore, GcsObjectStore, ObjectKey, ObjectMetadata,
    ObjectStore, ObjectStream, PutObjectRequest, StorageError,
};

/// The object store backend that a process uses. The configuration selects it
/// once at startup.
///
/// Services and handlers name [`QuarantineObjectStore`] or
/// [`EvidenceObjectStore`] rather than this enum, because those say which
/// bucket the operation reaches. Each method here sends the operation to the
/// selected backend. Dispatch is static.
#[derive(Debug, Clone)]
pub(crate) enum RuntimeObjectStore {
    Filesystem(FilesystemObjectStore),
    Gcs(GcsObjectStore),
}

/// The store that holds uploaded bytes that no scan cleared yet.
///
/// The API and the MCP server write only here. Nothing in this type reads or
/// writes the evidence store except [`QuarantineObjectStore::promote`], which
/// is the single path a document takes out of quarantine.
#[derive(Debug, Clone)]
pub struct QuarantineObjectStore(Arc<RuntimeObjectStore>);

/// The store that holds scanned documents.
///
/// It accepts no upload on purpose. [`QuarantineObjectStore::promote`] is the
/// only way bytes arrive here, and only after a clean scan. Delete remains,
/// because finalization removes a destination whose metadata did not match.
#[derive(Debug, Clone)]
pub struct EvidenceObjectStore(Arc<RuntimeObjectStore>);

/// The quarantine store and the evidence store a process uses.
///
/// One configuration builds both, so the two can never disagree about the
/// backend, and the configuration refuses to name one target for both.
#[derive(Debug, Clone)]
pub struct DocumentObjectStores {
    pub quarantine: QuarantineObjectStore,
    pub evidence: EvidenceObjectStore,
}

impl DocumentObjectStores {
    pub async fn from_config(config: &ObjectStorageConfig) -> Result<Self, StorageError> {
        let (quarantine, evidence) = match configured_backend(config) {
            ConfiguredBackend::Filesystem {
                quarantine_root,
                evidence_root,
            } => (
                RuntimeObjectStore::Filesystem(FilesystemObjectStore::new(quarantine_root).await?),
                RuntimeObjectStore::Filesystem(FilesystemObjectStore::new(evidence_root).await?),
            ),
            ConfiguredBackend::Gcs {
                quarantine_bucket,
                evidence_bucket,
                object_key_prefix,
            } => {
                let quarantine = GcsObjectStore::new(quarantine_bucket, object_key_prefix).await?;
                let evidence = quarantine.for_bucket(evidence_bucket);
                (
                    RuntimeObjectStore::Gcs(quarantine),
                    RuntimeObjectStore::Gcs(evidence),
                )
            }
        };

        Ok(Self {
            quarantine: QuarantineObjectStore(Arc::new(quarantine)),
            evidence: EvidenceObjectStore(Arc::new(evidence)),
        })
    }
}

impl QuarantineObjectStore {
    pub async fn put_object<S>(
        &self,
        request: PutObjectRequest<S>,
    ) -> Result<ObjectMetadata, StorageError>
    where
        S: Stream<Item = Result<Bytes, StorageError>> + Send,
    {
        self.0.put_object(request).await
    }

    pub async fn get_object(&self, key: &ObjectKey) -> Result<ObjectStream, StorageError> {
        self.0.get_object(key).await
    }

    pub async fn head_object(&self, key: &ObjectKey) -> Result<ObjectMetadata, StorageError> {
        self.0.head_object(key).await
    }

    pub async fn delete_object(&self, key: &ObjectKey) -> Result<(), StorageError> {
        self.0.delete_object(key).await
    }

    /// Copies a scanned object into the evidence store and returns the
    /// destination metadata.
    ///
    /// The caller compares that metadata with the persisted document before it
    /// marks the document uploaded, and removes the destination if the two
    /// disagree.
    pub async fn promote(
        &self,
        evidence: &EvidenceObjectStore,
        source: &ObjectKey,
        destination: &ObjectKey,
    ) -> Result<ObjectMetadata, StorageError> {
        self.0.copy_object(source, &evidence.0, destination).await
    }
}

impl EvidenceObjectStore {
    pub async fn get_object(&self, key: &ObjectKey) -> Result<ObjectStream, StorageError> {
        self.0.get_object(key).await
    }

    pub async fn head_object(&self, key: &ObjectKey) -> Result<ObjectMetadata, StorageError> {
        self.0.head_object(key).await
    }

    pub async fn delete_object(&self, key: &ObjectKey) -> Result<(), StorageError> {
        self.0.delete_object(key).await
    }
}

#[derive(Debug, PartialEq, Eq)]
enum ConfiguredBackend<'a> {
    Filesystem {
        quarantine_root: &'a Path,
        evidence_root: &'a Path,
    },
    Gcs {
        quarantine_bucket: &'a str,
        evidence_bucket: &'a str,
        object_key_prefix: &'a str,
    },
}

fn configured_backend(config: &ObjectStorageConfig) -> ConfiguredBackend<'_> {
    match config {
        ObjectStorageConfig::Filesystem(config) => ConfiguredBackend::Filesystem {
            quarantine_root: &config.quarantine_root,
            evidence_root: &config.evidence_root,
        },
        ObjectStorageConfig::Gcs(config) => ConfiguredBackend::Gcs {
            quarantine_bucket: &config.quarantine_bucket,
            evidence_bucket: &config.evidence_bucket,
            object_key_prefix: &config.object_key_prefix,
        },
    }
}

#[async_trait]
impl ObjectStore for RuntimeObjectStore {
    async fn put_object<S>(
        &self,
        request: PutObjectRequest<S>,
    ) -> Result<ObjectMetadata, StorageError>
    where
        S: Stream<Item = Result<Bytes, StorageError>> + Send,
    {
        match self {
            Self::Filesystem(store) => store.put_object(request).await,
            Self::Gcs(store) => store.put_object(request).await,
        }
    }

    async fn get_object(&self, key: &ObjectKey) -> Result<ObjectStream, StorageError> {
        match self {
            Self::Filesystem(store) => store.get_object(key).await,
            Self::Gcs(store) => store.get_object(key).await,
        }
    }

    async fn head_object(&self, key: &ObjectKey) -> Result<ObjectMetadata, StorageError> {
        match self {
            Self::Filesystem(store) => store.head_object(key).await,
            Self::Gcs(store) => store.head_object(key).await,
        }
    }

    async fn copy_object(
        &self,
        source: &ObjectKey,
        destination_store: &Self,
        destination: &ObjectKey,
    ) -> Result<ObjectMetadata, StorageError> {
        match (self, destination_store) {
            (Self::Gcs(source_store), Self::Gcs(destination_store)) => {
                source_store
                    .copy_object(source, destination_store, destination)
                    .await
            }
            // Filesystem copy is a read and a write. A mixed pair is the same
            // work, and the configuration cannot produce one.
            _ => stream_copy_object(self, source, destination_store, destination).await,
        }
    }

    async fn delete_object(&self, key: &ObjectKey) -> Result<(), StorageError> {
        match self {
            Self::Filesystem(store) => store.delete_object(key).await,
            Self::Gcs(store) => store.delete_object(key).await,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        config::{FilesystemObjectStorageConfig, GcsObjectStorageConfig},
        domain::WorkspaceId,
    };
    use bytes::Bytes;
    use futures_util::{stream, StreamExt};
    use std::path::PathBuf;
    use uuid::Uuid;

    #[tokio::test]
    async fn filesystem_configuration_builds_a_separate_root_for_each_store() {
        let quarantine_root = temp_dir("quarantine");
        let evidence_root = temp_dir("evidence");

        let stores = filesystem_stores(&quarantine_root, &evidence_root).await;

        let RuntimeObjectStore::Filesystem(quarantine) = stores.quarantine.0.as_ref() else {
            panic!("filesystem configuration selected a different backend for quarantine");
        };
        let RuntimeObjectStore::Filesystem(evidence) = stores.evidence.0.as_ref() else {
            panic!("filesystem configuration selected a different backend for evidence");
        };
        assert_eq!(quarantine.root(), quarantine_root);
        assert_eq!(evidence.root(), evidence_root);
        assert!(quarantine_root.try_exists().unwrap());
        assert!(evidence_root.try_exists().unwrap());
    }

    #[test]
    fn gcs_configuration_maps_each_store_to_its_own_bucket() {
        let config = ObjectStorageConfig::Gcs(GcsObjectStorageConfig {
            quarantine_bucket: "contract-quarantine".to_owned(),
            evidence_bucket: "contract-evidence".to_owned(),
            object_key_prefix: "contract/run".to_owned(),
        });

        assert_eq!(
            configured_backend(&config),
            ConfiguredBackend::Gcs {
                quarantine_bucket: "contract-quarantine",
                evidence_bucket: "contract-evidence",
                object_key_prefix: "contract/run",
            }
        );
    }

    #[tokio::test]
    async fn promotion_writes_only_to_the_evidence_store() {
        let quarantine_root = temp_dir("promote-quarantine");
        let evidence_root = temp_dir("promote-evidence");
        let stores = filesystem_stores(&quarantine_root, &evidence_root).await;
        let workspace_id = WorkspaceId::from(Uuid::new_v4());
        let source = ObjectKey::new(workspace_id, "documents/upload", "report.txt").unwrap();
        let destination = ObjectKey::new(workspace_id, "documents/final", "report.txt").unwrap();

        let staged = stores
            .quarantine
            .put_object(PutObjectRequest {
                key: source.clone(),
                content_type: "text/plain".to_owned(),
                chunks: stream::once(async { Ok(Bytes::from_static(b"hello")) }),
            })
            .await
            .expect("quarantine accepts the upload");

        let promoted = stores
            .quarantine
            .promote(&stores.evidence, &source, &destination)
            .await
            .expect("promotion succeeds");

        assert_eq!(promoted.key, destination);
        assert_eq!(promoted.sha256, staged.sha256);
        assert_eq!(promoted.content_length, staged.content_length);
        assert_eq!(
            evidence_bytes(&stores.evidence, &destination).await,
            b"hello"
        );
        assert!(matches!(
            stores.quarantine.head_object(&destination).await,
            Err(StorageError::NotFound)
        ));
        assert!(matches!(
            stores.evidence.head_object(&source).await,
            Err(StorageError::NotFound)
        ));
        stores
            .quarantine
            .head_object(&source)
            .await
            .expect("promotion leaves the source for the caller to delete");
    }

    async fn filesystem_stores(
        quarantine_root: &Path,
        evidence_root: &Path,
    ) -> DocumentObjectStores {
        DocumentObjectStores::from_config(&ObjectStorageConfig::Filesystem(
            FilesystemObjectStorageConfig {
                quarantine_root: quarantine_root.to_path_buf(),
                evidence_root: evidence_root.to_path_buf(),
            },
        ))
        .await
        .expect("filesystem stores initialize")
    }

    async fn evidence_bytes(store: &EvidenceObjectStore, key: &ObjectKey) -> Vec<u8> {
        store
            .get_object(key)
            .await
            .expect("evidence object reads")
            .chunks
            .map(|chunk| chunk.expect("evidence chunk reads"))
            .collect::<Vec<_>>()
            .await
            .concat()
    }

    fn temp_dir(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "proofplane-runtime-object-store-{name}-{}",
            Uuid::new_v4()
        ))
    }
}
