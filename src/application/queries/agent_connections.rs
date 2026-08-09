use crate::{
    authentication::AgentConnectionContext,
    domain::{
        canonical_permissions, AgentConnectionId, UserId, WorkspaceId, WorkspacePermission,
        WorkspacePermissions,
    },
    repository::{Error, Postgres},
};
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
        let expected_permissions = canonical_permissions(query.permissions).map_err(|_| {
            Error::InvariantViolation("invalid requested agent connection permissions")
        })?;
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
            if permissions == expected_permissions {
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
    ) -> Result<Vec<crate::projections::UserAgentConnectionSummary>, Error> {
        self.repository
            .agent_connection_projections()
            .list_for_user(query.user_id)
            .await
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
}
