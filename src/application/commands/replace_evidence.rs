use std::sync::Arc;

use chrono::Utc;

use crate::{
    application::ExecutionMetadata,
    authentication::AgentConnectionContext,
    domain::{
        Evidence, EvidenceAggregateError, EvidenceDefinition, EvidenceId, EvidenceStatus,
        WorkspacePermission,
    },
    repository::{Error as RepositoryError, Postgres},
};

#[derive(Debug, Clone)]
pub struct ReplaceEvidence {
    pub connection: AgentConnectionContext,
    pub evidence_id: EvidenceId,
    pub title: String,
    pub description: String,
    pub collection_instructions: String,
    pub status: EvidenceStatus,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplacedEvidence {
    pub evidence: Evidence,
}
#[derive(Clone)]
pub struct ReplaceEvidenceHandler {
    repository: Arc<Postgres>,
}
impl ReplaceEvidenceHandler {
    pub fn new(repository: Arc<Postgres>) -> Self {
        Self { repository }
    }
    pub async fn handle(
        &self,
        command: ReplaceEvidence,
        _metadata: ExecutionMetadata,
    ) -> Result<ReplacedEvidence, ReplaceEvidenceError> {
        if !command
            .connection
            .permissions
            .has(WorkspacePermission::WriteEvidence)
        {
            return Err(ReplaceEvidenceError::Unavailable);
        }
        let definition = EvidenceDefinition::new(
            command.title,
            command.description,
            command.collection_instructions,
        )
        .into_result()
        .map_err(ReplaceEvidenceError::InvalidDefinition)?;
        let evidence_id = command.evidence_id;
        let outcome = self
            .repository
            .in_agent_connection_workspace_context(
                command.connection.workspace_id,
                command.connection.user_id,
                command.connection.connection_id,
                async move |context| {
                    let repository = context.evidence();
                    let Some(mut evidence) = repository.get(evidence_id).await? else {
                        return Ok(None);
                    };
                    evidence
                        .replace(definition, command.status, Utc::now())
                        .map_err(|error| match error {
                            EvidenceAggregateError::InvalidRehydration
                            | EvidenceAggregateError::InvalidReplacementTime => {
                                RepositoryError::InvariantViolation("evidence snapshot is invalid")
                            }
                            EvidenceAggregateError::DuplicateControlMapping(_) => {
                                RepositoryError::InvariantViolation("evidence mappings are invalid")
                            }
                        })?;
                    repository.save(&evidence).await?;
                    context.get_evidence(evidence_id).await
                },
            )
            .await?;
        outcome
            .map(|evidence| ReplacedEvidence { evidence })
            .ok_or(ReplaceEvidenceError::Unavailable)
    }
}
#[derive(Debug, thiserror::Error)]
pub enum ReplaceEvidenceError {
    #[error("evidence is unavailable")]
    Unavailable,
    #[error("evidence definition is invalid")]
    InvalidDefinition(Vec<crate::domain::DomainError>),
    #[error("repository error")]
    Repository(#[from] RepositoryError),
}
