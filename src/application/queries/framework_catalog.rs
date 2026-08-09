use std::sync::Arc;

use crate::{
    authentication::AgentConnectionContext,
    domain::{FrameworkId, WorkspacePermission},
    projections::{FrameworkDetail, FrameworkRequirementDetail},
    repository::{Error as RepositoryError, Postgres},
};

#[derive(Debug, Clone, Copy)]
pub struct ListFrameworks {
    pub connection: AgentConnectionContext,
}

#[derive(Clone)]
pub struct ListFrameworksHandler {
    repository: Arc<Postgres>,
}

impl ListFrameworksHandler {
    pub fn new(repository: Arc<Postgres>) -> Self {
        Self { repository }
    }

    pub async fn handle(
        &self,
        query: ListFrameworks,
    ) -> Result<Vec<FrameworkDetail>, FrameworkCatalogError> {
        authorize(query.connection)?;
        Ok(self.repository.framework_projections().list().await?)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ListFrameworkRequirements {
    pub connection: AgentConnectionContext,
    pub framework_id: FrameworkId,
}

#[derive(Clone)]
pub struct ListFrameworkRequirementsHandler {
    repository: Arc<Postgres>,
}

impl ListFrameworkRequirementsHandler {
    pub fn new(repository: Arc<Postgres>) -> Self {
        Self { repository }
    }

    pub async fn handle(
        &self,
        query: ListFrameworkRequirements,
    ) -> Result<Vec<FrameworkRequirementDetail>, FrameworkCatalogError> {
        authorize(query.connection)?;
        Ok(self
            .repository
            .framework_projections()
            .list_requirements(query.framework_id)
            .await?)
    }
}

fn authorize(connection: AgentConnectionContext) -> Result<(), FrameworkCatalogError> {
    if connection
        .permissions
        .has(WorkspacePermission::ReadControls)
    {
        Ok(())
    } else {
        Err(FrameworkCatalogError::Unavailable)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum FrameworkCatalogError {
    #[error("framework catalog is unavailable")]
    Unavailable,
    #[error("repository error")]
    Repository(#[from] RepositoryError),
}
