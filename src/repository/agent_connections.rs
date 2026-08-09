use chrono::{DateTime, Utc};
use deadpool_postgres::GenericClient;
use tokio_postgres::Row;
use uuid::Uuid;

use super::{
    snapshot::{save_workspace_snapshot, workspace_snapshot_record},
    Error, Postgres, UnitOfWork,
};
use crate::{
    domain::{AgentConnection, AgentConnectionId, Sha256Digest, UserId, WorkspacePermission},
    projections::UserAgentConnectionSummary,
};

enum RepositoryConnection<'a> {
    Postgres(&'a Postgres),
    Transaction(&'a UnitOfWork<'a>),
}
pub struct AgentConnectionRepository<'a> {
    connection: RepositoryConnection<'a>,
}

impl Postgres {
    pub fn agent_connections(&self) -> AgentConnectionRepository<'_> {
        AgentConnectionRepository {
            connection: RepositoryConnection::Postgres(self),
        }
    }
}
impl<'a> UnitOfWork<'a> {
    pub fn agent_connections(&'a self) -> AgentConnectionRepository<'a> {
        AgentConnectionRepository {
            connection: RepositoryConnection::Transaction(self),
        }
    }
}

impl AgentConnectionRepository<'_> {
    /// Rehydrates the whole connection, including permissions and the opaque continuation digests.
    /// Transactional reads lock the snapshot through the surrounding commit.
    pub async fn get(&self, id: AgentConnectionId) -> Result<Option<AgentConnection>, Error> {
        let rows = match self.connection {
            RepositoryConnection::Postgres(postgres) => {
                postgres
                    .get()
                    .await?
                    .query(GET_SQL, &[&Uuid::from(id)])
                    .await?
            }
            RepositoryConnection::Transaction(context) => {
                context
                    .transaction
                    .query(
                        &format!("{GET_SQL} {GET_FOR_UPDATE_SQL}"),
                        &[&Uuid::from(id)],
                    )
                    .await?
            }
        };
        rows.into_iter()
            .next()
            .map(AgentConnection::try_from)
            .transpose()
    }

    /// Saves the aggregate's complete state. Eligibility and relationship policy belong in handlers.
    pub async fn save(&self, connection: &AgentConnection) -> Result<(), Error> {
        let RepositoryConnection::Transaction(context) = self.connection else {
            return Err(Error::InvariantViolation(
                "agent connections must be saved in a transaction",
            ));
        };
        let record = ConnectionRecord::from(connection);
        save_workspace_snapshot(&context.transaction, record.as_workspace_snapshot()).await?;
        context
            .transaction
            .execute(
                "DELETE FROM agent_connection_permissions WHERE agent_connection_id = $1",
                &[&record.id],
            )
            .await?;
        for permission in &connection.permissions {
            context.transaction.execute("INSERT INTO agent_connection_permissions (agent_connection_id, permission) VALUES ($1, $2)", &[&record.id, &permission.as_str()]).await?;
        }
        context.transaction.execute(
            "INSERT INTO agent_authorization_transactions (id, agent_connection_id, continuation_digest, nonce_digest, consumed_at, created_at) VALUES ($1, $2, $3, $4, $5, $6) ON CONFLICT (agent_connection_id) DO UPDATE SET id = EXCLUDED.id, continuation_digest = EXCLUDED.continuation_digest, nonce_digest = EXCLUDED.nonce_digest, consumed_at = EXCLUDED.consumed_at",
            &[&Uuid::from(connection.authorization_transaction_id()), &record.id, &connection.continuation_digest().as_bytes().as_slice(), &connection.nonce_digest().as_bytes().as_slice(), &connection.continuation_consumed_at(), &connection.created_at]
        ).await?;
        Ok(())
    }
}

const GET_SQL: &str = r#"SELECT c.id, c.user_id, c.workspace_id, c.auth0_subject, c.auth0_client_id, c.client_display_name, c.resource, c.status, c.pending_expires_at, c.activated_at, c.last_used_at, c.revoked_at, c.created_at, t.id AS transaction_id, t.continuation_digest, t.nonce_digest, t.consumed_at, COALESCE((SELECT array_agg(p.permission ORDER BY array_position(ARRAY['read_evidence','write_evidence','read_evidence_submissions','write_evidence_submissions','read_controls','write_controls','manage_auditor_access'], p.permission)) FROM agent_connection_permissions p WHERE p.agent_connection_id = c.id), ARRAY[]::text[]) AS permissions FROM agent_connections c JOIN agent_authorization_transactions t ON t.agent_connection_id = c.id WHERE c.id = $1"#;
const GET_FOR_UPDATE_SQL: &str = "FOR UPDATE OF c, t";

workspace_snapshot_record! {
    struct ConnectionRecord { id: Uuid, user_id: Uuid, workspace_id: Uuid, auth0_subject: String, auth0_client_id: String, client_display_name: String, resource: String, status: String, pending_expires_at: DateTime<Utc>, activated_at: Option<DateTime<Utc>>, last_used_at: Option<DateTime<Utc>>, revoked_at: Option<DateTime<Utc>>, created_at: DateTime<Utc>, }
    table: agent_connections, conflict: id, scope: workspace_id,
}

