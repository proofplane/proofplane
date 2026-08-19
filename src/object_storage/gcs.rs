use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use futures_core::Stream;
use futures_util::StreamExt;
use google_cloud_storage::{
    client::{Storage, StorageControl},
    error::{ReadError, WriteError},
    model::Object,
    streaming_source::StreamingSource,
};
use google_cloud_wkt::FieldMask;
use sha2::{Digest, Sha256};
use tokio::sync::{mpsc, Mutex};

use super::{
    ObjectByteStream, ObjectKey, ObjectMetadata, ObjectStore, ObjectStream, PutObjectRequest,
    SanitizedProviderError, StorageError,
};

const SHA256_METADATA_KEY: &str = "proofplane-sha256";

fn prefixed_name(prefix: &str, key: &ObjectKey) -> String {
    format!("{prefix}/{}", key.as_str())
}

#[derive(Clone, Debug)]
pub struct GcsObjectStore {
    clients: Arc<GcsClients>,
    bucket: String,
    object_key_prefix: String,
}

#[derive(Debug)]
struct GcsClients {
    data: Storage,
    control: StorageControl,
}

impl GcsObjectStore {
    pub async fn new(
        bucket: impl Into<String>,
        object_key_prefix: impl Into<String>,
    ) -> Result<Self, StorageError> {
        let data = Storage::builder()
            .build()
            .await
            .map_err(|_| client_initialization_error())?;
        let control = StorageControl::builder()
            .build()
            .await
            .map_err(|_| client_initialization_error())?;

        Ok(Self {
            clients: Arc::new(GcsClients { data, control }),
            bucket: bucket.into(),
            object_key_prefix: object_key_prefix.into(),
        })
    }

    fn bucket_resource(&self) -> String {
        format!("projects/_/buckets/{}", self.bucket)
    }

    fn physical_name(&self, key: &ObjectKey) -> String {
        prefixed_name(&self.object_key_prefix, key)
    }

    async fn delete_physical_object(&self, name: &str) -> Result<(), StorageError> {
        match self
            .clients
            .control
            .delete_object()
            .set_bucket(self.bucket_resource())
            .set_object(name)
            .send()
            .await
        {
            Ok(()) => Ok(()),
            Err(error) => match classify_provider_error(error) {
                StorageError::NotFound => Ok(()),
                other => Err(other),
            },
        }
    }

    async fn cleanup_failed_write(&self, name: &str) {
        if let Err(error) = self.delete_physical_object(name).await {
            crate::observability::record_cleanup_failure(
                &error,
                "gcs_object_storage_partial_write",
                None,
            );
        }
    }
}

#[async_trait]
impl ObjectStore for GcsObjectStore {
    async fn put_object<S>(
        &self,
        request: PutObjectRequest<S>,
    ) -> Result<ObjectMetadata, StorageError>
    where
        S: Stream<Item = Result<Bytes, StorageError>> + Send,
    {
        let physical_name = self.physical_name(&request.key);
        let digest = Arc::new(Mutex::new(UploadDigest::default()));
        let (sender, receiver) = mpsc::channel(1);
        let source = UploadSource {
            receiver: Mutex::new(receiver),
            digest: digest.clone(),
        };
        let upload = self
            .clients
            .data
            .write_object(self.bucket_resource(), physical_name.clone(), source)
            .set_content_type(request.content_type)
            .send_buffered();
        let forward = forward_chunks(request.chunks, sender);
        let (uploaded, ()) = tokio::join!(upload, forward);

        let uploaded = match uploaded {
            Ok(object) => object,
            Err(error) => {
                self.cleanup_failed_write(&physical_name).await;
                return Err(classify_provider_error(error));
            }
        };

        let (content_length, sha256) = digest.lock().await.finish();
        if uploaded.size < 0 || uploaded.size as u64 != content_length {
            self.cleanup_failed_write(&physical_name).await;
            return Err(StorageError::Integrity);
        }

        let metadata_object = Object::new()
            .set_bucket(self.bucket_resource())
            .set_name(physical_name.clone())
            .set_metadata([(SHA256_METADATA_KEY, sha256.as_str())]);
        if let Err(error) = self
            .clients
            .control
            .update_object()
            .set_object(metadata_object)
            .set_if_generation_match(uploaded.generation)
            .set_update_mask(FieldMask::default().set_paths(["metadata"]))
            .send()
            .await
        {
            self.cleanup_failed_write(&physical_name).await;
            return Err(classify_provider_error(error));
        }

        let metadata = match self.head_object(&request.key).await {
            Ok(metadata) => metadata,
            Err(error) => {
                self.cleanup_failed_write(&physical_name).await;
                return Err(error);
            }
        };
        if metadata.content_length != content_length || metadata.sha256 != sha256 {
            self.cleanup_failed_write(&physical_name).await;
            return Err(StorageError::Integrity);
        }

        Ok(metadata)
    }

