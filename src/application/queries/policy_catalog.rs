use std::sync::Arc;

use chrono::{DateTime, Utc};

use crate::{
    authentication::AgentConnectionContext,
    domain::{ControlId, Policy, PolicyId, WorkspacePermission},
    projections::policy_projection::{PolicyCatalogEntry, PolicyDetail},
    repository::{Error as RepositoryError, Postgres},
};

#[derive(Debug, Clone, Copy)]
pub struct ListPolicies {
    pub connection: AgentConnectionContext,
}
#[derive(Clone)]
pub struct ListPoliciesHandler {
    repository: Arc<Postgres>,
}
impl ListPoliciesHandler {
    pub fn new(repository: Arc<Postgres>) -> Self {
        Self { repository }
    }
    pub async fn handle(
        &self,
        query: ListPolicies,
    ) -> Result<Vec<PolicyCatalogEntry>, PolicyCatalogError> {
        authorize(query.connection)?;
        Ok(self
            .repository
            .in_workspace_context_read(query.connection.workspace_id, async |context| {
                context.list_policy_catalog().await
            })
            .await?)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct GetPolicy {
    pub connection: AgentConnectionContext,
    pub policy_id: PolicyId,
}
#[derive(Clone)]
pub struct GetPolicyHandler {
    repository: Arc<Postgres>,
}
impl GetPolicyHandler {
    pub fn new(repository: Arc<Postgres>) -> Self {
        Self { repository }
    }
    pub async fn handle(
        &self,
        query: GetPolicy,
    ) -> Result<Option<PolicyDetail>, PolicyCatalogError> {
        authorize(query.connection)?;
        Ok(self
            .repository
            .in_workspace_context_read(query.connection.workspace_id, async move |context| {
                context.get_policy_detail(query.policy_id).await
            })
            .await?)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ListControlPolicyMappings {
    pub connection: AgentConnectionContext,
    pub control_id: ControlId,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlPolicyMappingProjection {
    pub policy: Policy,
    pub created_at: DateTime<Utc>,
}
#[derive(Clone)]
pub struct ListControlPolicyMappingsHandler {
    repository: Arc<Postgres>,
}
impl ListControlPolicyMappingsHandler {
    pub fn new(repository: Arc<Postgres>) -> Self {
        Self { repository }
    }
    pub async fn handle(
        &self,
        query: ListControlPolicyMappings,
    ) -> Result<Option<Vec<ControlPolicyMappingProjection>>, PolicyCatalogError> {
        authorize(query.connection)?;
        let client = self.repository.get().await.map_err(RepositoryError::from)?;
        if client
            .query_opt(
                "SELECT 1 FROM controls WHERE id = $1 AND workspace_id = $2",
                &[
                    &uuid::Uuid::from(query.control_id),
                    &uuid::Uuid::from(query.connection.workspace_id),
                ],
            )
            .await
            .map_err(RepositoryError::from)?
            .is_none()
        {
            return Ok(None);
        }
        client
            .query(
                REVERSE_MAPPINGS_SQL,
                &[
                    &uuid::Uuid::from(query.control_id),
                    &uuid::Uuid::from(query.connection.workspace_id),
                ],
            )
            .await
            .map_err(RepositoryError::from)?
            .into_iter()
            .map(|row| {
                Ok(ControlPolicyMappingProjection {
                    policy: Policy {
                        id: row.try_get::<_, uuid::Uuid>("id")?.into(),
                        workspace_id: row.try_get::<_, uuid::Uuid>("workspace_id")?.into(),
                        name: row.try_get("name")?,
                        description: row.try_get("description")?,
                        control_mappings: Vec::new(),
                        created_at: row.try_get("policy_created_at")?,
                        updated_at: row.try_get("updated_at")?,
                        archived_at: None,
                    },
                    created_at: row.try_get("mapping_created_at")?,
                })
            })
            .collect::<Result<Vec<_>, RepositoryError>>()
            .map(Some)
            .map_err(Into::into)
    }
}
const REVERSE_MAPPINGS_SQL: &str = "SELECT p.id, p.workspace_id, p.name, p.description, p.created_at AS policy_created_at, p.updated_at, m.created_at AS mapping_created_at FROM policy_control_mappings m JOIN policies p ON p.id = m.policy_id AND p.workspace_id = $2 WHERE m.control_id = $1 AND p.archived_at IS NULL ORDER BY lower(p.name), p.id";
fn authorize(connection: AgentConnectionContext) -> Result<(), PolicyCatalogError> {
    if connection
        .permissions
        .has(WorkspacePermission::ReadControls)
    {
        Ok(())
    } else {
        Err(PolicyCatalogError::Unavailable)
    }
}
#[derive(Debug, thiserror::Error)]
pub enum PolicyCatalogError {
    #[error("policy catalog is unavailable")]
    Unavailable,
    #[error("repository error")]
    Repository(#[from] RepositoryError),
}

#[cfg(test)]
mod tests {
    #[test]
    fn reverse_mapping_query_is_read_only_workspace_scoped_and_ordered() {
        assert!(!super::REVERSE_MAPPINGS_SQL.contains("UPDATE"));
        assert!(super::REVERSE_MAPPINGS_SQL.contains("p.workspace_id = $2"));
        assert!(super::REVERSE_MAPPINGS_SQL.contains("ORDER BY lower(p.name), p.id"));
    }
}
