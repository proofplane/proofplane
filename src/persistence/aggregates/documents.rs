use tokio_postgres::Row;
use uuid::Uuid;

use crate::domain::{
    Document, DocumentId, DocumentIdentity, DocumentOwner, DocumentUploadStatus, UserId,
    WorkspaceId,
};

use super::{Error, UnitOfWork, WorkspaceUnitOfWork};

/// Complete-snapshot persistence for the document aggregate. Transactional
/// reads retain a row lock so scan and finalization deliveries race safely.
pub struct DocumentRepository<'a> {
    unit_of_work: &'a UnitOfWork<'a>,
}

pub struct WorkspaceDocumentRepository<'a> {
    workspace: &'a WorkspaceUnitOfWork<'a>,
}

/// Private persistence shape for a complete document snapshot.
struct DocumentRecord {
    created_by_user_id: Uuid,
    filename: String,
    content_type: String,
    content_length: i64,
    object_key: String,
    checksum_sha256: String,
    checksum_crc32c: String,
    archived: bool,
    upload_status: String,
    created_at: chrono::DateTime<chrono::Utc>,
}

impl DocumentRecord {
    fn try_from_row(row: &Row) -> Result<Self, Error> {
        Ok(Self {
            created_by_user_id: row.try_get("created_by_user_id")?,
            filename: row.try_get("filename")?,
            content_type: row.try_get("content_type")?,
            content_length: row.try_get("content_length")?,
            object_key: row.try_get("object_key")?,
            checksum_sha256: row.try_get("checksum_sha256")?,
            checksum_crc32c: row.try_get("checksum_crc32c")?,
            archived: row.try_get("archived")?,
            upload_status: row.try_get("upload_status")?,
            created_at: row.try_get("created_at")?,
        })
    }

    fn into_domain(self, identity: DocumentIdentity) -> Result<Document, Error> {
        Ok(Document {
            identity,
            created_by_user_id: UserId::from(self.created_by_user_id),
            filename: self.filename,
            content_type: self.content_type,
            content_length: self.content_length,
            object_key: self.object_key,
            checksum_sha256: self.checksum_sha256,
            checksum_crc32c: self.checksum_crc32c,
            upload_status: self.upload_status.parse::<DocumentUploadStatus>()?,
            archived: self.archived,
            created_at: self.created_at,
        })
    }
}

impl<'a> WorkspaceUnitOfWork<'a> {
    pub fn documents(&'a self) -> WorkspaceDocumentRepository<'a> {
        WorkspaceDocumentRepository { workspace: self }
    }
}

impl<'a> UnitOfWork<'a> {
    pub fn documents(&'a self) -> DocumentRepository<'a> {
        DocumentRepository { unit_of_work: self }
    }
}

impl DocumentRepository<'_> {
    pub async fn get(
        &self,
        id: DocumentId,
        owner: DocumentOwner,
    ) -> Result<Option<Document>, Error> {
        let identity = document_identity(id, owner);
        self.unit_of_work
            .transaction
            .query_opt(
                r#"SELECT id, workspace_id, owner_type, owner_id, created_by_user_id, filename, content_type, content_length, object_key, checksum_sha256, checksum_crc32c, archived, upload_status, created_at
FROM documents WHERE id = $1 AND owner_type = $2 AND owner_id = $3 FOR UPDATE"#,
                &[&Uuid::from(id), &owner.owner_type(), &owner.owner_uuid()],
            )
            .await?
            .map(|row| DocumentRecord::try_from_row(&row)?.into_domain(identity))
            .transpose()
    }

    /// Saves all mutable document snapshot fields for an already locked row.
    pub async fn save(&self, document: &Document) -> Result<(), Error> {
        let owner = document.owner();
        let updated = self
            .unit_of_work
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
    pub async fn get(
        &self,
        id: DocumentId,
        owner: DocumentOwner,
    ) -> Result<Option<Document>, Error> {
        let identity = document_identity(id, owner);
        self.workspace.transaction.query_opt(
            r#"SELECT id, workspace_id, owner_type, owner_id, created_by_user_id, filename, content_type, content_length, object_key, checksum_sha256, checksum_crc32c, archived, upload_status, created_at
FROM documents WHERE id = $1 AND workspace_id = $2 AND owner_type = $3 AND owner_id = $4 FOR UPDATE"#,
            &[&Uuid::from(id), &Uuid::from(self.workspace.workspace_id), &owner.owner_type(), &owner.owner_uuid()],
        ).await?.map(|row| DocumentRecord::try_from_row(&row)?.into_domain(identity)).transpose()
    }

    pub async fn save(&self, document: &Document) -> Result<(), Error> {
        save_workspace_document_snapshot(
            self.workspace.transaction,
            self.workspace.workspace_id,
            document,
        )
        .await
    }
}

fn document_identity(id: DocumentId, owner: DocumentOwner) -> DocumentIdentity {
    match owner {
        DocumentOwner::EvidenceSubmission(evidence_submission_id) => DocumentIdentity::Evidence {
            evidence_submission_id,
            document_id: id,
        },
        DocumentOwner::Policy(policy_id) => DocumentIdentity::Policy {
            policy_id,
            document_id: id,
        },
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

#[cfg(test)]
mod tests {
    use deadpool_postgres::GenericClient;
    use uuid::Uuid;

    use crate::{
        domain::{CreateDocumentPayload, Document, DocumentIdentity, PolicyId},
        messaging::IntegrationMessage,
        persistence::{test_support, Error, NewOutboxMessage},
        pubsub::{TopicName, MESSAGE_BUS_TOPIC},
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
            .in_unit_of_work(async move |unit_of_work| {
                let workspace = unit_of_work.workspace(workspace.workspace_id);
                workspace.documents().save(&document).await?;
                workspace
                    .append_outbox_message(&NewOutboxMessage::new(
                        TopicName::new(MESSAGE_BUS_TOPIC),
                        IntegrationMessage::scan_document(identity, object_key, None, None),
                    ))
                    .await?;
                Err::<(), _>(Error::InvariantViolation("force transaction rollback"))
            })
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
