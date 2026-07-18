use chrono::{DateTime, Utc};
use tokio_postgres::Row;
use uuid::Uuid;

use crate::{
    domain::{AgentConnectionId, PolicyDocumentUploadGrantId, PolicyId, UserId, WorkspaceId},
    repository::WorkspaceTransactionContext,
};

use super::{Error, Postgres};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NewPolicyDocumentUploadGrant {
    pub id: PolicyDocumentUploadGrantId,
    pub policy_id: PolicyId,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PolicyDocumentUploadGrant {
    pub id: PolicyDocumentUploadGrantId,
    pub workspace_id: WorkspaceId,
    pub policy_id: PolicyId,
    pub issued_by_user_id: UserId,
    pub issued_via_agent_connection_id: AgentConnectionId,
    pub issued_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub redeemed_at: Option<DateTime<Utc>>,
}

impl WorkspaceTransactionContext<'_> {
    pub async fn create_policy_document_upload_grant(
        &self,
        grant: NewPolicyDocumentUploadGrant,
    ) -> Result<Option<PolicyDocumentUploadGrant>, Error> {
        let agent_connection_id = self.credential.agent_connection_uuid();
        let rows = self
            .transaction
            .query(
                r#"
WITH scoped_policy AS (
    SELECT id
    FROM policies
    WHERE id = $2
      AND workspace_id = $3
      AND archived_at IS NULL
),
inserted AS (
    INSERT INTO policy_document_upload_grants (
        id,
        workspace_id,
        policy_id,
        issued_by_user_id,
        issued_via_agent_connection_id,
        expires_at
    )
    SELECT $1, $3, scoped_policy.id, $4, $5, $6
    FROM scoped_policy
    RETURNING
        id,
        workspace_id,
        policy_id,
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
                    &Uuid::from(grant.policy_id),
                    &Uuid::from(self.workspace_id),
                    &Uuid::from(self.user_id),
                    &agent_connection_id,
                    &grant.expires_at,
                ],
            )
            .await?;

        rows.into_iter()
            .next()
            .map(|row| policy_document_upload_grant_from_row(&row))
            .transpose()
    }
}

impl Postgres {
    pub async fn redeem_policy_document_upload_grant(
        &self,
        grant_id: PolicyDocumentUploadGrantId,
        workspace_id: WorkspaceId,
        policy_id: PolicyId,
    ) -> Result<Option<PolicyDocumentUploadGrant>, Error> {
        let client = self.get().await?;
        let rows = client
            .query(
                r#"
UPDATE policy_document_upload_grants AS g
SET redeemed_at = now()
FROM policies AS policy
WHERE g.id = $1
  AND g.workspace_id = $2
  AND g.policy_id = $3
  AND g.redeemed_at IS NULL
  AND g.expires_at > now()
  AND policy.id = g.policy_id
  AND policy.workspace_id = g.workspace_id
  AND policy.archived_at IS NULL
RETURNING
    g.id,
    g.workspace_id,
    g.policy_id,
    g.issued_by_user_id,
    g.issued_via_agent_connection_id,
    g.issued_at,
    g.expires_at,
    g.redeemed_at
"#,
                &[
                    &Uuid::from(grant_id),
                    &Uuid::from(workspace_id),
                    &Uuid::from(policy_id),
                ],
            )
            .await?;

        rows.into_iter()
            .next()
            .map(|row| policy_document_upload_grant_from_row(&row))
            .transpose()
    }
}

fn policy_document_upload_grant_from_row(row: &Row) -> Result<PolicyDocumentUploadGrant, Error> {
    Ok(PolicyDocumentUploadGrant {
        id: PolicyDocumentUploadGrantId::from(row.try_get::<_, Uuid>("id")?),
        workspace_id: WorkspaceId::from(row.try_get::<_, Uuid>("workspace_id")?),
        policy_id: PolicyId::from(row.try_get::<_, Uuid>("policy_id")?),
        issued_by_user_id: UserId::from(row.try_get::<_, Uuid>("issued_by_user_id")?),
        issued_via_agent_connection_id: AgentConnectionId::from(
            row.try_get::<_, Uuid>("issued_via_agent_connection_id")?,
        ),
        issued_at: row.try_get("issued_at")?,
        expires_at: row.try_get("expires_at")?,
        redeemed_at: row.try_get("redeemed_at")?,
    })
}