impl TryFrom<Row> for ConnectionRecord {
    type Error = Error;
    fn try_from(row: Row) -> Result<Self, Self::Error> {
        Ok(Self {
            id: row.try_get("id")?,
            user_id: row.try_get("user_id")?,
            workspace_id: row.try_get("workspace_id")?,
            auth0_subject: row.try_get("auth0_subject")?,
            auth0_client_id: row.try_get("auth0_client_id")?,
            client_display_name: row.try_get("client_display_name")?,
            resource: row.try_get("resource")?,
            status: row.try_get::<_, String>("status")?,
            pending_expires_at: row.try_get("pending_expires_at")?,
            activated_at: row.try_get("activated_at")?,
            last_used_at: row.try_get("last_used_at")?,
            revoked_at: row.try_get("revoked_at")?,
            created_at: row.try_get("created_at")?,
        })
    }
}
impl TryFrom<Row> for AgentConnection {
    type Error = Error;
    fn try_from(row: Row) -> Result<Self, Self::Error> {
        let record = ConnectionRecord::try_from(row.clone())?;
        let continuation: [u8; 32] = row
            .try_get::<_, Vec<u8>>("continuation_digest")?
            .try_into()
            .map_err(|_| {
                Error::InvariantViolation("agent continuation digest must contain 32 bytes")
            })?;
        let nonce: [u8; 32] = row
            .try_get::<_, Vec<u8>>("nonce_digest")?
            .try_into()
            .map_err(|_| Error::InvariantViolation("agent nonce digest must contain 32 bytes"))?;
        let permissions = row
            .try_get::<_, Vec<String>>("permissions")?
            .into_iter()
            .map(|value| {
                value
                    .parse()
                    .map_err(|_| Error::InvariantViolation("unknown agent connection permission"))
            })
            .collect::<Result<Vec<WorkspacePermission>, Error>>()?;
        AgentConnection::rehydrate(
            record.id.into(),
            record.user_id.into(),
            record.workspace_id.into(),
            record.auth0_subject,
            record.auth0_client_id,
            record.client_display_name,
            record.resource,
            record
                .status
                .parse()
                .map_err(|_| Error::InvariantViolation("unknown agent connection status"))?,
            permissions,
            record.pending_expires_at,
            record.activated_at,
            record.last_used_at,
            record.revoked_at,
            record.created_at,
            row.try_get::<_, Uuid>("transaction_id")?.into(),
            Sha256Digest::from_bytes(continuation),
            Sha256Digest::from_bytes(nonce),
            row.try_get("consumed_at")?,
        )
        .map_err(|_| Error::InvariantViolation("persisted agent connection is inconsistent"))
    }
}
impl From<&AgentConnection> for ConnectionRecord {
    fn from(value: &AgentConnection) -> Self {
        Self {
            id: value.id.into(),
            user_id: value.user_id.into(),
            workspace_id: value.workspace_id.into(),
            auth0_subject: value.auth0_subject.clone(),
            auth0_client_id: value.auth0_client_id.clone(),
            client_display_name: value.client_display_name.clone(),
            resource: value.resource.clone(),
            status: value.status.as_str().to_owned(),
            pending_expires_at: value.pending_expires_at,
            activated_at: value.activated_at,
            last_used_at: value.last_used_at,
            revoked_at: value.revoked_at,
            created_at: value.created_at,
        }
    }
}

impl Postgres {
    pub(super) async fn load_user_agent_connection_summaries(
        &self,
        user_id: UserId,
    ) -> Result<Vec<UserAgentConnectionSummary>, Error> {
        self.get()
            .await?
            .query(LIST_SUMMARIES_SQL, &[&Uuid::from(user_id)])
            .await?
            .into_iter()
            .map(|row| {
                Ok(UserAgentConnectionSummary {
                    id: row.try_get::<_, Uuid>("id")?.into(),
                    client_name: row.try_get("client_display_name")?,
                    status: row.try_get::<_, String>("status")?.parse().map_err(|_| {
                        Error::InvariantViolation("unknown agent connection status")
                    })?,
                    authorized_at: row.try_get("authorized_at")?,
                    last_used_at: row.try_get("last_used_at")?,
                })
            })
            .collect()
    }
}

const LIST_SUMMARIES_SQL: &str = "SELECT c.id, c.client_display_name, c.status, t.consumed_at AS authorized_at, c.last_used_at FROM agent_connections c JOIN agent_authorization_transactions t ON t.agent_connection_id = c.id WHERE c.user_id = $1 AND c.status IN ('authorized', 'active') AND t.consumed_at IS NOT NULL ORDER BY t.consumed_at DESC, c.id DESC";

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn snapshot_read_locks_only_in_a_transaction() {
        assert!(!GET_SQL.contains("FOR UPDATE"));
        assert!(GET_FOR_UPDATE_SQL.contains("FOR UPDATE OF c, t"));
    }
    #[test]
    fn snapshot_query_loads_digests_and_canonical_permissions() {
        assert!(GET_SQL.contains("continuation_digest"));
        assert!(GET_SQL.contains("array_position"));
    }
    #[test]
    fn summary_query_is_a_safe_audit_projection() {
        assert!(!LIST_SUMMARIES_SQL.contains("digest"));
        assert!(LIST_SUMMARIES_SQL.contains("ORDER BY t.consumed_at DESC, c.id DESC"));
    }
}
