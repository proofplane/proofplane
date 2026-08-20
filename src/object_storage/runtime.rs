use async_trait::async_trait;
use bytes::Bytes;
use futures_core::Stream;
use std::path::Path;

use crate::config::{GcsObjectStorageConfig, ObjectStorageConfig};

use super::{
    FilesystemObjectStore, GcsObjectStore, ObjectKey, ObjectMetadata, ObjectStore, ObjectStream,
    PutObjectRequest, StorageError,
};

/// The object store that a process uses. The configuration selects it once at
/// startup.
///
/// Services and handlers name this enum instead of a backend type. Each method
/// sends the operation to the selected backend. Dispatch is static.
#[derive(Debug, Clone)]
pub enum RuntimeObjectStore {
    Filesystem(FilesystemObjectStore),
    Gcs(GcsObjectStore),
}

impl RuntimeObjectStore {
    pub async fn from_config(config: &ObjectStorageConfig) -> Result<Self, StorageError> {
        match configured_backend(config) {
            ConfiguredBackend::Filesystem(root) => {
                FilesystemObjectStore::new(root).await.map(Self::Filesystem)
            }
            ConfiguredBackend::Gcs(config) => {
                GcsObjectStore::new(&config.bucket, &config.object_key_prefix)
                    .await
                    .map(Self::Gcs)
            }
        }
    }
}

#[derive(Debug)]
enum ConfiguredBackend<'a> {
    Filesystem(&'a Path),
    Gcs(&'a GcsObjectStorageConfig),
}

fn configured_backend(config: &ObjectStorageConfig) -> ConfiguredBackend<'_> {
    match config {
        ObjectStorageConfig::Filesystem { root } => ConfiguredBackend::Filesystem(root),
        ObjectStorageConfig::Gcs(config) => ConfiguredBackend::Gcs(config),
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
        destination: &ObjectKey,
    ) -> Result<ObjectMetadata, StorageError> {
        match self {
            Self::Filesystem(store) => store.copy_object(source, destination).await,
            Self::Gcs(store) => store.copy_object(source, destination).await,
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
    use std::path::PathBuf;
    use uuid::Uuid;

    #[tokio::test]
    async fn filesystem_configuration_selects_the_filesystem_store() {
        let root = temp_dir("selection");

        let store = RuntimeObjectStore::from_config(&ObjectStorageConfig::Filesystem {
            root: root.clone(),
        })
        .await
        .unwrap();

        let RuntimeObjectStore::Filesystem(filesystem) = &store else {
            panic!("filesystem configuration selected a different backend");
        };
        assert_eq!(filesystem.root(), root);
        assert!(root.try_exists().unwrap());
    }

    #[test]
    fn gcs_configuration_selects_the_gcs_backend_before_client_initialization() {
        let config = ObjectStorageConfig::Gcs(crate::config::GcsObjectStorageConfig {
            bucket: "contract-bucket".to_owned(),
            object_key_prefix: "contract/run".to_owned(),
        });

        let ConfiguredBackend::Gcs(selected) = configured_backend(&config) else {
            panic!("GCS configuration selected a different backend");
        };
        assert_eq!(selected.bucket, "contract-bucket");
        assert_eq!(selected.object_key_prefix, "contract/run");
    }

    fn temp_dir(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "proofplane-runtime-object-store-{name}-{}",
            Uuid::new_v4()
        ))
    }
}
