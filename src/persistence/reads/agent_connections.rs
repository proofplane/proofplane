use uuid::Uuid;

use crate::{
    authentication::AgentConnectionContext,
    domain::{
        canonical_permissions, AgentConnectionId, UserId, WorkspacePermission, WorkspacePermissions,
    },
    persistence::Error,
    read_models::{AgentConnectionAuthority, ReusableAgentConnection, UserAgentConnectionSummary},
};

use super::{param, ReadExecutor};

pub(crate) struct AgentConnectionReads<'a, E> {
    executor: &'a E,
}
impl<'a, E> AgentConnectionReads<'a, E> {
    pub(crate) fn new(executor: &'a E) -> Self {
        Self { executor }
    }
}

impl<E: ReadExecutor> AgentConnectionReads<'_, E> {
    pub async fn find_reusable(
        &self,
        auth0_subject: &str,
        auth0_client_id: &str,
        resource: &str,
        requested: Vec<WorkspacePermission>,
    ) -> Result<Option<ReusableAgentConnection>, Error> {
        let expected = canonical_permissions(requested).map_err(|_| {
            Error::InvariantViolation("invalid requested agent connection permissions")
        })?;
        self.executor
            .query_opt(
                REUSABLE_SQL,
                &[
                    param(&auth0_subject),
                    param(&auth0_client_id),
                    param(&resource),
                ],
            )
            .await?
            .map(|row| {
                let permissions = permissions(row.try_get("permissions")?)?;
                if permissions != expected {
                    return Ok(None);
                }
                Ok(Some(ReusableAgentConnection {
                    id: row.try_get::<_, Uuid>("id")?.into(),
                    user_id: row.try_get::<_, Uuid>("user_id")?.into(),
                    workspace_id: row.try_get::<_, Uuid>("workspace_id")?.into(),
                    permissions,
                }))
            })
            .transpose()
            .map(Option::flatten)
    }
    pub async fn resolve_authority(
        &self,
        id: AgentConnectionId,
    ) -> Result<Option<AgentConnectionAuthority>, Error> {
        self.executor
            .query_opt(AUTHORITY_SQL, &[param(&Uuid::from(id))])
            .await?
            .map(|row| {
                let permissions = permissions(row.try_get("permissions")?)?;
                Ok(AgentConnectionAuthority {
                    context: AgentConnectionContext {
                        user_id: row.try_get::<_, Uuid>("user_id")?.into(),
                        connection_id: id,
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
    pub async fn list_for_user(
        &self,
        user_id: UserId,
    ) -> Result<Vec<UserAgentConnectionSummary>, Error> {
        self.executor.query(
            "SELECT c.id, c.client_display_name, c.status, t.consumed_at AS authorized_at, c.last_used_at FROM agent_connections c JOIN agent_authorization_transactions t ON t.agent_connection_id = c.id WHERE c.user_id = $1 AND c.status IN ('authorized', 'active') AND t.consumed_at IS NOT NULL ORDER BY t.consumed_at DESC, c.id DESC",
            &[param(&Uuid::from(user_id))],
        ).await?.into_iter().map(|row| Ok(UserAgentConnectionSummary { id: row.try_get::<_, Uuid>("id")?.into(), client_name: row.try_get("client_display_name")?, status: row.try_get::<_, String>("status")?.parse().map_err(|_| Error::InvariantViolation("unknown agent connection status"))?, authorized_at: row.try_get("authorized_at")?, last_used_at: row.try_get("last_used_at")? })).collect()
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
    use super::{AUTHORITY_SQL, REUSABLE_SQL};

    #[test]
    fn queries_are_read_only_ordered_and_conceal_secrets() {
        assert!(!REUSABLE_SQL.contains("UPDATE"));
        assert!(REUSABLE_SQL.contains("array_position"));
        assert!(!AUTHORITY_SQL.contains("digest"));
        assert!(AUTHORITY_SQL.contains("c.status = 'active'"));
    }
}