    async fn get_object(&self, key: &ObjectKey) -> Result<ObjectStream, StorageError> {
        let metadata = self.head_object(key).await?;
        let reader = self
            .clients
            .data
            .read_object(self.bucket_resource(), self.physical_name(key))
            .send()
            .await
            .map_err(classify_provider_error)?;
        let chunks: ObjectByteStream = Box::pin(futures_util::stream::unfold(
            reader,
            |mut reader| async move {
                reader
                    .next()
                    .await
                    .map(|result| (result.map_err(classify_provider_error), reader))
            },
        ));

        Ok(ObjectStream { metadata, chunks })
    }

    async fn head_object(&self, key: &ObjectKey) -> Result<ObjectMetadata, StorageError> {
        let physical_name = self.physical_name(key);
        let object = self
            .clients
            .control
            .get_object()
            .set_bucket(self.bucket_resource())
            .set_object(physical_name.clone())
            .send()
            .await
            .map_err(classify_provider_error)?;

        map_object_metadata(key, &physical_name, &object)
    }

    async fn copy_object(
        &self,
        source: &ObjectKey,
        destination: &ObjectKey,
    ) -> Result<ObjectMetadata, StorageError> {
        if !source.has_same_workspace(destination) {
            return Err(StorageError::InvalidKey);
        }

        let source_metadata = self.head_object(source).await?;
        let source_name = self.physical_name(source);
        let destination_name = self.physical_name(destination);
        let bucket = self.bucket_resource();
        let mut rewrite_token = None;

        loop {
            let mut request = self
                .clients
                .control
                .rewrite_object()
                .set_source_bucket(bucket.clone())
                .set_source_object(source_name.clone())
                .set_destination_bucket(bucket.clone())
                .set_destination_name(destination_name.clone());
            if let Some(token) = rewrite_token.take() {
                request = request.set_rewrite_token(token);
            }
            let response = request.send().await.map_err(classify_provider_error)?;
            if response.done {
                break;
            }
            if response.rewrite_token.is_empty() {
                self.cleanup_failed_write(&destination_name).await;
                return Err(StorageError::Integrity);
            }
            rewrite_token = Some(response.rewrite_token);
        }

        let destination_metadata = self.head_object(destination).await?;
        if destination_metadata.content_type != source_metadata.content_type
            || destination_metadata.content_length != source_metadata.content_length
            || destination_metadata.sha256 != source_metadata.sha256
        {
            self.cleanup_failed_write(&destination_name).await;
            return Err(StorageError::Integrity);
        }

        Ok(destination_metadata)
    }

    async fn delete_object(&self, key: &ObjectKey) -> Result<(), StorageError> {
        self.delete_physical_object(&self.physical_name(key)).await
    }
}

#[derive(Default)]
struct UploadDigest {
    sha256: Sha256,
    content_length: u64,
}

impl UploadDigest {
    fn update(&mut self, chunk: &Bytes) -> Result<(), StorageError> {
        self.content_length = self
            .content_length
            .checked_add(chunk.len() as u64)
            .ok_or_else(|| StorageError::StreamRead {
                message: "object stream is too large".to_owned(),
                payload_too_large: true,
            })?;
        self.sha256.update(chunk);
        Ok(())
    }

    fn finish(&mut self) -> (u64, String) {
        let sha256 = hex::encode(self.sha256.finalize_reset());
        (self.content_length, sha256)
    }
}

struct UploadSource {
    receiver: Mutex<mpsc::Receiver<Result<Bytes, StorageError>>>,
    digest: Arc<Mutex<UploadDigest>>,
}

impl StreamingSource for UploadSource {
    type Error = StorageError;

