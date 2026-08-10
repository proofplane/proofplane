use crate::{
    domain::{Document, DocumentIdentity, DocumentUploadStatus, UserId, WorkspaceId},
    persistence::Error,
    read_models::TypedDocumentUploadWork,
};
use tokio_postgres::Row;
use uuid::Uuid;

use super::ReadExecutor;

pub(crate) struct DocumentReads<'a, E> {
    executor: &'a E,
}
impl<'a, E> DocumentReads<'a, E> {
    pub(crate) fn new(executor: &'a E) -> Self {
        Self { executor }
    }
}
impl<E: ReadExecutor> DocumentReads<'_, E> {
    pub async fn load_pending_upload_work(
        &self,
        identity: DocumentIdentity,
        object_key: &str,
    ) -> Result<Option<TypedDocumentUploadWork>, Error> {
        self.load(identity, object_key, DocumentUploadStatus::PendingUpload)
            .await
    }
    pub async fn load_finalizing_upload_work(
        &self,
        identity: DocumentIdentity,
        object_key: &str,
    ) -> Result<Option<TypedDocumentUploadWork>, Error> {
        self.load(identity, object_key, DocumentUploadStatus::Finalizing)
            .await
    }
    async fn load(
        &self,
        identity: DocumentIdentity,
        object_key: &str,
        status: DocumentUploadStatus,
    ) -> Result<Option<TypedDocumentUploadWork>, Error> {
        let owner = identity.owner();
        self.executor
            .query_opt(
                SQL,
                &[
                    &identity.document_uuid(),
                    &owner.owner_type(),
                    &owner.owner_uuid(),
                    &object_key,
                    &status.as_str(),
                ],
            )
            .await?
            .map(|row| {
                Ok(TypedDocumentUploadWork {
                    workspace_id: WorkspaceId::from(row.try_get::<_, uuid::Uuid>("workspace_id")?),
                    identity,
                    filename: row.try_get("filename")?,
                    content_type: row.try_get("content_type")?,
                    content_length: row.try_get("content_length")?,
                    object_key: row.try_get("object_key")?,
                    checksum_sha256: row.try_get("checksum_sha256")?,
                    upload_status: row.try_get::<_, String>("upload_status")?.parse()?,
                })
            })
            .transpose()
    }
}

const SQL: &str = "SELECT d.workspace_id, d.filename, d.content_type, d.content_length, d.object_key, d.checksum_sha256, d.upload_status FROM documents d WHERE d.id = $1 AND d.owner_type = $2 AND d.owner_id = $3 AND d.object_key = $4 AND d.upload_status = $5 AND d.archived = false AND (d.owner_type <> 'policy' OR EXISTS (SELECT 1 FROM policies p WHERE p.id = d.owner_id AND p.workspace_id = d.workspace_id AND p.archived_at IS NULL))";

pub(super) fn document_from_row(row: &Row, identity: DocumentIdentity) -> Result<Document, Error> {
    Ok(Document {
        identity,
        workspace_id: WorkspaceId::from(row.try_get::<_, Uuid>("workspace_id")?),
        created_by_user_id: UserId::from(row.try_get::<_, Uuid>("created_by_user_id")?),
        filename: row.try_get("filename")?,
        content_type: row.try_get("content_type")?,
        content_length: row.try_get("content_length")?,
        object_key: row.try_get("object_key")?,
        checksum_sha256: row.try_get("checksum_sha256")?,
        checksum_crc32c: row.try_get("checksum_crc32c")?,
        upload_status: row
            .try_get::<_, String>("upload_status")?
            .parse::<DocumentUploadStatus>()?,
        archived: row.try_get("archived").unwrap_or(false),
        created_at: row.try_get("created_at")?,
    })
}
