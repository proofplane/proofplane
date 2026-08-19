use async_trait::async_trait;
use bytes::Bytes;
use futures_core::Stream;

use crate::config::ObjectStorageConfig;

use super::{
    FilesystemObjectStore, ObjectKey, ObjectMetadata, ObjectStore, ObjectStream, PutObjectRequest,
    StorageError,
};

/// The object store that a process uses. The configuration selects it once at
/// startup.
///
/// Services and handlers name this enum instead of a backend type. Each method
/// sends the operation to the selected backend. Dispatch is static.
#[derive(Debug, Clone)]
pub enum RuntimeObjectStore {
    Filesystem(FilesystemObjectStore),
}

impl RuntimeObjectStore {
    pub async fn from_config(config: &ObjectStorageConfig) -> Result<Self, StorageError> {
        match config {
            ObjectStorageConfig::Filesystem { root } => {
                FilesystemObjectStore::new(root).await.map(Self::Filesystem)
            }
            ObjectStorageConfig::Gcs(_) => Err(StorageError::UnsupportedBackend { backend: "gcs" }),
        }
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
        }
    }

    async fn get_object(&self, key: &ObjectKey) -> Result<ObjectStream, StorageError> {
        match self {
            Self::Filesystem(store) => store.get_object(key).await,
        }
    }

    async fn head_object(&self, key: &ObjectKey) -> Result<ObjectMetadata, StorageError> {
        match self {
            Self::Filesystem(store) => store.head_object(key).await,
        }
    }

    async fn copy_object(
        &self,
        source: &ObjectKey,
        destination: &ObjectKey,
    ) -> Result<ObjectMetadata, StorageError> {
        match self {
            Self::Filesystem(store) => store.copy_object(source, destination).await,
        }
    }

    async fn delete_object(&self, key: &ObjectKey) -> Result<(), StorageError> {
        match self {
            Self::Filesystem(store) => store.delete_object(key).await,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{GcsCredentialsMode, GcsObjectStorageConfig};
    use std::path::PathBuf;
    use url::Url;
    use uuid::Uuid;

    #[tokio::test]
    async fn filesystem_configuration_selects_the_filesystem_store() {
        let root = temp_dir("selection");

        let store = RuntimeObjectStore::from_config(&ObjectStorageConfig::Filesystem {
            root: root.clone(),
        })
        .await
        .unwrap();

        let RuntimeObjectStore::Filesystem(filesystem) = &store;
        assert_eq!(filesystem.root(), root);
        assert!(root.try_exists().unwrap());
    }

    #[tokio::test]
    async fn unsupported_configuration_fails_and_names_the_backend() {
        let config = ObjectStorageConfig::Gcs(GcsObjectStorageConfig {
            bucket: "bucket".to_owned(),
            endpoint_override: Some(Url::parse("http://localhost:4443").unwrap()),
            credentials_mode: GcsCredentialsMode::Anonymous,
            object_key_prefix: "proofplane".to_owned(),
        });

        let error = RuntimeObjectStore::from_config(&config)
            .await
            .expect_err("an unsupported backend cannot start");

        assert!(matches!(
            error,
            StorageError::UnsupportedBackend { backend: "gcs" }
        ));
        assert_eq!(
            error.to_string(),
            "object storage backend gcs is not supported"
        );
    }

    fn temp_dir(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "proofplane-runtime-object-store-{name}-{}",
            Uuid::new_v4()
        ))
    }
}