    async fn next(&mut self) -> Option<Result<Bytes, Self::Error>> {
        let chunk = self.receiver.lock().await.recv().await?;
        match chunk {
            Ok(chunk) => match self.digest.lock().await.update(&chunk) {
                Ok(()) => Some(Ok(chunk)),
                Err(error) => Some(Err(error)),
            },
            Err(error) => Some(Err(error)),
        }
    }
}

async fn forward_chunks<S>(chunks: S, sender: mpsc::Sender<Result<Bytes, StorageError>>)
where
    S: Stream<Item = Result<Bytes, StorageError>>,
{
    let mut chunks = std::pin::pin!(chunks);
    while let Some(chunk) = chunks.next().await {
        let is_error = chunk.is_err();
        if sender.send(chunk).await.is_err() || is_error {
            break;
        }
    }
}

fn client_initialization_error() -> StorageError {
    StorageError::Authentication {
        source: SanitizedProviderError::new("client initialization", None),
    }
}

fn map_object_metadata(
    key: &ObjectKey,
    physical_name: &str,
    object: &Object,
) -> Result<ObjectMetadata, StorageError> {
    if object.name != physical_name || object.size < 0 || object.content_type.is_empty() {
        return Err(StorageError::Integrity);
    }
    let sha256 = object
        .metadata
        .get(SHA256_METADATA_KEY)
        .cloned()
        .ok_or(StorageError::Integrity)?;
    if sha256.len() != 64 || !sha256.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(StorageError::Integrity);
    }

    Ok(ObjectMetadata {
        key: key.clone(),
        content_type: object.content_type.clone(),
        content_length: object.size as u64,
        sha256,
    })
}

fn classify_provider_error(error: google_cloud_storage::Error) -> StorageError {
    if let Some(error) = stream_error_in_chain(&error) {
        return error;
    }
    if has_checksum_mismatch(&error) {
        return StorageError::Integrity;
    }

    let http_status = error.http_status_code();
    let status_name = error.status().map(|status| status.code.name());

    if matches!(http_status, Some(404)) || status_name == Some("NOT_FOUND") {
        StorageError::NotFound
    } else if error.is_authentication()
        || matches!(http_status, Some(401 | 403))
        || matches!(status_name, Some("UNAUTHENTICATED" | "PERMISSION_DENIED"))
    {
        StorageError::Authentication {
            source: SanitizedProviderError::new("authentication", http_status),
        }
    } else if error.is_timeout()
        || error.is_exhausted()
        || error.is_connect()
        || error.is_io()
        || matches!(http_status, Some(408 | 429 | 500..=599))
        || matches!(
            status_name,
            Some("DEADLINE_EXCEEDED" | "RESOURCE_EXHAUSTED" | "UNAVAILABLE")
        )
    {
        StorageError::Unavailable {
            source: SanitizedProviderError::new("unavailable", http_status),
        }
    } else {
        StorageError::Provider {
            source: SanitizedProviderError::new("provider", http_status),
        }
    }
}

