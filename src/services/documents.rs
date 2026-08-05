use std::sync::{
    atomic::{AtomicU32, AtomicU64, Ordering},
    Arc,
};

use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine};
use bytes::Bytes;
use futures_core::Stream;
use futures_util::StreamExt;

use crate::{
    domain::{DocumentOwner, EvidenceSubmissionId, WorkspaceId},
    object_storage::{ObjectKey, PutObjectRequest, QuarantineObjectStore, StorageError},
};

use super::Error;

pub(crate) struct StagedDocument {
    pub filename: String,
    pub content_type: String,
    pub content_length: i64,
    pub object_key: String,
    pub checksum_sha256: String,
    pub checksum_crc32c: String,
}

pub(crate) struct BoundedStagingRequest {
    pub workspace_id: WorkspaceId,
    pub owner: DocumentOwner,
    pub upload_id: uuid::Uuid,
    pub filename: String,
    pub content_type: String,
    pub max_bytes: usize,
    pub cleanup_operation: &'static str,
}

pub(crate) async fn stage_document<S>(
    quarantine_store: &QuarantineObjectStore,
    workspace_id: WorkspaceId,
    owner: DocumentOwner,
    upload_id: uuid::Uuid,
    filename: String,
    content_type: String,
    chunks: S,
) -> Result<StagedDocument, Error>
where
    S: Stream<Item = Result<Bytes, StorageError>> + Send,
{
    // The quarantine store is a separate store, so the key does not name the
    // lifecycle stage. A staged key ends in the upload id and the finalized key
    // ends in the document id, so the two never collide.
    let prefix = match owner {
        DocumentOwner::EvidenceSubmission(id) => {
            format!("evidence-submissions/{id}/documents/{upload_id}")
        }
        DocumentOwner::Policy(id) => {
            format!("policies/{id}/documents/{upload_id}")
        }
    };
    let key = ObjectKey::new(workspace_id, prefix, &filename)?;
    let metadata = quarantine_store
        .put_object(PutObjectRequest {
            key,
            content_type,
            chunks,
        })
        .await?;
    let content_length = i64::try_from(metadata.content_length).map_err(|_| {
        Error::Storage(StorageError::StreamRead {
            message: "file is too large".to_owned(),
            payload_too_large: true,
        })
    })?;

    Ok(StagedDocument {
        filename,
        content_type: metadata.content_type,
        content_length,
        object_key: metadata.key.to_string(),
        checksum_sha256: metadata.sha256,
        checksum_crc32c: String::new(),
    })
}

pub(crate) async fn stage_evidence_document<S>(
    quarantine_store: &QuarantineObjectStore,
    workspace_id: WorkspaceId,
    evidence_submission_id: EvidenceSubmissionId,
    filename: String,
    content_type: String,
    max_bytes: usize,
    chunks: S,
) -> Result<StagedDocument, Error>
where
    S: Stream<Item = Result<Bytes, StorageError>> + Send,
{
    stage_bounded_document(
        quarantine_store,
        BoundedStagingRequest {
            workspace_id,
            owner: DocumentOwner::EvidenceSubmission(evidence_submission_id),
            upload_id: uuid::Uuid::new_v4(),
            filename,
            content_type,
            max_bytes,
            cleanup_operation: "evidence_document_length_mismatch",
        },
        chunks,
    )
    .await
}

pub(crate) async fn stage_bounded_document<S>(
    quarantine_store: &QuarantineObjectStore,
    request: BoundedStagingRequest,
    chunks: S,
) -> Result<StagedDocument, Error>
where
    S: Stream<Item = Result<Bytes, StorageError>> + Send,
{
    let content_length = Arc::new(AtomicU64::new(0));
    let checksum_crc32c = Arc::new(AtomicU32::new(0));
    let stream_content_length = Arc::clone(&content_length);
    let stream_checksum_crc32c = Arc::clone(&checksum_crc32c);
    let max_bytes = u64::try_from(request.max_bytes).map_err(|_| stream_too_large())?;
    let chunks = chunks.map(move |chunk| {
        let chunk = chunk?;
        let chunk_length = u64::try_from(chunk.len()).map_err(|_| stream_too_large())?;
        let current_length = stream_content_length.load(Ordering::Relaxed);
        let next_length = current_length
            .checked_add(chunk_length)
            .ok_or_else(stream_too_large)?;
        if next_length > max_bytes {
            return Err(stream_too_large());
        }
        stream_content_length.store(next_length, Ordering::Relaxed);

        let current_crc32c = stream_checksum_crc32c.load(Ordering::Relaxed);
        stream_checksum_crc32c.store(
            crc32c::crc32c_append(current_crc32c, &chunk),
            Ordering::Relaxed,
        );
        Ok(chunk)
    });

    let mut staged = stage_document(
        quarantine_store,
        request.workspace_id,
        request.owner,
        request.upload_id,
        request.filename,
        request.content_type,
        chunks,
    )
    .await?;
    let actual_content_length = content_length.load(Ordering::Relaxed);
    if u64::try_from(staged.content_length) != Ok(actual_content_length) {
        if let Err(error) = delete_staged_document(quarantine_store, &staged.object_key).await {
            crate::observability::record_cleanup_failure(&error, request.cleanup_operation, None);
        }
        return Err(Error::Storage(StorageError::StreamRead {
            message: "staged object length does not match received bytes".to_owned(),
            payload_too_large: false,
        }));
    }
    staged.checksum_crc32c =
        BASE64_STANDARD.encode(checksum_crc32c.load(Ordering::Relaxed).to_be_bytes());
    Ok(staged)
}

