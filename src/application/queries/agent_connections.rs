use crate::{
    authentication::AgentConnectionContext,
    domain::{
        AgentConnectionId, AgentConnectionStatus, UserId, WorkspaceId, WorkspacePermission,
        WorkspacePermissions,
    },
    repository::{Error, Postgres},
};
use chrono::{DateTime, Utc};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct FindReusableAgentConnection {
    pub auth0_subject: String,
    pub auth0_client_id: String,
    pub resource: String,
    pub permissions: Vec<WorkspacePermission>,
}
#[derive(Debug, Clone, Copy)]
pub struct ResolveAgentConnectionAuthority {
    pub connection_id: AgentConnectionId,
}
#[derive(Debug, Clone, Copy)]
pub struct ListUserAgentConnections {
    pub user_id: UserId,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReusableAgentConnection {
    pub id: AgentConnectionId,
    pub user_id: UserId,
    pub workspace_id: WorkspaceId,
    pub permissions: Vec<WorkspacePermission>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentConnectionAuthority {
    pub context: AgentConnectionContext,
    pub auth0_subject: String,
    pub auth0_client_id: String,
    pub resource: String,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserAgentConnectionProjection {
    pub id: AgentConnectionId,
    pub client_name: String,
    pub status: AgentConnectionStatus,
    pub authorized_at: DateTime<Utc>,
    pub last_used_at: Option<DateTime<Utc>>,
}
#[derive(Clone)]
pub struct FindReusableAgentConnectionHandler {
    repository: Arc<Postgres>,
}
#[derive(Clone)]
pub struct ResolveAgentConnectionAuthorityHandler {
    repository: Arc<Postgres>,
}
#[derive(Clone)]
pub struct ListUserAgentConnectionsHandler {
    repository: Arc<Postgres>,
}
impl FindReusableAgentConnectionHandler {
    pub fn new(repository: Arc<Postgres>) -> Self {
        Self { repository }
    }
    pub async fn handle(
        &self,
        query: FindReusableAgentConnection,
    ) -> Result<Option<ReusableAgentConnection>, Error> {
        let row = self
            .repository
            .get()
            .await?
            .query_opt(
                REUSABLE_SQL,
                &[
                    &query.auth0_subject,
                    &query.auth0_client_id,
                    &query.resource,
                ],
            )
            .await?;
        row.map(|row| {
            let permissions = permissions(row.try_get("permissions")?)?;
            if permissions == query.permissions {
                Ok(Some(ReusableAgentConnection {
                    id: row.try_get::<_, Uuid>("id")?.into(),
                    user_id: row.try_get::<_, Uuid>("user_id")?.into(),
                    workspace_id: row.try_get::<_, Uuid>("workspace_id")?.into(),
                    permissions,
                }))
            } else {
                Ok(None)
            }
        })
        .transpose()
        .map(Option::flatten)
    }
}
impl ResolveAgentConnectionAuthorityHandler {
    pub fn new(repository: Arc<Postgres>) -> Self {
        Self { repository }
    }
    pub async fn handle(
        &self,
        query: ResolveAgentConnectionAuthority,
    ) -> Result<Option<AgentConnectionAuthority>, Error> {
        self.repository
            .get()
            .await?
            .query_opt(AUTHORITY_SQL, &[&Uuid::from(query.connection_id)])
            .await?
            .map(|row| {
                let permissions = permissions(row.try_get("permissions")?)?;
                Ok(AgentConnectionAuthority {
                    context: AgentConnectionContext {
                        user_id: row.try_get::<_, Uuid>("user_id")?.into(),
                        connection_id: query.connection_id,
                        workspace_id: row.try_get::<_, Uuid>("workspace_id")?.into(),
                        permissions: WorkspacePermissions::from_iter(permissions),
                    },
                    auth0_subject: row.try_get("auth0_subject")?,
                    auth0_client_id: row.try_get("auth0_client_id")?,
                    resource: row.try_get("resource")?,
                })
            })
            .transpose()
    }
}
impl ListUserAgentConnectionsHandler {
    pub fn new(repository: Arc<Postgres>) -> Self {
        Self { repository }
    }
    pub async fn handle(
        &self,
        query: ListUserAgentConnections,
    ) -> Result<Vec<UserAgentConnectionProjection>, Error> {
        self.repository
            .get()
            .await?
            .query(LIST_SQL, &[&Uuid::from(query.user_id)])
            .await?
            .into_iter()
            .map(|row| {
                Ok(UserAgentConnectionProjection {
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
fn permissions(values: Vec<String>) -> Result<Vec<WorkspacePermission>, Error> {
    values
        .into_iter()
        .map(|value| {
            value
                .parse()
                .map_err(|_| Error::InvariantViolation("unknown agent connection permission"))
        })
        .collect()
}
const REUSABLE_SQL: &str = "SELECT c.id, c.user_id, c.workspace_id, COALESCE(array_agg(p.permission ORDER BY array_position(ARRAY['read_evidence','write_evidence','read_evidence_submissions','write_evidence_submissions','read_controls','write_controls','manage_auditor_access'], p.permission)) FILTER (WHERE p.permission IS NOT NULL), ARRAY[]::text[]) AS permissions FROM agent_connections c JOIN users u ON u.id = c.user_id AND u.auth0_sub = c.auth0_subject JOIN workspace_memberships m ON m.user_id = c.user_id AND m.workspace_id = c.workspace_id LEFT JOIN agent_connection_permissions p ON p.agent_connection_id = c.id WHERE c.auth0_subject = $1 AND c.auth0_client_id = $2 AND c.resource = $3 AND c.status IN ('authorized', 'active') GROUP BY c.id";
const AUTHORITY_SQL: &str = "SELECT c.user_id, c.workspace_id, c.auth0_subject, c.auth0_client_id, c.resource, COALESCE(array_agg(p.permission ORDER BY array_position(ARRAY['read_evidence','write_evidence','read_evidence_submissions','write_evidence_submissions','read_controls','write_controls','manage_auditor_access'], p.permission)) FILTER (WHERE p.permission IS NOT NULL), ARRAY[]::text[]) AS permissions FROM agent_connections c JOIN users u ON u.id = c.user_id AND u.auth0_sub = c.auth0_subject JOIN workspace_memberships m ON m.user_id = c.user_id AND m.workspace_id = c.workspace_id LEFT JOIN agent_connection_permissions p ON p.agent_connection_id = c.id WHERE c.id = $1 AND c.status = 'active' GROUP BY c.id";
const LIST_SQL: &str = "SELECT c.id, c.client_display_name, c.status, t.consumed_at AS authorized_at, c.last_used_at FROM agent_connections c JOIN agent_authorization_transactions t ON t.agent_connection_id = c.id WHERE c.user_id = $1 AND c.status IN ('authorized', 'active') AND t.consumed_at IS NOT NULL ORDER BY t.consumed_at DESC, c.id DESC";
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reusable_query_is_read_only_and_keeps_permissions_ordered() {
        assert!(!REUSABLE_SQL.contains("UPDATE"));
        assert!(REUSABLE_SQL.contains("array_position"));
    }
    #[test]
    fn authority_query_conceals_digests_and_revoked_connections() {
        assert!(!AUTHORITY_SQL.contains("digest"));
        assert!(AUTHORITY_SQL.contains("c.status = 'active'"));
    }
    #[test]
    fn list_query_is_a_safe_audit_projection() {
        assert!(!LIST_SQL.contains("digest"));
        assert!(LIST_SQL.contains("ORDER BY t.consumed_at DESC, c.id DESC"));
    }
}
