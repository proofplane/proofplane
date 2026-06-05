use std::{
    fmt, io,
    path::{Path, PathBuf},
    pin::pin,
};

use async_trait::async_trait;
use bytes::Bytes;
use futures_core::Stream;
use futures_util::{stream, StreamExt};
use serde::{Deserialize, Deserializer, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;
use tokio::{fs, io::AsyncWriteExt};

use crate::{config::ObjectStorageConfig, domain::WorkspaceId};

#[async_trait]
pub trait ObjectStore {
    async fn put_object<S>(
        &self,
        request: PutObjectRequest<S>,
    ) -> Result<ObjectMetadata, StorageError>
    where
        S: Stream<Item = Result<Bytes, StorageError>> + Send;

    async fn get_object(&self, key: ObjectKey) -> Result<ObjectStream, StorageError>;

    async fn head_object(&self, key: ObjectKey) -> Result<ObjectMetadata, StorageError>;

    async fn copy_object(
        &self,
        source: ObjectKey,
        destination: ObjectKey,
    ) -> Result<ObjectMetadata, StorageError>;

    async fn delete_object(&self, key: ObjectKey) -> Result<(), StorageError>;
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct ObjectKey(String);

impl ObjectKey {
    pub fn new(
        workspace_id: WorkspaceId,
        stable_prefix: impl AsRef<str>,
        object_name: impl AsRef<str>,
    ) -> Result<Self, StorageError> {
        let mut segments = vec!["workspaces".to_owned(), workspace_id.to_string()];
        segments.extend(validated_path(stable_prefix.as_ref())?);
        segments.extend(validated_path(object_name.as_ref())?);
        Ok(Self(segments.join("/")))
    }

    pub fn parse(value: impl Into<String>) -> Result<Self, StorageError> {
        let value = value.into();
        if value.is_empty() {
            return Err(StorageError::InvalidKey);
        }

        let segments = validated_path(&value)?;
        if segments.len() < 4 || segments.first().map(String::as_str) != Some("workspaces") {
            return Err(StorageError::InvalidKey);
        }

        uuid::Uuid::parse_str(&segments[1]).map_err(|_| StorageError::InvalidKey)?;

        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn segments(&self) -> impl Iterator<Item = &str> {
        self.0.split('/')
    }
}

impl fmt::Display for ObjectKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl<'de> Deserialize<'de> for ObjectKey {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(value).map_err(serde::de::Error::custom)
    }
}

pub struct PutObjectRequest<S> {
    pub key: ObjectKey,
    pub content_type: String,
    pub chunks: S,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObjectMetadata {
    pub key: ObjectKey,
    pub content_type: String,
    pub content_length: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectStream {
    pub metadata: ObjectMetadata,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("invalid object key")]
    InvalidKey,

    #[error("object not found")]
    NotFound,

    #[error("filesystem storage error")]
    Filesystem {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("metadata persistence error")]
    MetadataPersistence {
        #[source]
        source: serde_json::Error,
    },

    #[error("object stream read error: {message}")]
    StreamRead {
        message: String,
        payload_too_large: bool,
    },

    #[error("object storage backend is not supported")]
    UnsupportedBackend,
}

#[derive(Debug, Clone)]
pub struct FilesystemObjectStore {
    root: PathBuf,
}

impl FilesystemObjectStore {
    pub async fn from_config(config: &ObjectStorageConfig) -> Result<Self, StorageError> {
        match config {
            ObjectStorageConfig::Filesystem { root } => Self::new(root).await,
            ObjectStorageConfig::Gcs(_) => Err(StorageError::UnsupportedBackend),
        }
    }

    pub async fn new(root: impl AsRef<Path>) -> Result<Self, StorageError> {
        let root = root.as_ref().to_path_buf();
        fs::create_dir_all(&root)
            .await
            .map_err(|source| StorageError::Filesystem {
                path: root.clone(),
                source,
            })?;

        Ok(Self { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn object_path(&self, key: &ObjectKey) -> PathBuf {
        key.segments()
            .fold(self.root.join("objects"), |path, segment| {
                path.join(segment)
            })
    }

    fn metadata_path(&self, key: &ObjectKey) -> PathBuf {
        let mut path = key
            .segments()
            .fold(self.root.join("metadata"), |path, segment| {
                path.join(segment)
            });
        let file_name = path
            .file_name()
            .expect("validated object keys always have a file name")
            .to_string_lossy()
            .into_owned();
        path.set_file_name(format!("{file_name}.json"));
        path
    }
}

#[async_trait]
impl ObjectStore for FilesystemObjectStore {
    async fn put_object<S>(
        &self,
        request: PutObjectRequest<S>,
    ) -> Result<ObjectMetadata, StorageError>
    where
        S: Stream<Item = Result<Bytes, StorageError>> + Send,
    {
        let object_path = self.object_path(&request.key);
        create_parent_dir(&object_path).await?;

        let write_result = write_object_stream(&object_path, request.chunks).await;
        let (content_length, sha256) = match write_result {
            Ok(result) => result,
            Err(error) => {
                let _ = remove_file_if_exists(object_path.clone()).await;
                return Err(error);
            }
        };

        let metadata = ObjectMetadata {
            key: request.key,
            content_type: request.content_type,
            content_length,
            sha256,
        };

        let metadata_path = self.metadata_path(&metadata.key);
        create_parent_dir(&metadata_path).await?;
        let metadata_bytes = serde_json::to_vec_pretty(&metadata)
            .map_err(|source| StorageError::MetadataPersistence { source })?;
        if let Err(error) = fs::write(&metadata_path, metadata_bytes)
            .await
            .map_err(|source| StorageError::Filesystem {
                path: metadata_path.clone(),
                source,
            })
        {
            let _ = remove_file_if_exists(object_path).await;
            return Err(error);
        }

        Ok(metadata)
    }

    async fn get_object(&self, key: ObjectKey) -> Result<ObjectStream, StorageError> {
        let metadata = self.head_object(key.clone()).await?;
        let object_path = self.object_path(&key);
        let bytes = fs::read(&object_path)
            .await
            .map_err(|source| match source.kind() {
                io::ErrorKind::NotFound => StorageError::NotFound,
                _ => StorageError::Filesystem {
                    path: object_path.clone(),
                    source,
                },
            })?;

        Ok(ObjectStream { metadata, bytes })
    }

    async fn head_object(&self, key: ObjectKey) -> Result<ObjectMetadata, StorageError> {
        let metadata_path = self.metadata_path(&key);
        let bytes = fs::read(&metadata_path)
            .await
            .map_err(|source| match source.kind() {
                io::ErrorKind::NotFound => StorageError::NotFound,
                _ => StorageError::Filesystem {
                    path: metadata_path.clone(),
                    source,
                },
            })?;

        serde_json::from_slice(&bytes)
            .map_err(|source| StorageError::MetadataPersistence { source })
    }

    async fn copy_object(
        &self,
        source: ObjectKey,
        destination: ObjectKey,
    ) -> Result<ObjectMetadata, StorageError> {
        let object = self.get_object(source).await?;

        self.put_object(PutObjectRequest {
            key: destination,
            content_type: object.metadata.content_type,
            chunks: stream::once(async move { Ok(Bytes::from(object.bytes)) }),
        })
        .await
    }

    async fn delete_object(&self, key: ObjectKey) -> Result<(), StorageError> {
        remove_file_if_exists(self.object_path(&key)).await?;
        remove_file_if_exists(self.metadata_path(&key)).await
    }
}

pub async fn from_config(
    config: &ObjectStorageConfig,
) -> Result<FilesystemObjectStore, StorageError> {
    FilesystemObjectStore::from_config(config).await
}

fn validated_path(value: &str) -> Result<Vec<String>, StorageError> {
    if value.is_empty() || value.starts_with('/') || value.ends_with('/') {
        return Err(StorageError::InvalidKey);
    }

    value
        .split('/')
        .map(validated_segment)
        .collect::<Result<Vec<_>, _>>()
}

fn validated_segment(segment: &str) -> Result<String, StorageError> {
    let has_invalid_char = segment
        .chars()
        .any(|character| character == '\\' || character == '\0');

    if segment.is_empty() || segment == "." || segment == ".." || has_invalid_char {
        return Err(StorageError::InvalidKey);
    }

    Ok(segment.to_owned())
}

async fn create_parent_dir(path: &Path) -> Result<(), StorageError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .await
            .map_err(|source| StorageError::Filesystem {
                path: parent.to_path_buf(),
                source,
            })?;
    }
    Ok(())
}

async fn write_object_stream(
    path: &Path,
    chunks: impl Stream<Item = Result<Bytes, StorageError>>,
) -> Result<(u64, String), StorageError> {
    let mut file = fs::File::create(path)
        .await
        .map_err(|source| StorageError::Filesystem {
            path: path.to_path_buf(),
            source,
        })?;
    let mut sha256 = Sha256::new();
    let mut content_length = 0_u64;
    let mut chunks = pin!(chunks);

    while let Some(chunk) = chunks.next().await {
        let chunk = chunk?;
        file.write_all(&chunk)
            .await
            .map_err(|source| StorageError::Filesystem {
                path: path.to_path_buf(),
                source,
            })?;
        sha256.update(&chunk);
        content_length = content_length
            .checked_add(chunk.len() as u64)
            .ok_or_else(|| StorageError::StreamRead {
                message: "object stream is too large".to_owned(),
                payload_too_large: true,
            })?;
    }

    file.flush()
        .await
        .map_err(|source| StorageError::Filesystem {
            path: path.to_path_buf(),
            source,
        })?;

    Ok((content_length, hex::encode(sha256.finalize())))
}

async fn remove_file_if_exists(path: PathBuf) -> Result<(), StorageError> {
    fs::remove_file(&path)
        .await
        .map_err(|source| match source.kind() {
            io::ErrorKind::NotFound => StorageError::NotFound,
            _ => StorageError::Filesystem {
                path: path.clone(),
                source,
            },
        })
        .or_else(|error| match error {
            StorageError::NotFound => Ok(()),
            other => Err(other),
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{GcsCredentialsMode, GcsObjectStorageConfig};
    use bytes::Bytes;
    use futures_util::stream;
    use std::{
        fs as std_fs,
        sync::atomic::{AtomicU64, Ordering},
    };
    use url::Url;
    use uuid::Uuid;

    static TEMP_DIR_COUNTER: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn object_keys_include_workspace_id_and_stable_prefixes() {
        let workspace_id =
            WorkspaceId::from(Uuid::parse_str("00000000-0000-4000-8000-000000000123").unwrap());

        let key = ObjectKey::new(workspace_id, "evidence/submissions", "artifact.pdf").unwrap();

        assert_eq!(
            key.as_str(),
            "workspaces/00000000-0000-4000-8000-000000000123/evidence/submissions/artifact.pdf"
        );
    }

    #[test]
    fn invalid_path_traversal_key_segments_are_rejected() {
        let workspace_id = WorkspaceId::from(Uuid::new_v4());

        assert!(matches!(
            ObjectKey::new(workspace_id, "../evidence", "artifact.pdf"),
            Err(StorageError::InvalidKey)
        ));
        assert!(matches!(
            ObjectKey::new(workspace_id, "evidence", ".."),
            Err(StorageError::InvalidKey)
        ));
        assert!(matches!(
            ObjectKey::parse("workspaces/id/evidence\\artifact"),
            Err(StorageError::InvalidKey)
        ));
        assert!(matches!(
            ObjectKey::parse("/workspaces/id/evidence"),
            Err(StorageError::InvalidKey)
        ));
        assert!(matches!(
            ObjectKey::parse("not-workspaces/00000000-0000-4000-8000-000000000001/evidence/a"),
            Err(StorageError::InvalidKey)
        ));
        assert!(matches!(
            ObjectKey::parse("workspaces/not-a-uuid/evidence/a"),
            Err(StorageError::InvalidKey)
        ));
    }

    #[tokio::test]
    async fn upload_calculates_and_persists_length_and_sha256() {
        let root = temp_dir("calculates");
        let store = FilesystemObjectStore::new(&root).await.unwrap();
        let key = test_key("artifact.txt");

        let metadata = store
            .put_object(PutObjectRequest {
                key: key.clone(),
                content_type: "text/plain".to_owned(),
                chunks: chunks(["hello object storage"]),
            })
            .await
            .unwrap();

        assert_eq!(metadata.content_length, 20);
        assert_eq!(
            metadata.sha256,
            "f5cc4171a2e81eaba9b21e188e0d087d2dd3a190512fcc37b356c6da77adad93"
        );
        assert!(root
            .join("objects")
            .join(key.as_str())
            .try_exists()
            .unwrap());
        assert!(root
            .join("metadata")
            .join("workspaces")
            .join("00000000-0000-4000-8000-000000000001")
            .join("evidence")
            .join("artifact.txt.json")
            .try_exists()
            .unwrap());
    }

    #[tokio::test]
    async fn metadata_sidecar_round_trips_all_metadata_fields() {
        let store = FilesystemObjectStore::new(temp_dir("metadata"))
            .await
            .unwrap();
        let key = test_key("artifact.txt");

        let written = store
            .put_object(PutObjectRequest {
                key: key.clone(),
                content_type: "text/plain".to_owned(),
                chunks: chunks(["hello"]),
            })
            .await
            .unwrap();
        let read = store.head_object(key).await.unwrap();

        assert_eq!(read, written);
        assert_eq!(read.content_type, "text/plain");
        assert_eq!(read.content_length, 5);
        assert_eq!(
            read.sha256,
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }

    #[tokio::test]
    async fn copy_preserves_bytes_and_metadata_at_destination() {
        let store = FilesystemObjectStore::new(temp_dir("copy")).await.unwrap();
        let source = test_key("quarantine/artifact.txt");
        let destination = test_key("final/artifact.txt");

        let source_metadata = store
            .put_object(PutObjectRequest {
                key: source.clone(),
                content_type: "text/plain".to_owned(),
                chunks: chunks(["hello"]),
            })
            .await
            .unwrap();

        let copied = store
            .copy_object(source.clone(), destination.clone())
            .await
            .unwrap();

        assert_eq!(copied.key, destination);
        assert_eq!(copied.content_type, source_metadata.content_type);
        assert_eq!(copied.content_length, source_metadata.content_length);
        assert_eq!(copied.sha256, source_metadata.sha256);
        assert_eq!(store.get_object(source).await.unwrap().bytes, b"hello");
        assert_eq!(store.get_object(copied.key).await.unwrap().bytes, b"hello");
    }

    #[tokio::test]
    async fn delete_is_idempotent_and_get_after_delete_returns_not_found() {
        let store = FilesystemObjectStore::new(temp_dir("delete"))
            .await
            .unwrap();
        let key = test_key("artifact.txt");
        store
            .put_object(PutObjectRequest {
                key: key.clone(),
                content_type: "text/plain".to_owned(),
                chunks: chunks(["hello"]),
            })
            .await
            .unwrap();

        store.delete_object(key.clone()).await.unwrap();
        store.delete_object(key.clone()).await.unwrap();

        assert!(matches!(
            store.head_object(key.clone()).await,
            Err(StorageError::NotFound)
        ));
        assert!(matches!(
            store.get_object(key).await,
            Err(StorageError::NotFound)
        ));
    }

    #[tokio::test]
    async fn filesystem_config_upload_head_get_delete_through_trait() {
        let root = temp_dir("trait");
        let config = ObjectStorageConfig::Filesystem { root: root.clone() };
        let store = FilesystemObjectStore::from_config(&config).await.unwrap();
        let store_ref: &FilesystemObjectStore = &store;
        let key = test_key("artifact.bin");

        let metadata = ObjectStore::put_object(
            store_ref,
            PutObjectRequest {
                key: key.clone(),
                content_type: "application/octet-stream".to_owned(),
                chunks: byte_chunks([vec![1, 2], vec![3, 4]]),
            },
        )
        .await
        .unwrap();

        assert_eq!(
            ObjectStore::head_object(store_ref, key.clone())
                .await
                .unwrap(),
            metadata
        );
        assert_eq!(
            ObjectStore::get_object(store_ref, key.clone())
                .await
                .unwrap(),
            ObjectStream {
                metadata,
                bytes: vec![1, 2, 3, 4],
            }
        );
        assert!(root
            .join("objects")
            .join(key.as_str())
            .try_exists()
            .unwrap());
        assert!(root
            .join("metadata")
            .join("workspaces")
            .join("00000000-0000-4000-8000-000000000001")
            .join("evidence")
            .join("artifact.bin.json")
            .try_exists()
            .unwrap());

        ObjectStore::delete_object(store_ref, key.clone())
            .await
            .unwrap();
        assert!(matches!(
            ObjectStore::get_object(store_ref, key).await,
            Err(StorageError::NotFound)
        ));
    }

    #[tokio::test]
    async fn unsupported_gcs_config_returns_clear_error() {
        let config = ObjectStorageConfig::Gcs(GcsObjectStorageConfig {
            bucket: "bucket".to_owned(),
            endpoint_override: Some(Url::parse("http://localhost:4443").unwrap()),
            credentials_mode: GcsCredentialsMode::Anonymous,
            object_key_prefix: "proofplane".to_owned(),
        });

        assert!(matches!(
            FilesystemObjectStore::from_config(&config).await,
            Err(StorageError::UnsupportedBackend)
        ));
    }

    #[tokio::test]
    async fn upload_accepts_multiple_chunks_and_persists_combined_metadata() {
        let store = FilesystemObjectStore::new(temp_dir("multi-chunk"))
            .await
            .unwrap();
        let key = test_key("artifact.txt");

        let metadata = store
            .put_object(PutObjectRequest {
                key: key.clone(),
                content_type: "text/plain".to_owned(),
                chunks: chunks(["hello ", "object ", "storage"]),
            })
            .await
            .unwrap();

        assert_eq!(metadata.content_length, 20);
        assert_eq!(
            metadata.sha256,
            "f5cc4171a2e81eaba9b21e188e0d087d2dd3a190512fcc37b356c6da77adad93"
        );
        assert_eq!(
            store.get_object(key).await.unwrap().bytes,
            b"hello object storage"
        );
    }

    #[tokio::test]
    async fn upload_removes_partial_object_when_stream_fails() {
        let root = temp_dir("stream-failure");
        let store = FilesystemObjectStore::new(&root).await.unwrap();
        let key = test_key("artifact.txt");

        let result = store
            .put_object(PutObjectRequest {
                key: key.clone(),
                content_type: "text/plain".to_owned(),
                chunks: Box::pin(stream::iter([
                    Ok(Bytes::from_static(b"partial")),
                    Err(StorageError::StreamRead {
                        message: "multipart stopped".to_owned(),
                        payload_too_large: false,
                    }),
                ])),
            })
            .await;

        assert!(matches!(result, Err(StorageError::StreamRead { .. })));
        assert!(!root
            .join("objects")
            .join(key.as_str())
            .try_exists()
            .unwrap());
        assert!(!root
            .join("metadata")
            .join("workspaces")
            .join("00000000-0000-4000-8000-000000000001")
            .join("evidence")
            .join("artifact.txt.json")
            .try_exists()
            .unwrap());
    }

    fn test_key(object_name: &str) -> ObjectKey {
        let workspace_id =
            WorkspaceId::from(Uuid::parse_str("00000000-0000-4000-8000-000000000001").unwrap());
        ObjectKey::new(workspace_id, "evidence", object_name).unwrap()
    }

    fn temp_dir(name: &str) -> PathBuf {
        let counter = TEMP_DIR_COUNTER.fetch_add(1, Ordering::SeqCst);
        let path = std::env::temp_dir().join(format!(
            "proofplane-object-storage-{name}-{}-{counter}",
            std::process::id()
        ));
        let _ = std_fs::remove_dir_all(&path);
        path
    }

    fn chunks<const N: usize>(
        chunks: [&'static str; N],
    ) -> impl Stream<Item = Result<Bytes, StorageError>> {
        byte_chunks(chunks.map(|chunk| chunk.as_bytes().to_vec()))
    }

    fn byte_chunks<const N: usize>(
        chunks: [Vec<u8>; N],
    ) -> impl Stream<Item = Result<Bytes, StorageError>> {
        stream::iter(chunks.into_iter().map(|chunk| Ok(Bytes::from(chunk))))
    }
}
