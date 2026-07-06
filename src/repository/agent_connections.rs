use deadpool_postgres::GenericClient;
use tokio_postgres::Row;
use uuid::Uuid;

use crate::domain::{
    AgentConnection, AgentConnectionId, CreatePendingAgentConnection, SecretDigest,
    WorkspacePermission,
};

use super::{constraints::classify_db_error, Error, Postgres};

const CONNECTION_COLUMNS: &str = "c.id, c.user_id, c.workspace_id, c.auth0_subject, \
    c.auth0_client_id, c.client_display_name, c.resource, c.status, \
    c.pending_expires_at, c.activated_at, c.last_used_at, c.revoked_at, c.created_at";

impl Postgres {
    pub async fn create_pending_agent_connection(
        &self,
        pending: &CreatePendingAgentConnection,
    ) -> Result<AgentConnection, Error> {
        let mut client = self.get().await?;
        let transaction = client.transaction().await?;

        transaction
            .execute(
                r#"
DELETE FROM agent_connections
WHERE user_id = $1
  AND auth0_client_id = $2
  AND resource = $3
  AND status = 'pending'
  AND pending_expires_at <= now()
"#,
                &[
                    &Uuid::from(pending.user_id),
                    &pending.auth0_client_id,
                    &pending.resource,
                ],
            )
            .await?;

        let row = transaction
            .query_one(
                r#"
INSERT INTO agent_connections (
    id, user_id, workspace_id, auth0_subject, auth0_client_id,
    client_display_name, resource, status, pending_expires_at
)
VALUES ($1, $2, $3, $4, $5, $6, $7, 'pending', $8)
RETURNING id, user_id, workspace_id, auth0_subject, auth0_client_id,
    client_display_name, resource, status, pending_expires_at, activated_at,
    last_used_at, revoked_at, created_at
"#,
                &[
                    &Uuid::from(pending.id),
                    &Uuid::from(pending.user_id),
                    &Uuid::from(pending.workspace_id),
                    &pending.auth0_subject,
                    &pending.auth0_client_id,
                    &pending.client_display_name,
                    &pending.resource,
                    &pending.pending_expires_at,
                ],
            )
            .await
            .map_err(classify_db_error)?;

        for permission in &pending.permissions {
            transaction
                .execute(
                    r#"
INSERT INTO agent_connection_permissions (agent_connection_id, permission)
VALUES ($1, $2)
"#,
                    &[&Uuid::from(pending.id), &permission.as_str()],
                )
                .await
                .map_err(classify_db_error)?;
        }

        let continuation_digest: &[u8] = pending.continuation_digest.as_bytes();
        let nonce_digest: &[u8] = pending.nonce_digest.as_bytes();
        transaction
            .execute(
                r#"
INSERT INTO agent_authorization_transactions (
    id, agent_connection_id, continuation_digest, nonce_digest
)
VALUES ($1, $2, $3, $4)
"#,
                &[
                    &Uuid::from(pending.transaction_id),
                    &Uuid::from(pending.id),
                    &continuation_digest,
                    &nonce_digest,
                ],
            )
            .await
            .map_err(classify_db_error)?;

        transaction.commit().await?;
        let mut connection = connection_from_row(&row)?;
        connection.permissions = pending.permissions.clone();
        Ok(connection)
    }

    pub async fn deny_pending_agent_connection(
        &self,
        continuation_digest: SecretDigest,
    ) -> Result<bool, Error> {
        let client = self.get().await?;
        let digest: &[u8] = continuation_digest.as_bytes();
        let deleted = client
            .execute(
                r#"
DELETE FROM agent_connections c
USING agent_authorization_transactions t
WHERE t.agent_connection_id = c.id
  AND t.continuation_digest = $1
  AND t.consumed_at IS NULL
  AND c.status = 'pending'
"#,
                &[&digest],
            )
            .await?;
        Ok(deleted == 1)
    }

    pub async fn consume_agent_connection_continuation(
        &self,
        continuation_digest: SecretDigest,
        nonce_digest: SecretDigest,
    ) -> Result<Option<AgentConnection>, Error> {
        let mut client = self.get().await?;
        let transaction = client.transaction().await?;
        let continuation: &[u8] = continuation_digest.as_bytes();
        let nonce: &[u8] = nonce_digest.as_bytes();
        let row = transaction
            .query_opt(
                &format!(
                    r#"
WITH consumed AS (
    UPDATE agent_authorization_transactions t
    SET consumed_at = now()
    FROM agent_connections candidate
    WHERE t.agent_connection_id = candidate.id
      AND t.continuation_digest = $1
      AND t.nonce_digest = $2
      AND t.consumed_at IS NULL
      AND candidate.status = 'pending'
      AND candidate.pending_expires_at > now()
      AND EXISTS (
          SELECT 1 FROM users u
          WHERE u.id = candidate.user_id
            AND u.auth0_sub = candidate.auth0_subject
      )
      AND EXISTS (
          SELECT 1 FROM workspace_memberships m
          WHERE m.user_id = candidate.user_id
            AND m.workspace_id = candidate.workspace_id
      )
    RETURNING t.agent_connection_id
)
SELECT {CONNECTION_COLUMNS}
FROM agent_connections c
JOIN consumed ON consumed.agent_connection_id = c.id
"#
                ),
                &[&continuation, &nonce],
            )
            .await?;

        let Some(row) = row else {
            transaction.commit().await?;
            return Ok(None);
        };
        let mut connection = connection_from_row(&row)?;
        connection.permissions = permissions_for_connection(&transaction, connection.id).await?;
        transaction.commit().await?;
        Ok(Some(connection))
    }

