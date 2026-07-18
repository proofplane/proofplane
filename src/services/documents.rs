use bytes::Bytes;
use futures_core::Stream;

use crate::{
    domain::{DocumentOwner, WorkspaceId},
    object_storage::{
        FilesystemObjectStore, ObjectKey, ObjectStore, PutObjectRequest, StorageError,
    },
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

pub(crate) async fn stage_document<S>(
    object_store: &FilesystemObjectStore,
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
    let prefix = match owner {
        DocumentOwner::EvidenceSubmission(id) => {
            format!("quarantine/evidence-submissions/{id}/documents/{upload_id}")
        }
        DocumentOwner::Policy(id) => {
            format!("quarantine/policies/{id}/documents/{upload_id}")
        }
    };
    let key = ObjectKey::new(workspace_id, prefix, &filename)?;
    let metadata = object_store
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

pub(crate) async fn delete_staged_document(
    object_store: &FilesystemObjectStore,
    object_key: &str,
) -> Result<(), Error> {
    object_store
        .delete_object(&ObjectKey::parse(object_key.to_owned())?)
        .await?;
    Ok(())
}
