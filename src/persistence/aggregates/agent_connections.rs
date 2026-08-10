use chrono::{DateTime, Utc};
use deadpool_postgres::GenericClient;
use tokio_postgres::Row;
use uuid::Uuid;

use super::{
    snapshot::{save_snapshot, snapshot_record},
    Error, Postgres, UnitOfWork,
};
use crate::domain::{AgentConnection, AgentConnectionId, Sha256Digest, WorkspacePermission};

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
            RepositoryConnection::Transaction(unit_of_work) => {
                unit_of_work
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
            .map(|row| {
                let authorization = AgentAuthorizationTransactionRecord::try_from_row(&row)?;
                AgentConnectionRecord::try_from_row(&row)?.into_domain(&row, authorization)
            })
            .transpose()
    }

    /// Saves the aggregate's complete state. Eligibility and relationship policy belong in handlers.
    pub async fn save(&self, connection: &AgentConnection) -> Result<(), Error> {
        let RepositoryConnection::Transaction(unit_of_work) = self.connection else {
            return Err(Error::InvariantViolation(
                "agent connections must be saved in a transaction",
            ));
        };
        let record = AgentConnectionRecord::from_domain(connection)?;
        save_snapshot(&unit_of_work.transaction, record.as_snapshot()).await?;
        unit_of_work
            .transaction
            .execute(
                "DELETE FROM agent_connection_permissions WHERE agent_connection_id = $1",
                &[&record.id],
            )
            .await?;
        for permission in connection
            .permissions
            .iter()
            .map(|permission| AgentConnectionPermissionRecord::from_domain(record.id, *permission))
        {
            unit_of_work.transaction.execute("INSERT INTO agent_connection_permissions (agent_connection_id, permission) VALUES ($1, $2)", &[&permission.agent_connection_id, &permission.permission]).await?;
        }
        let authorization = AgentAuthorizationTransactionRecord::from_domain(connection)?;
        save_snapshot(&unit_of_work.transaction, authorization.as_snapshot()).await
    }
}

struct AgentConnectionPermissionRecord {
    agent_connection_id: Uuid,
    permission: String,
}

impl AgentConnectionPermissionRecord {
    fn from_domain(agent_connection_id: Uuid, permission: WorkspacePermission) -> Self {
        Self {
            agent_connection_id,
            permission: permission.as_str().to_owned(),
        }
    }
}

const GET_SQL: &str = r#"SELECT c.id, c.user_id, c.workspace_id, c.auth0_subject, c.auth0_client_id, c.client_display_name, c.resource, c.status, c.pending_expires_at, c.activated_at, c.last_used_at, c.revoked_at, c.created_at, t.id AS transaction_id, t.continuation_digest, t.nonce_digest, t.consumed_at, COALESCE((SELECT array_agg(p.permission ORDER BY array_position(ARRAY['read_evidence','write_evidence','read_evidence_submissions','write_evidence_submissions','read_controls','write_controls','manage_auditor_access'], p.permission)) FROM agent_connection_permissions p WHERE p.agent_connection_id = c.id), ARRAY[]::text[]) AS permissions FROM agent_connections c JOIN agent_authorization_transactions t ON t.agent_connection_id = c.id WHERE c.id = $1"#;
const GET_FOR_UPDATE_SQL: &str = "FOR UPDATE OF c, t";

snapshot_record! {
    struct AgentConnectionRecord { id: Uuid, user_id: Uuid, workspace_id: Uuid, auth0_subject: String, auth0_client_id: String, client_display_name: String, resource: String, status: String, pending_expires_at: DateTime<Utc>, activated_at: Option<DateTime<Utc>>, last_used_at: Option<DateTime<Utc>>, revoked_at: Option<DateTime<Utc>>, created_at: DateTime<Utc>, }
    table: agent_connections, conflict: id,
}

impl AgentConnectionRecord {
    fn try_from_row(row: &Row) -> Result<Self, Error> {
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
    fn into_domain(
        self,
        row: &Row,
        authorization: AgentAuthorizationTransactionRecord,
    ) -> Result<AgentConnection, Error> {
        let continuation: [u8; 32] =
            authorization.continuation_digest.try_into().map_err(|_| {
                Error::InvariantViolation("agent continuation digest must contain 32 bytes")
            })?;
        let nonce: [u8; 32] = authorization
            .nonce_digest
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
            self.id.into(),
            self.user_id.into(),
            self.workspace_id.into(),
            self.auth0_subject,
            self.auth0_client_id,
            self.client_display_name,
            self.resource,
            self.status
                .parse()
                .map_err(|_| Error::InvariantViolation("unknown agent connection status"))?,
            permissions,
            self.pending_expires_at,
            self.activated_at,
            self.last_used_at,
            self.revoked_at,
            self.created_at,
            authorization.id.into(),
            Sha256Digest::from_bytes(continuation),
            Sha256Digest::from_bytes(nonce),
            authorization.consumed_at,
        )
        .map_err(|_| Error::InvariantViolation("persisted agent connection is inconsistent"))
    }

    fn from_domain(value: &AgentConnection) -> Result<Self, Error> {
        Ok(Self {
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
        })
    }
}

snapshot_record! {
    struct AgentAuthorizationTransactionRecord {
        id: Uuid,
        agent_connection_id: Uuid,
        continuation_digest: Vec<u8>,
        nonce_digest: Vec<u8>,
        consumed_at: Option<DateTime<Utc>>,
        created_at: DateTime<Utc>,
    }
    table: agent_authorization_transactions,
    conflict: agent_connection_id,
}

impl AgentAuthorizationTransactionRecord {
    fn try_from_row(row: &Row) -> Result<Self, Error> {
        Ok(Self {
            id: row.try_get("transaction_id")?,
            agent_connection_id: row.try_get("id")?,
            continuation_digest: row.try_get("continuation_digest")?,
            nonce_digest: row.try_get("nonce_digest")?,
            consumed_at: row.try_get("consumed_at")?,
            created_at: row.try_get("created_at")?,
        })
    }

    fn from_domain(connection: &AgentConnection) -> Result<Self, Error> {
        Ok(Self {
            id: connection.authorization_transaction_id().into(),
            agent_connection_id: connection.id.into(),
            continuation_digest: connection.continuation_digest().as_bytes().to_vec(),
            nonce_digest: connection.nonce_digest().as_bytes().to_vec(),
            consumed_at: connection.continuation_consumed_at(),
            created_at: connection.created_at,
        })
    }
}

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
}
