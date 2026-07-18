use chrono::{DateTime, Utc};
use tokio_postgres::Row;
use uuid::Uuid;

use crate::{
    domain::{AgentConnectionId, DocumentUploadGrantId, EvidenceSubmissionId, UserId, WorkspaceId},
    repository::WorkspaceTransactionContext,
};

use super::{Error, Postgres};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NewDocumentUploadGrant {
    pub id: DocumentUploadGrantId,
    pub evidence_submission_id: EvidenceSubmissionId,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DocumentUploadGrant {
    pub id: DocumentUploadGrantId,
    pub workspace_id: WorkspaceId,
    pub evidence_submission_id: EvidenceSubmissionId,
    pub issued_by_user_id: UserId,
    pub issued_via_agent_connection_id: Option<AgentConnectionId>,
    pub issued_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub redeemed_at: Option<DateTime<Utc>>,
}

impl WorkspaceTransactionContext<'_> {
    pub async fn create_document_upload_grant(
        &self,
        grant: NewDocumentUploadGrant,
    ) -> Result<Option<DocumentUploadGrant>, Error> {
        let agent_connection_id = self.credential.agent_connection_uuid();
        let rows = self
            .transaction
            .query(
                r#"
WITH scoped_submission AS (
    SELECT s.id
    FROM evidence_submissions s
    JOIN evidence_requests er ON er.id = s.evidence_request_id
    WHERE s.id = $2
      AND er.workspace_id = $3
),
inserted AS (
    INSERT INTO document_upload_grants (
        id,
        workspace_id,
	        evidence_submission_id,
	        issued_by_user_id,
	        issued_via_agent_connection_id,
	        expires_at
	    )
	    SELECT $1, $3, scoped_submission.id, $4, $5, $6
	    FROM scoped_submission
    RETURNING
        id,
        workspace_id,
	        evidence_submission_id,
	        issued_by_user_id,
	        issued_via_agent_connection_id,
	        issued_at,
        expires_at,
        redeemed_at
)
SELECT *
FROM inserted
"#,
                &[
                    &Uuid::from(grant.id),
                    &Uuid::from(grant.evidence_submission_id),
                    &Uuid::from(self.workspace_id),
                    &Uuid::from(self.user_id),
                    &agent_connection_id,
                    &grant.expires_at,
                ],
            )
            .await?;

        rows.into_iter()
            .next()
            .map(|row| document_upload_grant_from_row(&row))
            .transpose()
    }
}

impl Postgres {
    pub async fn redeem_document_upload_grant(
        &self,
        grant_id: DocumentUploadGrantId,
        workspace_id: WorkspaceId,
        evidence_submission_id: EvidenceSubmissionId,
    ) -> Result<Option<DocumentUploadGrant>, Error> {
        let client = self.get().await?;
        let rows = client
            .query(
                r#"
UPDATE document_upload_grants
SET redeemed_at = now()
WHERE id = $1
  AND workspace_id = $2
  AND evidence_submission_id = $3
  AND redeemed_at IS NULL
  AND expires_at > now()
RETURNING
    id,
    workspace_id,
	    evidence_submission_id,
	    issued_by_user_id,
	    issued_via_agent_connection_id,
	    issued_at,
    expires_at,
    redeemed_at
"#,
                &[
                    &Uuid::from(grant_id),
                    &Uuid::from(workspace_id),
                    &Uuid::from(evidence_submission_id),
                ],
            )
            .await?;

        rows.into_iter()
            .next()
            .map(|row| document_upload_grant_from_row(&row))
            .transpose()
    }
}

fn document_upload_grant_from_row(row: &Row) -> Result<DocumentUploadGrant, Error> {
    Ok(DocumentUploadGrant {
        id: DocumentUploadGrantId::from(row.try_get::<_, Uuid>("id")?),
        workspace_id: WorkspaceId::from(row.try_get::<_, Uuid>("workspace_id")?),
        evidence_submission_id: EvidenceSubmissionId::from(
            row.try_get::<_, Uuid>("evidence_submission_id")?,
        ),
        issued_by_user_id: UserId::from(row.try_get::<_, Uuid>("issued_by_user_id")?),
        issued_via_agent_connection_id: row
            .try_get::<_, Option<Uuid>>("issued_via_agent_connection_id")?
            .map(AgentConnectionId::from),
        issued_at: row.try_get("issued_at")?,
        expires_at: row.try_get("expires_at")?,
        redeemed_at: row.try_get("redeemed_at")?,
    })
}