fn stream_error_in_chain(error: &(dyn std::error::Error + 'static)) -> Option<StorageError> {
    let mut current = error.source();
    while let Some(source) = current {
        if let Some(StorageError::StreamRead {
            message,
            payload_too_large,
        }) = source.downcast_ref::<StorageError>()
        {
            return Some(StorageError::StreamRead {
                message: message.clone(),
                payload_too_large: *payload_too_large,
            });
        }
        current = source.source();
    }
    None
}

fn has_checksum_mismatch(error: &(dyn std::error::Error + 'static)) -> bool {
    let mut current = error.source();
    while let Some(source) = current {
        if matches!(
            source.downcast_ref::<ReadError>(),
            Some(ReadError::ChecksumMismatch(_))
        ) || matches!(
            source.downcast_ref::<WriteError>(),
            Some(WriteError::ChecksumMismatch { .. })
        ) {
            return true;
        }
        current = source.source();
    }
    false
}

#[cfg(test)]
mod tests {
    use bytes::Bytes;
    use futures_util::stream;
    use google_cloud_storage::model::Object;

    use super::*;
    use crate::domain::WorkspaceId;

    #[test]
    fn provider_metadata_maps_to_the_logical_key() {
        let key = test_key();
        let physical_name = format!("isolated/run/{}", key.as_str());
        let object = Object::new()
            .set_name(physical_name.clone())
            .set_content_type("text/plain")
            .set_size(5)
            .set_metadata([(
                SHA256_METADATA_KEY,
                "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824",
            )]);

        let metadata = map_object_metadata(&key, &physical_name, &object).unwrap();

        assert_eq!(metadata.key, key);
        assert_eq!(metadata.content_type, "text/plain");
        assert_eq!(metadata.content_length, 5);
        assert_eq!(
            metadata.sha256,
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }

    #[test]
    fn incomplete_provider_metadata_is_an_integrity_error() {
        let key = test_key();
        let physical_name = format!("isolated/run/{}", key.as_str());
        let missing_digest = Object::new()
            .set_name(physical_name.clone())
            .set_content_type("text/plain")
            .set_size(5);
        let wrong_name = Object::new()
            .set_name("other/object")
            .set_content_type("text/plain")
            .set_size(5)
            .set_metadata([(SHA256_METADATA_KEY, "a".repeat(64))]);

        assert!(matches!(
            map_object_metadata(&key, &physical_name, &missing_digest),
            Err(StorageError::Integrity)
        ));
        assert!(matches!(
            map_object_metadata(&key, &physical_name, &wrong_name),
            Err(StorageError::Integrity)
        ));
    }

    #[test]
    fn provider_errors_are_classified_and_redacted() {
        let error = google_cloud_storage::Error::http(
            403,
            http::HeaderMap::new(),
            Bytes::from_static(b"credential ya29.secret-token was rejected"),
        );

        let classified = classify_provider_error(error);

        assert!(matches!(classified, StorageError::Authentication { .. }));
        assert!(!classified.to_string().contains("ya29.secret-token"));
        assert!(!format!("{classified:?}").contains("ya29.secret-token"));

        for (status, expected) in [
            (404, "not_found"),
            (408, "unavailable"),
            (429, "unavailable"),
            (503, "unavailable"),
            (400, "provider"),
        ] {
            let error = google_cloud_storage::Error::http(
                status,
                http::HeaderMap::new(),
                Bytes::from_static(b"provider details"),
            );
            let classified = classify_provider_error(error);
            let actual = match classified {
                StorageError::NotFound => "not_found",
                StorageError::Unavailable { .. } => "unavailable",
                StorageError::Provider { .. } => "provider",
                other => panic!("unexpected classification for {status}: {other:?}"),
            };
            assert_eq!(actual, expected);
        }
    }

    #[tokio::test]
    async fn upload_source_hashes_chunks_and_preserves_stream_failures() {
        let digest = Arc::new(Mutex::new(UploadDigest::default()));
        let (sender, receiver) = mpsc::channel(1);
        let mut source = UploadSource {
            receiver: Mutex::new(receiver),
            digest: digest.clone(),
        };
        let forward = forward_chunks(
            stream::iter([
                Ok(Bytes::from_static(b"hello ")),
                Ok(Bytes::from_static(b"world")),
                Err(StorageError::StreamRead {
                    message: "multipart stopped".to_owned(),
                    payload_too_large: false,
                }),
            ]),
            sender,
        );
        let consume = async {
            assert_eq!(source.next().await.unwrap().unwrap(), b"hello "[..]);
            assert_eq!(source.next().await.unwrap().unwrap(), b"world"[..]);
            assert!(matches!(
                source.next().await.unwrap(),
                Err(StorageError::StreamRead { .. })
            ));
        };

        tokio::join!(forward, consume);
        let (length, sha256) = digest.lock().await.finish();
        assert_eq!(length, 11);
        assert_eq!(
            sha256,
            "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
        );
    }

    #[test]
    fn physical_names_add_the_prefix_without_changing_logical_keys() {
        let key = test_key();

        assert_eq!(
            prefixed_name("contract/run-a", &key),
            format!("contract/run-a/{}", key.as_str())
        );
        assert_eq!(
            prefixed_name("contract/run-b", &key),
            format!("contract/run-b/{}", key.as_str())
        );
        assert_eq!(
            key.as_str(),
            "workspaces/00000000-0000-4000-8000-000000000001/evidence/artifact.txt"
        );
    }

    fn test_key() -> ObjectKey {
        ObjectKey::new(
            WorkspaceId::from(
                uuid::Uuid::parse_str("00000000-0000-4000-8000-000000000001").unwrap(),
            ),
            "evidence",
            "artifact.txt",
        )
        .unwrap()
    }
}
