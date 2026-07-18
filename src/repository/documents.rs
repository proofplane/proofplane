use tokio_postgres::Row;
use uuid::Uuid;

use crate::domain::{Document, DocumentIdentity, DocumentUploadStatus, UserId, WorkspaceId};

use super::{Error, Postgres, TransactionContext};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypedDocumentUploadWork {
    pub workspace_id: WorkspaceId,
    pub identity: DocumentIdentity,
    pub filename: String,
    pub content_type: String,
    pub content_length: i64,
    pub object_key: String,
    pub checksum_sha256: String,
    pub upload_status: DocumentUploadStatus,
}

impl Postgres {
    pub async fn load_pending_typed_document_upload_work(
        &self,
        identity: DocumentIdentity,
        quarantine_object_key: &str,
    ) -> Result<Option<TypedDocumentUploadWork>, Error> {
        self.load_typed_document_upload_work(
            identity,
            quarantine_object_key,
            DocumentUploadStatus::PendingUpload,
        )
        .await
    }

    pub async fn load_finalizing_typed_document_upload_work(
        &self,
        identity: DocumentIdentity,
        quarantine_object_key: &str,
    ) -> Result<Option<TypedDocumentUploadWork>, Error> {
        self.load_typed_document_upload_work(
            identity,
            quarantine_object_key,
            DocumentUploadStatus::Finalizing,
        )
        .await
    }

    async fn load_typed_document_upload_work(
        &self,
        identity: DocumentIdentity,
        quarantine_object_key: &str,
        status: DocumentUploadStatus,
    ) -> Result<Option<TypedDocumentUploadWork>, Error> {
        let owner = identity.owner();
        let client = self.get().await?;
        let row = client
            .query_opt(
                r#"
SELECT
    d.workspace_id,
    d.filename,
    d.content_type,
    d.content_length,
    d.object_key,
    d.checksum_sha256,
    d.upload_status
FROM documents d
WHERE d.id = $1
  AND d.owner_type = $2
  AND d.owner_id = $3
  AND d.object_key = $4
  AND d.upload_status = $5
  AND d.archived = false
  AND (
      d.owner_type <> 'policy'
      OR EXISTS (
          SELECT 1
          FROM policies p
          WHERE p.id = d.owner_id
            AND p.workspace_id = d.workspace_id
            AND p.archived_at IS NULL
      )
  )
"#,
                &[
                    &identity.document_uuid(),
                    &owner.owner_type(),
                    &owner.owner_uuid(),
                    &quarantine_object_key,
                    &status.as_str(),
                ],
            )
            .await?;

        row.map(|row| typed_document_upload_work_from_row(row, identity))
            .transpose()
    }

    pub async fn mark_typed_document_uploaded(
        &self,
        work: &TypedDocumentUploadWork,
        final_object_key: &str,
    ) -> Result<bool, Error> {
        self.update_typed_document(
            work,
            DocumentUploadStatus::Finalizing,
            DocumentUploadStatus::Uploaded,
            Some(final_object_key),
        )
        .await
    }

    pub async fn mark_typed_document_contains_virus(
        &self,
        work: &TypedDocumentUploadWork,
    ) -> Result<bool, Error> {
        self.update_typed_document(
            work,
            DocumentUploadStatus::PendingUpload,
            DocumentUploadStatus::ContainsVirus,
            None,
        )
        .await
    }

    pub async fn mark_typed_document_upload_failed(
        &self,
        work: &TypedDocumentUploadWork,
    ) -> Result<bool, Error> {
        self.update_typed_document(
            work,
            DocumentUploadStatus::PendingUpload,
            DocumentUploadStatus::FailedUpload,
            None,
        )
        .await
    }

    async fn update_typed_document(
        &self,
        work: &TypedDocumentUploadWork,
        expected_status: DocumentUploadStatus,
        next_status: DocumentUploadStatus,
        next_object_key: Option<&str>,
    ) -> Result<bool, Error> {
        let owner = work.identity.owner();
        let object_key = next_object_key.unwrap_or(&work.object_key);
        let client = self.get().await?;
        let updated = client
            .execute(
                r#"
UPDATE documents
SET object_key = $6,
    upload_status = $7
WHERE id = $1
  AND owner_type = $2
  AND owner_id = $3
  AND object_key = $4
  AND upload_status = $5
  AND archived = false
"#,
                &[
                    &work.identity.document_uuid(),
                    &owner.owner_type(),
                    &owner.owner_uuid(),
                    &work.object_key,
                    &expected_status.as_str(),
                    &object_key,
                    &next_status.as_str(),
                ],
            )
            .await?;
        Ok(updated > 0)
    }
}

impl TransactionContext<'_> {
    pub async fn request_typed_document_finalization(
        &self,
        work: &TypedDocumentUploadWork,
    ) -> Result<bool, Error> {
        let owner = work.identity.owner();
        let updated = self
            .transaction
            .execute(
                r#"
UPDATE documents
SET upload_status = 'finalizing'
WHERE id = $1
  AND owner_type = $2
  AND owner_id = $3
  AND object_key = $4
  AND upload_status = 'pending'
  AND archived = false
"#,
                &[
                    &work.identity.document_uuid(),
                    &owner.owner_type(),
                    &owner.owner_uuid(),
                    &work.object_key,
                ],
            )
            .await?;
        Ok(updated > 0)
    }
}

fn typed_document_upload_work_from_row(
    row: Row,
    identity: DocumentIdentity,
) -> Result<TypedDocumentUploadWork, Error> {
    Ok(TypedDocumentUploadWork {
        workspace_id: WorkspaceId::from(row.try_get::<_, Uuid>("workspace_id")?),
        identity,
        filename: row.try_get("filename")?,
        content_type: row.try_get("content_type")?,
        content_length: row.try_get("content_length")?,
        object_key: row.try_get("object_key")?,
        checksum_sha256: row.try_get("checksum_sha256")?,
        upload_status: row
            .try_get::<_, String>("upload_status")?
            .parse::<DocumentUploadStatus>()?,
    })
}

pub(crate) fn document_from_row(row: &Row, identity: DocumentIdentity) -> Result<Document, Error> {
    Ok(Document {
        identity,
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
        created_at: row.try_get("created_at")?,
    })
}
