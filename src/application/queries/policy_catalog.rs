use std::sync::Arc;

use crate::{
    authentication::AgentConnectionContext,
    domain::{ControlId, PolicyId, WorkspacePermission},
    persistence::{Error as RepositoryError, Postgres},
    read_models::{ControlPolicyMapping, PolicyCatalogEntry, PolicyDetail},
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
            .workspace_reads(query.connection.workspace_id)
            .await?
            .policies()
            .list_catalog()
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
            .workspace_reads(query.connection.workspace_id)
            .await?
            .policies()
            .get(query.policy_id)
            .await?)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ListControlPolicyMappings {
    pub connection: AgentConnectionContext,
    pub control_id: ControlId,
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
    ) -> Result<Option<Vec<ControlPolicyMapping>>, PolicyCatalogError> {
        authorize(query.connection)?;
        Ok(self
            .repository
            .workspace_reads(query.connection.workspace_id)
            .await?
            .controls()
            .list_policies_for_control(query.control_id)
            .await?)
    }
}
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
