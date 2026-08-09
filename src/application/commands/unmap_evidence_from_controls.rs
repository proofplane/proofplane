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
            .in_unit_of_work(async move |unit_of_work| {
                let workspace = unit_of_work.for_workspace(command.connection.workspace_id);
                let context = &workspace;
                let repository = context.evidence();
                let Some(mut evidence) = repository.get(evidence_id).await? else {
                    return Ok(UnmapOutcome::Unavailable);
                };
                let existing_controls = context.existing_control_ids(&requested).await?;
                let unknown = requested
                    .iter()
                    .copied()
                    .filter(|id| !existing_controls.contains(id))
                    .collect::<Vec<_>>();
                let not_mapped = requested
                    .iter()
                    .copied()
                    .filter(|id| existing_controls.contains(id))
                    .filter(|id| {
                        !evidence
                            .mappings()
                            .iter()
                            .any(|mapping| mapping.control_id() == *id)
                    })
                    .collect::<Vec<_>>();
                if !unknown.is_empty() || !not_mapped.is_empty() {
                    return Ok(UnmapOutcome::Rejected {
                        unknown,
                        not_mapped,
                    });
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
            })
            .await?;
        match result {
            UnmapOutcome::Unmapped(control_ids) => Ok(UnmappedEvidenceFromControls { control_ids }),
            UnmapOutcome::Unavailable => Err(UnmapEvidenceFromControlsError::EvidenceNotFound),
            UnmapOutcome::Rejected {
                unknown,
                not_mapped,
            } => Err(UnmapEvidenceFromControlsError::Rejected {
                unknown,
                not_mapped,
            }),
        }
    }
}
enum UnmapOutcome {
    Unmapped(Vec<ControlId>),
    Unavailable,
    Rejected {
        unknown: Vec<ControlId>,
        not_mapped: Vec<ControlId>,
    },
}
#[derive(Debug, thiserror::Error)]
pub enum UnmapEvidenceFromControlsError {
    #[error("evidence is unavailable")]
    Unavailable,
    #[error("evidence was not found")]
    EvidenceNotFound,
    #[error("control mappings are invalid")]
    Rejected {
        unknown: Vec<ControlId>,
        not_mapped: Vec<ControlId>,
    },
    #[error("repository error")]
    Repository(#[from] RepositoryError),
}
