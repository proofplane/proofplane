use tokio_postgres::Row;
use uuid::Uuid;

use crate::domain::{Document, DocumentIdentity, DocumentUploadStatus, UserId, WorkspaceId};

use super::{Error, Postgres, TransactionContext, WorkspaceTransactionContext};

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
}

/// Complete-snapshot persistence for the document aggregate. Transactional
/// reads retain a row lock so scan and finalization deliveries race safely.
pub struct DocumentRepository<'a> {
    context: &'a TransactionContext<'a>,
}

pub struct WorkspaceDocumentRepository<'a> {
    context: &'a WorkspaceTransactionContext<'a>,
}

impl<'a> WorkspaceTransactionContext<'a> {
    pub fn documents(&'a self) -> WorkspaceDocumentRepository<'a> {
        WorkspaceDocumentRepository { context: self }
    }
}

impl<'a> TransactionContext<'a> {
    pub fn documents(&'a self) -> DocumentRepository<'a> {
        DocumentRepository { context: self }
    }
}

impl DocumentRepository<'_> {
    pub async fn get(&self, identity: DocumentIdentity) -> Result<Option<Document>, Error> {
        let owner = identity.owner();
        self.context
            .transaction
            .query_opt(
                r#"SELECT id, workspace_id, owner_type, owner_id, created_by_user_id, filename, content_type, content_length, object_key, checksum_sha256, checksum_crc32c, archived, upload_status, created_at
FROM documents WHERE id = $1 AND owner_type = $2 AND owner_id = $3 FOR UPDATE"#,
                &[&identity.document_uuid(), &owner.owner_type(), &owner.owner_uuid()],
            )
            .await?
            .map(|row| document_from_row(&row, identity))
            .transpose()
    }

    /// Saves all mutable document snapshot fields for an already locked row.
    pub async fn save(&self, document: &Document) -> Result<(), Error> {
        let owner = document.owner();
        let updated = self
            .context
            .transaction
            .execute(
                r#"UPDATE documents SET filename = $4, content_type = $5, content_length = $6,
object_key = $7, checksum_sha256 = $8, checksum_crc32c = $9, archived = $10,
upload_status = $11
WHERE id = $1 AND owner_type = $2 AND owner_id = $3"#,
                &[
                    &Uuid::from(document.id()),
                    &owner.owner_type(),
                    &owner.owner_uuid(),
                    &document.filename,
                    &document.content_type,
                    &document.content_length,
                    &document.object_key,
                    &document.checksum_sha256,
                    &document.checksum_crc32c,
                    &document.archived,
                    &document.upload_status.as_str(),
                ],
            )
            .await?;
        if updated != 1 {
            return Err(Error::InvariantViolation(
                "document snapshot disappeared while locked",
            ));
        }
        Ok(())
    }
}

impl WorkspaceDocumentRepository<'_> {
    pub async fn get(&self, identity: DocumentIdentity) -> Result<Option<Document>, Error> {
        let owner = identity.owner();
        self.context.transaction.query_opt(
            r#"SELECT id, workspace_id, owner_type, owner_id, created_by_user_id, filename, content_type, content_length, object_key, checksum_sha256, checksum_crc32c, archived, upload_status, created_at
FROM documents WHERE id = $1 AND workspace_id = $2 AND owner_type = $3 AND owner_id = $4 FOR UPDATE"#,
            &[&identity.document_uuid(), &Uuid::from(self.context.workspace_id), &owner.owner_type(), &owner.owner_uuid()],
        ).await?.map(|row| document_from_row(&row, identity)).transpose()
    }

    pub async fn save(&self, document: &Document) -> Result<(), Error> {
        save_workspace_document_snapshot(
            &self.context.transaction,
            self.context.workspace_id,
            document,
        )
        .await
    }
}

