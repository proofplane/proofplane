use std::sync::Arc;

use chrono::Utc;
use uuid::Uuid;

use crate::{
    application::ExecutionMetadata,
    authentication::AgentConnectionContext,
    domain::{Evidence, EvidenceDefinition, EvidenceId, EvidenceStatus, WorkspacePermission},
    persistence::{Error as RepositoryError, Postgres},
    read_models::EvidenceDetail,
};

#[derive(Debug, Clone)]
pub struct CreateEvidence {
    pub connection: AgentConnectionContext,
    pub title: String,
    pub description: String,
    pub collection_instructions: String,
    pub status: EvidenceStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreatedEvidence {
    pub evidence: EvidenceDetail,
}

#[derive(Clone)]
pub struct CreateEvidenceHandler {
    repository: Arc<Postgres>,
}

impl CreateEvidenceHandler {
    pub fn new(repository: Arc<Postgres>) -> Self {
        Self { repository }
    }

    pub async fn handle(
        &self,
        command: CreateEvidence,
        _metadata: ExecutionMetadata,
    ) -> Result<CreatedEvidence, CreateEvidenceError> {
        if !command
            .connection
            .permissions
            .has(WorkspacePermission::WriteEvidence)
        {
            return Err(CreateEvidenceError::Unavailable);
        }
        let definition = EvidenceDefinition::new(
            command.title,
            command.description,
            command.collection_instructions,
        )
        .into_result()
        .map_err(CreateEvidenceError::InvalidDefinition)?;
        let id = EvidenceId::from(Uuid::new_v4());
        let evidence = Evidence::define(
            id,
            command.connection.workspace_id,
            definition,
            command.status,
            Utc::now(),
        );
        let evidence = self
            .repository
            .in_unit_of_work(async move |unit_of_work| {
                let workspace = unit_of_work.workspace(command.connection.workspace_id);
                workspace.aggregates().evidence().save(&evidence).await?;
                workspace.reads().evidence().get(id).await?.ok_or(
                    RepositoryError::InvariantViolation(
                        "created evidence must be readable in its transaction",
                    ),
                )
            })
            .await?;
        Ok(CreatedEvidence { evidence })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CreateEvidenceError {
    #[error("evidence is unavailable")]
    Unavailable,
    #[error("evidence definition is invalid")]
    InvalidDefinition(Vec<crate::domain::DomainError>),
    #[error("repository error")]
    Repository(#[from] RepositoryError),
}