fn stream_too_large() -> StorageError {
    StorageError::StreamRead {
        message: "request payload is too large".to_owned(),
        payload_too_large: true,
    }
}

pub(crate) async fn delete_staged_document(
    quarantine_store: &QuarantineObjectStore,
    object_key: &str,
) -> Result<(), Error> {
    quarantine_store
        .delete_object(&ObjectKey::parse(object_key.to_owned())?)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf};

    use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine};
    use bytes::Bytes;
    use futures_util::{stream, StreamExt};
    use uuid::Uuid;

    use super::stage_evidence_document;
    use crate::{
        config::{FilesystemObjectStorageConfig, ObjectStorageConfig},
        domain::{EvidenceSubmissionId, WorkspaceId},
        object_storage::{DocumentObjectStores, ObjectKey, StorageError},
        services::Error,
    };

    #[tokio::test]
    async fn evidence_stream_stages_bytes_and_computes_all_metadata() {
        let root = temp_dir("metadata");
        let stores = document_stores(&root).await;
        let workspace_id = WorkspaceId::from(Uuid::new_v4());
        let submission_id = EvidenceSubmissionId::from(Uuid::new_v4());

        let staged = stage_evidence_document(
            &stores.quarantine,
            workspace_id,
            submission_id,
            "report.txt".to_owned(),
            "text/plain".to_owned(),
            11,
            stream::iter([
                Ok(Bytes::from_static(b"hello ")),
                Ok(Bytes::from_static(b"world")),
            ]),
        )
        .await
        .expect("evidence stream stages");

        assert_eq!(staged.filename, "report.txt");
        assert_eq!(staged.content_type, "text/plain");
        assert_eq!(staged.content_length, 11);
        assert_eq!(
            staged.checksum_sha256,
            "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
        );
        assert_eq!(
            staged.checksum_crc32c,
            BASE64_STANDARD.encode(crc32c::crc32c(b"hello world").to_be_bytes())
        );

        let staged_key =
            ObjectKey::parse(staged.object_key).expect("staged object key remains valid");
        let object = stores
            .quarantine
            .get_object(&staged_key)
            .await
            .expect("staged object reads");
        let bytes = object
            .chunks
            .map(|chunk| chunk.expect("stored chunk reads"))
            .collect::<Vec<_>>()
            .await
            .concat();
        assert_eq!(bytes, b"hello world");

        tokio::fs::remove_dir_all(root)
            .await
            .expect("test storage cleans up");
    }

    #[tokio::test]
    async fn evidence_stream_rejects_bytes_over_limit_and_removes_partial_object() {
        let root = temp_dir("over-limit");
        let stores = document_stores(&root).await;

        let result = stage_evidence_document(
            &stores.quarantine,
            WorkspaceId::from(Uuid::new_v4()),
            EvidenceSubmissionId::from(Uuid::new_v4()),
            "report.txt".to_owned(),
            "text/plain".to_owned(),
            5,
            stream::iter([
                Ok(Bytes::from_static(b"abc")),
                Ok(Bytes::from_static(b"def")),
            ]),
        )
        .await;

        assert!(matches!(
            result,
            Err(Error::Storage(StorageError::StreamRead {
                payload_too_large: true,
                ..
            }))
        ));
        assert!(files_under(&root).is_empty());

        tokio::fs::remove_dir_all(root)
            .await
            .expect("test storage cleans up");
    }

    #[tokio::test]
    async fn evidence_stream_failure_removes_partial_object() {
        let root = temp_dir("stream-failure");
        let stores = document_stores(&root).await;

        let result = stage_evidence_document(
            &stores.quarantine,
            WorkspaceId::from(Uuid::new_v4()),
            EvidenceSubmissionId::from(Uuid::new_v4()),
            "report.txt".to_owned(),
            "text/plain".to_owned(),
            10,
            stream::iter([
                Ok(Bytes::from_static(b"partial")),
                Err(StorageError::StreamRead {
                    message: "connection interrupted".to_owned(),
                    payload_too_large: false,
                }),
            ]),
        )
        .await;

        assert!(matches!(
            result,
            Err(Error::Storage(StorageError::StreamRead {
                payload_too_large: false,
                ..
            }))
        ));
        assert!(files_under(&root).is_empty());

        tokio::fs::remove_dir_all(root)
            .await
            .expect("test storage cleans up");
    }

    async fn document_stores(root: &std::path::Path) -> DocumentObjectStores {
        DocumentObjectStores::from_config(&ObjectStorageConfig::Filesystem(
            FilesystemObjectStorageConfig {
                quarantine_root: root.join("quarantine"),
                evidence_root: root.join("evidence"),
            },
        ))
        .await
        .expect("object stores initialize")
    }

    fn temp_dir(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "proofplane-document-staging-{name}-{}",
            Uuid::new_v4()
        ))
    }

    fn files_under(root: &std::path::Path) -> Vec<PathBuf> {
        let Ok(entries) = fs::read_dir(root) else {
            return Vec::new();
        };
        entries
            .filter_map(Result::ok)
            .flat_map(|entry| {
                let path = entry.path();
                if path.is_dir() {
                    files_under(&path)
                } else {
                    vec![path]
                }
            })
            .collect()
    }
}
