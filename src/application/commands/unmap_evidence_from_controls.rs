use crate::{
    application::ExecutionMetadata,
    authentication::AgentConnectionContext,
    domain::{ControlId, EvidenceId, WorkspacePermission},
    repository::{Error as RepositoryError, Postgres},
};
use std::sync::Arc;
#[derive(Debug, Clone)]
pub struct UnmapEvidenceFromControls {
    pub connection: AgentConnectionContext,
    pub evidence_id: EvidenceId,
    pub control_ids: Vec<ControlId>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnmappedEvidenceFromControls {
    pub control_ids: Vec<ControlId>,
}
#[derive(Clone)]
pub struct UnmapEvidenceFromControlsHandler {
    repository: Arc<Postgres>,
}
impl UnmapEvidenceFromControlsHandler {
    pub fn new(repository: Arc<Postgres>) -> Self {
        Self { repository }
    }
    pub async fn handle(
        &self,
        command: UnmapEvidenceFromControls,
        _metadata: ExecutionMetadata,
    ) -> Result<UnmappedEvidenceFromControls, UnmapEvidenceFromControlsError> {
        if !command
            .connection
            .permissions
            .has(WorkspacePermission::WriteControls)
        {
            return Err(UnmapEvidenceFromControlsError::Unavailable);
        }
        let evidence_id = command.evidence_id;
        let requested = command.control_ids;
        let result = self
            .repository
            .in_agent_connection_workspace_context(
                command.connection.workspace_id,
                command.connection.user_id,
                command.connection.connection_id,
                async move |context| {
                    let repository = context.evidence();
                    let Some(mut evidence) = repository.get(evidence_id).await? else {
                        return Ok(UnmapOutcome::Unavailable);
                    };
                    if requested.iter().any(|id| {
                        !evidence
                            .mappings()
                            .iter()
                            .any(|mapping| mapping.control_id() == *id)
                    }) {
                        return Ok(UnmapOutcome::Rejected);
                    }
                    evidence
                        .replace_mappings(
                            evidence
                                .mappings()
                                .iter()
                                .filter(|mapping| !requested.contains(&mapping.control_id()))
                                .cloned()
                                .collect(),
                        )
                        .map_err(|_| {
                            RepositoryError::InvariantViolation("evidence mappings are invalid")
                        })?;
                    repository.save(&evidence).await?;
                    Ok(UnmapOutcome::Unmapped(requested))
                },
            )
            .await?;
        match result {
            UnmapOutcome::Unmapped(control_ids) => Ok(UnmappedEvidenceFromControls { control_ids }),
            UnmapOutcome::Unavailable => Err(UnmapEvidenceFromControlsError::Unavailable),
            UnmapOutcome::Rejected => Err(UnmapEvidenceFromControlsError::Rejected),
        }
    }
}
enum UnmapOutcome {
    Unmapped(Vec<ControlId>),
    Unavailable,
    Rejected,
}
#[derive(Debug, thiserror::Error)]
pub enum UnmapEvidenceFromControlsError {
    #[error("evidence is unavailable")]
    Unavailable,
    #[error("control mappings are invalid")]
    Rejected,
    #[error("repository error")]
    Repository(#[from] RepositoryError),
}