async fn save_workspace_document_snapshot(
    transaction: &deadpool_postgres::Transaction<'_>,
    workspace_id: WorkspaceId,
    document: &Document,
) -> Result<(), Error> {
    let owner = document.owner();
    let saved = transaction.execute(
        r#"INSERT INTO documents (id, workspace_id, owner_type, owner_id, created_by_user_id, filename, content_type, content_length, object_key, checksum_sha256, checksum_crc32c, archived, upload_status, created_at)
VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)
ON CONFLICT (id) DO UPDATE SET owner_type = EXCLUDED.owner_type, owner_id = EXCLUDED.owner_id,
created_by_user_id = EXCLUDED.created_by_user_id, filename = EXCLUDED.filename,
content_type = EXCLUDED.content_type, content_length = EXCLUDED.content_length,
object_key = EXCLUDED.object_key, checksum_sha256 = EXCLUDED.checksum_sha256,
checksum_crc32c = EXCLUDED.checksum_crc32c, archived = EXCLUDED.archived,
upload_status = EXCLUDED.upload_status, created_at = EXCLUDED.created_at
WHERE documents.workspace_id = EXCLUDED.workspace_id"#,
        &[&Uuid::from(document.id()), &Uuid::from(workspace_id), &owner.owner_type(), &owner.owner_uuid(),
          &Uuid::from(document.created_by_user_id), &document.filename, &document.content_type,
          &document.content_length, &document.object_key, &document.checksum_sha256,
          &document.checksum_crc32c, &document.archived, &document.upload_status.as_str(), &document.created_at],
    ).await?;
    if saved != 1 {
        return Err(Error::InvariantViolation(
            "document snapshot save must affect one row",
        ));
    }
    Ok(())
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
        archived: row.try_get("archived").unwrap_or(false),
        created_at: row.try_get("created_at")?,
    })
}

#[cfg(test)]
mod tests {
    use deadpool_postgres::GenericClient;
    use uuid::Uuid;

    use crate::{
        domain::{CreateDocumentPayload, Document, DocumentIdentity, PolicyId},
        messaging::IntegrationMessage,
        pubsub::{TopicName, MESSAGE_BUS_TOPIC},
        repository::{test_support, Error, NewOutboxMessage},
    };

    #[test]
    fn transactional_document_reads_lock_the_complete_snapshot() {
        let source = include_str!("documents.rs");
        assert!(source.contains("checksum_crc32c, archived, upload_status, created_at"));
        assert!(source.contains("owner_id = $3 FOR UPDATE"));
    }

    #[test]
    fn snapshot_updates_include_archive_and_upload_lifecycle() {
        let source = include_str!("documents.rs");
        assert!(source.contains("archived = $10"));
        assert!(source.contains("upload_status = $11"));
    }

    #[tokio::test]
    async fn document_snapshot_and_scan_message_rollback_together() {
        let postgres = test_support::database().await;
        let workspace = test_support::workspace(&postgres, "Document owner").await;
        let identity = DocumentIdentity::Policy {
            policy_id: PolicyId::from(Uuid::new_v4()),
            document_id: Uuid::new_v4().into(),
        };
        let (document, _) = Document::create(
            identity,
            workspace.user_id,
            CreateDocumentPayload {
                owner: identity.owner(),
                filename: "rollback.pdf".to_owned(),
                content_type: "application/pdf".to_owned(),
                content_length: 1,
                object_key: format!("quarantine/{}", Uuid::new_v4()),
                checksum_sha256: "sha".to_owned(),
                checksum_crc32c: "crc".to_owned(),
            },
            chrono::Utc::now(),
        )
        .expect("test document is valid");
        let object_key = document.object_key.clone();
        let result = postgres
            .in_agent_connection_workspace_context(
                workspace.workspace_id,
                workspace.user_id,
                workspace.agent_connection_id,
                async move |context| {
                    context.documents().save(&document).await?;
                    context
                        .append_outbox_message(&NewOutboxMessage::new(
                            TopicName::new(MESSAGE_BUS_TOPIC),
                            IntegrationMessage::scan_document(identity, object_key, None, None),
                        ))
                        .await?;
                    Err::<(), _>(Error::InvariantViolation("force transaction rollback"))
                },
            )
            .await;
        assert!(result.is_err());

        let client = postgres.get().await.expect("test database is available");
        let document_count: i64 = client
            .query_one(
                "SELECT count(*) FROM documents WHERE id = $1",
                &[&Uuid::from(identity.document_id())],
            )
            .await
            .expect("document count query succeeds")
            .get(0);
        let outbox_count: i64 = client
            .query_one(
                "SELECT count(*) FROM outbox_messages WHERE subject = $1",
                &[&identity.document_uuid().to_string()],
            )
            .await
            .expect("outbox count query succeeds")
            .get(0);
        assert_eq!(document_count, 0);
        assert_eq!(outbox_count, 0);
    }
}
