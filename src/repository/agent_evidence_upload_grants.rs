use chrono::{DateTime, Utc};
use tokio_postgres::Row;
use uuid::Uuid;

use crate::{
    domain::{
        AgentConnectionId, AgentEvidenceUploadDeclaration, AgentEvidenceUploadGrantId,
        CoverageWindow, DocumentId, EvidenceId, EvidenceSubmissionId, Sha256Digest, UserId,
        WorkspaceId,
    },
    repository::WorkspaceTransactionContext,
};

use super::{Error, Postgres};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewAgentEvidenceUploadGrant {
    pub id: AgentEvidenceUploadGrantId,
    pub submission_id: EvidenceSubmissionId,
    pub evidence_id: EvidenceId,
    pub coverage: CoverageWindow,
    pub declaration: AgentEvidenceUploadDeclaration,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentEvidenceUploadGrant {
    pub id: AgentEvidenceUploadGrantId,
    pub submission_id: EvidenceSubmissionId,
    pub workspace_id: WorkspaceId,
    pub evidence_id: EvidenceId,
    pub coverage: CoverageWindow,
    pub declaration: AgentEvidenceUploadDeclaration,
    pub issued_by_user_id: UserId,
    pub issued_via_agent_connection_id: AgentConnectionId,
    pub issued_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub document_id: Option<DocumentId>,
}

impl WorkspaceTransactionContext<'_> {
    pub async fn create_agent_evidence_upload_grant(
        &self,
        grant: NewAgentEvidenceUploadGrant,
    ) -> Result<Option<AgentEvidenceUploadGrant>, Error> {
        let agent_connection_id =
            self.credential
                .agent_connection_uuid()
                .ok_or(Error::InvariantViolation(
                    "machine upload grant requires an agent connection",
                ))?;
        let expected_content_length = i64::try_from(grant.declaration.expected_content_length)
            .map_err(|_| {
                Error::InvariantViolation("machine upload length exceeds Postgres BIGINT")
            })?;
        let expected_sha256 = grant
            .declaration
            .expected_sha256
            .map(|digest| digest.as_bytes().to_vec());
        let rows = self
            .transaction
            .query(
                r#"
WITH eligible AS (
    SELECT e.id AS evidence_id
    FROM evidence e
    JOIN agent_connections c
     ON c.id = $11
     AND c.workspace_id = $3
     AND c.user_id = $10
    WHERE e.id = $4
      AND e.workspace_id = $3
),
inserted AS (
    INSERT INTO agent_evidence_upload_grants (
        id,
        submission_id,
        workspace_id,
        evidence_id,
        valid_from,
        valid_until,
        filename,
        content_type,
        expected_content_length,
        expected_sha256,
        issued_by_user_id,
        issued_via_agent_connection_id,
        expires_at
    )
    SELECT $1, $2, $3, eligible.evidence_id, $5, $6, $7, $8, $9, $12, $10, $11, $13
    FROM eligible
    RETURNING *
)
SELECT * FROM inserted
"#,
                &[
                    &Uuid::from(grant.id),
                    &Uuid::from(grant.submission_id),
                    &Uuid::from(self.workspace_id),
                    &Uuid::from(grant.evidence_id),
                    &grant.coverage.valid_from,
                    &grant.coverage.valid_until,
                    &grant.declaration.filename,
                    &grant.declaration.content_type,
                    &expected_content_length,
                    &Uuid::from(self.user_id),
                    &agent_connection_id,
                    &expected_sha256,
                    &grant.expires_at,
                ],
            )
            .await?;

        rows.into_iter()
            .next()
            .map(|row| agent_evidence_upload_grant_from_row(&row))
            .transpose()
    }
}

impl Postgres {
    pub async fn get_unexpired_agent_evidence_upload_grant(
        &self,
        grant_id: AgentEvidenceUploadGrantId,
        workspace_id: WorkspaceId,
    ) -> Result<Option<AgentEvidenceUploadGrant>, Error> {
        let client = self.get().await?;
        let rows = client
            .query(
                r#"
SELECT *
FROM agent_evidence_upload_grants
WHERE id = $1
  AND workspace_id = $2
  AND expires_at > now()
"#,
                &[&Uuid::from(grant_id), &Uuid::from(workspace_id)],
            )
            .await?;

        rows.into_iter()
            .next()
            .map(|row| agent_evidence_upload_grant_from_row(&row))
            .transpose()
    }
}

fn agent_evidence_upload_grant_from_row(row: &Row) -> Result<AgentEvidenceUploadGrant, Error> {
    let expected_content_length = row.try_get::<_, i64>("expected_content_length")?;
    let expected_content_length = u64::try_from(expected_content_length)
        .map_err(|_| Error::InvariantViolation("persisted machine upload length is negative"))?;
    let expected_sha256 = row
        .try_get::<_, Option<Vec<u8>>>("expected_sha256")?
        .map(|bytes| {
            bytes.try_into().map(Sha256Digest::from_bytes).map_err(|_| {
                Error::InvariantViolation("persisted machine upload SHA-256 is invalid")
            })
        })
        .transpose()?;

    Ok(AgentEvidenceUploadGrant {
        id: AgentEvidenceUploadGrantId::from(row.try_get::<_, Uuid>("id")?),
        submission_id: EvidenceSubmissionId::from(row.try_get::<_, Uuid>("submission_id")?),
        workspace_id: WorkspaceId::from(row.try_get::<_, Uuid>("workspace_id")?),
        evidence_id: EvidenceId::from(row.try_get::<_, Uuid>("evidence_id")?),
        coverage: CoverageWindow::new(row.try_get("valid_from")?, row.try_get("valid_until")?)?,
        declaration: AgentEvidenceUploadDeclaration {
            filename: row.try_get("filename")?,
            content_type: row.try_get("content_type")?,
            expected_content_length,
            expected_sha256,
        },
        issued_by_user_id: UserId::from(row.try_get::<_, Uuid>("issued_by_user_id")?),
        issued_via_agent_connection_id: AgentConnectionId::from(
            row.try_get::<_, Uuid>("issued_via_agent_connection_id")?,
        ),
        issued_at: row.try_get("issued_at")?,
        expires_at: row.try_get("expires_at")?,
        completed_at: row.try_get("completed_at")?,
        document_id: row
            .try_get::<_, Option<Uuid>>("document_id")?
            .map(DocumentId::from),
    })
}
