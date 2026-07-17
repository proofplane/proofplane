use chrono::{DateTime, Utc};
use tokio_postgres::Row;
use uuid::Uuid;

use crate::{
    domain::{
        AgentConnectionId, CoverageWindow, EvidenceId, EvidenceUploadGrantId, UserId, WorkspaceId,
    },
    repository::WorkspaceTransactionContext,
};

use super::{Error, Postgres};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NewEvidenceUploadGrant {
    pub id: EvidenceUploadGrantId,
    pub evidence_id: EvidenceId,
    pub coverage: CoverageWindow,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EvidenceUploadGrant {
    pub id: EvidenceUploadGrantId,
    pub workspace_id: WorkspaceId,
    pub evidence_id: EvidenceId,
    pub coverage: CoverageWindow,
    pub issued_by_user_id: UserId,
    pub issued_via_agent_connection_id: Option<AgentConnectionId>,
    pub issued_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub redeemed_at: Option<DateTime<Utc>>,
}

impl WorkspaceTransactionContext<'_> {
    pub async fn create_evidence_upload_grant(
        &self,
        grant: NewEvidenceUploadGrant,
    ) -> Result<Option<EvidenceUploadGrant>, Error> {
        let agent_connection_id = self.credential.agent_connection_uuid();
        let rows = self
            .transaction
            .query(
                r#"
WITH scoped_evidence AS (
    SELECT e.id
    FROM evidence e
    WHERE e.id = $2
      AND e.workspace_id = $3
),
inserted AS (
    INSERT INTO evidence_upload_grants (
        id,
        workspace_id,
        evidence_id,
        valid_from,
        valid_until,
        issued_by_user_id,
        issued_via_agent_connection_id,
        expires_at
    )
    SELECT $1, $3, scoped_evidence.id, $4, $5, $6, $7, $8
    FROM scoped_evidence
    RETURNING
        id,
        workspace_id,
        evidence_id,
        valid_from,
        valid_until,
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
                    &Uuid::from(grant.evidence_id),
                    &Uuid::from(self.workspace_id),
                    &grant.coverage.valid_from,
                    &grant.coverage.valid_until,
                    &Uuid::from(self.user_id),
                    &agent_connection_id,
                    &grant.expires_at,
                ],
            )
            .await?;

        rows.into_iter()
            .next()
            .map(|row| evidence_upload_grant_from_row(&row))
            .transpose()
    }
}

impl Postgres {
    pub async fn redeem_evidence_upload_grant(
        &self,
        grant_id: EvidenceUploadGrantId,
        workspace_id: WorkspaceId,
        evidence_id: EvidenceId,
    ) -> Result<Option<EvidenceUploadGrant>, Error> {
        let client = self.get().await?;
        let rows = client
            .query(
                r#"
UPDATE evidence_upload_grants
SET redeemed_at = now()
WHERE id = $1
  AND workspace_id = $2
  AND evidence_id = $3
  AND redeemed_at IS NULL
  AND expires_at > now()
RETURNING
    id,
    workspace_id,
    evidence_id,
    valid_from,
    valid_until,
    issued_by_user_id,
    issued_via_agent_connection_id,
    issued_at,
    expires_at,
    redeemed_at
"#,
                &[
                    &Uuid::from(grant_id),
                    &Uuid::from(workspace_id),
                    &Uuid::from(evidence_id),
                ],
            )
            .await?;

        rows.into_iter()
            .next()
            .map(|row| evidence_upload_grant_from_row(&row))
            .transpose()
    }
}

fn evidence_upload_grant_from_row(row: &Row) -> Result<EvidenceUploadGrant, Error> {
    let coverage = CoverageWindow::new(row.try_get("valid_from")?, row.try_get("valid_until")?)?;

    Ok(EvidenceUploadGrant {
        id: EvidenceUploadGrantId::from(row.try_get::<_, Uuid>("id")?),
        workspace_id: WorkspaceId::from(row.try_get::<_, Uuid>("workspace_id")?),
        evidence_id: EvidenceId::from(row.try_get::<_, Uuid>("evidence_id")?),
        coverage,
        issued_by_user_id: UserId::from(row.try_get::<_, Uuid>("issued_by_user_id")?),
        issued_via_agent_connection_id: row
            .try_get::<_, Option<Uuid>>("issued_via_agent_connection_id")?
            .map(AgentConnectionId::from),
        issued_at: row.try_get("issued_at")?,
        expires_at: row.try_get("expires_at")?,
        redeemed_at: row.try_get("redeemed_at")?,
    })
}
