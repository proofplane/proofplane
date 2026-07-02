use tokio_postgres::Row;
use uuid::Uuid;

use crate::{
    authentication::opaque_token::AuditorInviteSecretDigest,
    domain::{
        AgentConnectionId, AuditorAccessGrant, AuditorAccessGrantId,
        CreateAuditorAccessGrantPayload, WorkspaceId,
    },
    repository::WorkspaceTransactionContext,
};

use super::{Error, Postgres};

const AUDITOR_ACCESS_GRANT_COLUMNS: &str = "id, workspace_id, auditor_email, created_by_user_id, created_via_agent_connection_id, created_at, expires_at, revoked_at";

impl WorkspaceTransactionContext<'_> {
    pub async fn create_auditor_access_grant(
        &self,
        grant: CreateAuditorAccessGrantPayload,
    ) -> Result<AuditorAccessGrant, Error> {
        let digest: &[u8] = grant.secret_digest.as_bytes();
        let agent_connection_id = self.credential.agent_connection_uuid();
        let row = self
            .transaction
            .query_one(
                &format!(
                    r#"
INSERT INTO auditor_access_grants (
    id,
    workspace_id,
    auditor_email,
    secret_digest,
    created_by_user_id,
    created_via_agent_connection_id,
    expires_at
)
VALUES ($1, $2, $3, $4, $5, $6, $7)
RETURNING {AUDITOR_ACCESS_GRANT_COLUMNS}
"#
                ),
                &[
                    &Uuid::from(grant.id),
                    &Uuid::from(self.workspace_id),
                    &grant.auditor_email,
                    &digest,
                    &Uuid::from(self.user_id),
                    &agent_connection_id,
                    &grant.expires_at,
                ],
            )
            .await?;

        auditor_access_grant_from_row(&row)
    }
}

impl Postgres {
    pub async fn list_auditor_access_grants(
        &self,
        workspace_id: WorkspaceId,
    ) -> Result<Vec<AuditorAccessGrant>, Error> {
        let client = self.get().await?;
        let rows = client
            .query(
                &format!(
                    r#"
SELECT {AUDITOR_ACCESS_GRANT_COLUMNS}
FROM auditor_access_grants
WHERE workspace_id = $1
ORDER BY created_at DESC, id DESC
"#
                ),
                &[&Uuid::from(workspace_id)],
            )
            .await?;

        rows.iter().map(auditor_access_grant_from_row).collect()
    }

    pub async fn revoke_auditor_access_grant(
        &self,
        workspace_id: WorkspaceId,
        grant_id: AuditorAccessGrantId,
    ) -> Result<Option<AuditorAccessGrant>, Error> {
        let client = self.get().await?;
        let rows = client
            .query(
                &format!(
                    r#"
UPDATE auditor_access_grants
SET revoked_at = COALESCE(revoked_at, now())
WHERE workspace_id = $1 AND id = $2
RETURNING {AUDITOR_ACCESS_GRANT_COLUMNS}
"#
                ),
                &[&Uuid::from(workspace_id), &Uuid::from(grant_id)],
            )
            .await?;

        rows.into_iter()
            .next()
            .map(|row| auditor_access_grant_from_row(&row))
            .transpose()
    }

    pub async fn get_active_auditor_access_grant_by_digest(
        &self,
        workspace_id: WorkspaceId,
        digest: AuditorInviteSecretDigest,
    ) -> Result<Option<AuditorAccessGrant>, Error> {
        let client = self.get().await?;
        let digest: &[u8] = digest.as_bytes();
        let rows = client
            .query(
                &format!(
                    r#"
SELECT {AUDITOR_ACCESS_GRANT_COLUMNS}
FROM auditor_access_grants
WHERE workspace_id = $1
  AND secret_digest = $2
  AND revoked_at IS NULL
  AND expires_at > now()
"#
                ),
                &[&Uuid::from(workspace_id), &digest],
            )
            .await?;

        rows.into_iter()
            .next()
            .map(|row| auditor_access_grant_from_row(&row))
            .transpose()
    }
}

fn auditor_access_grant_from_row(row: &Row) -> Result<AuditorAccessGrant, Error> {
    Ok(AuditorAccessGrant {
        id: AuditorAccessGrantId::from(row.try_get::<_, Uuid>("id")?),
        workspace_id: WorkspaceId::from(row.try_get::<_, Uuid>("workspace_id")?),
        auditor_email: row.try_get("auditor_email")?,
        created_by_user_id: row.try_get::<_, Uuid>("created_by_user_id")?.into(),
        created_via_agent_connection_id: row
            .try_get::<_, Uuid>("created_via_agent_connection_id")
            .map(AgentConnectionId::from)?,
        created_at: row.try_get("created_at")?,
        expires_at: row.try_get("expires_at")?,
        revoked_at: row.try_get("revoked_at")?,
    })
}