    pub async fn find_reusable_agent_connection(
        &self,
        auth0_subject: &str,
        auth0_client_id: &str,
        resource: &str,
    ) -> Result<Option<AgentConnection>, Error> {
        let client = self.get().await?;
        let row = client
            .query_opt(
                &format!(
                    r#"
SELECT {CONNECTION_COLUMNS}
FROM agent_connections c
JOIN users u ON u.id = c.user_id
JOIN workspace_memberships m
  ON m.user_id = c.user_id AND m.workspace_id = c.workspace_id
WHERE c.auth0_subject = $1
  AND u.auth0_sub = $1
  AND c.auth0_client_id = $2
  AND c.resource = $3
  AND c.status = 'active'
"#
                ),
                &[&auth0_subject, &auth0_client_id, &resource],
            )
            .await?;
        let Some(row) = row else {
            return Ok(None);
        };
        let mut connection = connection_from_row(&row)?;
        connection.permissions = permissions_for_connection(&client, connection.id).await?;
        Ok(Some(connection))
    }

    pub async fn activate_agent_connection(
        &self,
        id: AgentConnectionId,
    ) -> Result<Option<AgentConnection>, Error> {
        let mut client = self.get().await?;
        let transaction = client.transaction().await?;
        let row = transaction
            .query_opt(
                r#"
UPDATE agent_connections c
SET status = 'active', activated_at = now()
WHERE c.id = $1
  AND c.status = 'pending'
  AND c.pending_expires_at > now()
  AND EXISTS (
      SELECT 1 FROM agent_authorization_transactions t
      WHERE t.agent_connection_id = c.id AND t.consumed_at IS NOT NULL
  )
  AND EXISTS (
      SELECT 1 FROM workspace_memberships m
      WHERE m.user_id = c.user_id AND m.workspace_id = c.workspace_id
  )
RETURNING c.id, c.user_id, c.workspace_id, c.auth0_subject, c.auth0_client_id,
    c.client_display_name, c.resource, c.status, c.pending_expires_at,
    c.activated_at, c.last_used_at, c.revoked_at, c.created_at
"#,
                &[&Uuid::from(id)],
            )
            .await?;
        let Some(row) = row else {
            transaction.commit().await?;
            return Ok(None);
        };
        let mut connection = connection_from_row(&row)?;
        connection.permissions = permissions_for_connection(&transaction, id).await?;
        transaction.commit().await?;
        Ok(Some(connection))
    }

    pub async fn touch_agent_connection_last_used_at(
        &self,
        id: AgentConnectionId,
    ) -> Result<bool, Error> {
        let client = self.get().await?;
        Ok(client
            .execute(
                r#"
UPDATE agent_connections
SET last_used_at = now()
WHERE id = $1 AND status = 'active'
"#,
                &[&Uuid::from(id)],
            )
            .await?
            == 1)
    }

    pub async fn revoke_agent_connection(&self, id: AgentConnectionId) -> Result<bool, Error> {
        let client = self.get().await?;
        Ok(client
            .execute(
                r#"
UPDATE agent_connections
SET status = 'revoked', revoked_at = now()
WHERE id = $1 AND status IN ('pending', 'active')
"#,
                &[&Uuid::from(id)],
            )
            .await?
            == 1)
    }
}

async fn permissions_for_connection(
    client: &impl GenericClient,
    id: AgentConnectionId,
) -> Result<Vec<WorkspacePermission>, Error> {
    let rows = client
        .query(
            r#"
SELECT permission
FROM agent_connection_permissions
WHERE agent_connection_id = $1
"#,
            &[&Uuid::from(id)],
        )
        .await?;
    let mut permissions = rows
        .into_iter()
        .map(|row| {
            row.try_get::<_, String>("permission")?
                .parse()
                .map_err(|_| Error::InvariantViolation("unknown agent connection permission"))
        })
        .collect::<Result<Vec<_>, Error>>()?;
    permissions.sort_by_key(|permission| {
        WorkspacePermission::ALL
            .iter()
            .position(|candidate| candidate == permission)
            .expect("parsed permission is canonical")
    });
    Ok(permissions)
}

fn connection_from_row(row: &Row) -> Result<AgentConnection, Error> {
    Ok(AgentConnection {
        id: AgentConnectionId::from(row.try_get::<_, Uuid>("id")?),
        user_id: row.try_get::<_, Uuid>("user_id")?.into(),
        workspace_id: row.try_get::<_, Uuid>("workspace_id")?.into(),
        auth0_subject: row.try_get("auth0_subject")?,
        auth0_client_id: row.try_get("auth0_client_id")?,
        client_display_name: row.try_get("client_display_name")?,
        resource: row.try_get("resource")?,
        status: row
            .try_get::<_, String>("status")?
            .parse()
            .map_err(|_| Error::InvariantViolation("unknown agent connection status"))?,
        permissions: Vec::new(),
        pending_expires_at: row.try_get("pending_expires_at")?,
        activated_at: row.try_get("activated_at")?,
        last_used_at: row.try_get("last_used_at")?,
        revoked_at: row.try_get("revoked_at")?,
        created_at: row.try_get("created_at")?,
    })
}
