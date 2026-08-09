use std::sync::Arc;

use crate::{
    authentication::AgentConnectionContext,
    domain::{ControlId, EvidenceId, WorkspacePermission},
    projections::{ControlEvidenceMapping, EvidenceControlMapping, EvidenceDetail},
    repository::{Error as RepositoryError, Postgres},
};

#[derive(Debug, Clone, Copy)]
pub struct ListEvidence {
    pub connection: AgentConnectionContext,
}

#[derive(Clone)]
pub struct ListEvidenceHandler {
    repository: Arc<Postgres>,
}

impl ListEvidenceHandler {
    pub fn new(repository: Arc<Postgres>) -> Self {
        Self { repository }
    }

    pub async fn handle(
        &self,
        query: ListEvidence,
    ) -> Result<Vec<EvidenceDetail>, EvidenceCatalogError> {
        authorize_evidence(query.connection)?;
        Ok(self
            .repository
            .in_workspace_context_read(query.connection.workspace_id, async |context| {
                context.evidence_projections().list().await
            })
            .await?)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct GetEvidence {
    pub connection: AgentConnectionContext,
    pub evidence_id: EvidenceId,
}

#[derive(Clone)]
pub struct GetEvidenceHandler {
    repository: Arc<Postgres>,
}

impl GetEvidenceHandler {
    pub fn new(repository: Arc<Postgres>) -> Self {
        Self { repository }
    }

    pub async fn handle(
        &self,
        query: GetEvidence,
    ) -> Result<Option<EvidenceDetail>, EvidenceCatalogError> {
        authorize_evidence(query.connection)?;
        Ok(self
            .repository
            .in_workspace_context_read(query.connection.workspace_id, async move |context| {
                context.evidence_projections().get(query.evidence_id).await
            })
            .await?)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ListEvidenceControlMappings {
    pub connection: AgentConnectionContext,
    pub evidence_id: EvidenceId,
}

#[derive(Clone)]
pub struct ListEvidenceControlMappingsHandler {
    repository: Arc<Postgres>,
}

impl ListEvidenceControlMappingsHandler {
    pub fn new(repository: Arc<Postgres>) -> Self {
        Self { repository }
    }

    pub async fn handle(
        &self,
        query: ListEvidenceControlMappings,
    ) -> Result<Option<Vec<EvidenceControlMapping>>, EvidenceCatalogError> {
        if !query
            .connection
            .permissions
            .has(WorkspacePermission::ReadControls)
        {
            return Err(EvidenceCatalogError::Unavailable);
        }
        Ok(self
            .repository
            .in_workspace_context_read(query.connection.workspace_id, async move |context| {
                context
                    .control_projections()
                    .list_evidence_mappings(query.evidence_id)
                    .await
            })
            .await?)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ListControlEvidenceMappings {
    pub connection: AgentConnectionContext,
    pub control_id: ControlId,
}

#[derive(Clone)]
pub struct ListControlEvidenceMappingsHandler {
    repository: Arc<Postgres>,
}

impl ListControlEvidenceMappingsHandler {
    pub fn new(repository: Arc<Postgres>) -> Self {
        Self { repository }
    }

    pub async fn handle(
        &self,
        query: ListControlEvidenceMappings,
    ) -> Result<Option<Vec<ControlEvidenceMapping>>, EvidenceCatalogError> {
        if !query
            .connection
            .permissions
            .has(WorkspacePermission::ReadControls)
        {
            return Err(EvidenceCatalogError::Unavailable);
        }
        Ok(self
            .repository
            .in_workspace_context_read(query.connection.workspace_id, async move |context| {
                context
                    .control_projections()
                    .list_evidence_for_control(query.control_id)
                    .await
            })
            .await?)
    }
}

fn authorize_evidence(connection: AgentConnectionContext) -> Result<(), EvidenceCatalogError> {
    if connection
        .permissions
        .has(WorkspacePermission::ReadEvidence)
    {
        Ok(())
    } else {
        Err(EvidenceCatalogError::Unavailable)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum EvidenceCatalogError {
    #[error("evidence is unavailable")]
    Unavailable,
    #[error("repository error")]
    Repository(#[from] RepositoryError),
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use deadpool_postgres::{Manager, Pool};
    use tokio_postgres::{Config, NoTls};
    use uuid::Uuid;

    use crate::{
        authentication::AgentConnectionContext,
        domain::{AgentConnectionId, EvidenceId, UserId, WorkspaceId, WorkspacePermissions},
        repository::Postgres,
    };

    use super::{
        EvidenceCatalogError, GetEvidence, GetEvidenceHandler, ListEvidence, ListEvidenceHandler,
    };

    #[tokio::test]
    async fn evidence_catalog_queries_conceal_data_without_read_evidence_permission() {
        let connection = connection();
        let list = ListEvidenceHandler::new(repository())
            .handle(ListEvidence { connection })
            .await;
        let detail = GetEvidenceHandler::new(repository())
            .handle(GetEvidence {
                connection,
                evidence_id: EvidenceId::from(Uuid::new_v4()),
            })
            .await;

        assert!(matches!(list, Err(EvidenceCatalogError::Unavailable)));
        assert!(matches!(detail, Err(EvidenceCatalogError::Unavailable)));
    }

    fn repository() -> Arc<Postgres> {
        let pool = Pool::builder(Manager::new(Config::new(), NoTls))
            .build()
            .unwrap();
        Arc::new(Postgres::new(pool))
    }

    fn connection() -> AgentConnectionContext {
        AgentConnectionContext {
            user_id: UserId::from(Uuid::new_v4()),
            connection_id: AgentConnectionId::from(Uuid::new_v4()),
            workspace_id: WorkspaceId::from(Uuid::new_v4()),
            permissions: WorkspacePermissions::none(),
        }
    }
}
