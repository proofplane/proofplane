use std::sync::Arc;

use crate::{
    application::ExecutionMetadata,
    authentication::AgentConnectionContext,
    domain::{ControlId, Evidence, EvidenceId, WorkspacePermission},
    repository::{Error as RepositoryError, Postgres},
};

#[derive(Debug, Clone)]
pub struct UnmapControlFromEvidence {
    pub connection: AgentConnectionContext,
    pub control_id: ControlId,
    pub evidence_ids: Vec<EvidenceId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnmappedControlFromEvidence {
    pub evidence_ids: Vec<EvidenceId>,
}

#[derive(Clone)]
pub struct UnmapControlFromEvidenceHandler {
    repository: Arc<Postgres>,
}

impl UnmapControlFromEvidenceHandler {
    pub fn new(repository: Arc<Postgres>) -> Self {
        Self { repository }
    }

    pub async fn handle(
        &self,
        command: UnmapControlFromEvidence,
        _metadata: ExecutionMetadata,
    ) -> Result<UnmappedControlFromEvidence, UnmapControlFromEvidenceError> {
        if !command
            .connection
            .permissions
            .has(WorkspacePermission::WriteControls)
        {
            return Err(UnmapControlFromEvidenceError::Unavailable);
        }
        let control_id = command.control_id;
        let requested_ids = command.evidence_ids;
        let mut lock_order = requested_ids.clone();
        lock_order.sort_unstable_by_key(|id| uuid::Uuid::from(*id));

        let outcome = self
            .repository
            .in_unit_of_work(async move |unit_of_work| {
                let workspace = unit_of_work.for_workspace(command.connection.workspace_id);
                let context = &workspace;
                if !context.controls_exist(&[control_id]).await? {
                    return Ok(UnmapOutcome::ControlNotFound);
                }
                let repository = context.evidence();
                let mut evidence = Vec::<Evidence>::new();
                for evidence_id in lock_order {
                    if let Some(aggregate) = repository.get(evidence_id).await? {
                        evidence.push(aggregate);
                    }
                }
                let unknown = requested_ids
                    .iter()
                    .copied()
                    .filter(|id| !evidence.iter().any(|item| item.id() == *id))
                    .collect::<Vec<_>>();
                let not_mapped = requested_ids
                    .iter()
                    .copied()
                    .filter(|id| {
                        evidence.iter().any(|item| item.id() == *id)
                            && !evidence.iter().any(|item| {
                                item.id() == *id
                                    && item
                                        .mappings()
                                        .iter()
                                        .any(|mapping| mapping.control_id() == control_id)
                            })
                    })
                    .collect::<Vec<_>>();
                if !unknown.is_empty() || !not_mapped.is_empty() {
                    return Ok(UnmapOutcome::Rejected {
                        unknown,
                        not_mapped,
                    });
                }
                for mut item in evidence {
                    item.replace_mappings(
                        item.mappings()
                            .iter()
                            .filter(|mapping| mapping.control_id() != control_id)
                            .cloned()
                            .collect(),
                    )
                    .map_err(|_| {
                        RepositoryError::InvariantViolation("evidence mappings are invalid")
                    })?;
                    repository.save(&item).await?;
                }
                Ok(UnmapOutcome::Unmapped(requested_ids))
            })
            .await?;

        match outcome {
            UnmapOutcome::Unmapped(evidence_ids) => {
                Ok(UnmappedControlFromEvidence { evidence_ids })
            }
            UnmapOutcome::ControlNotFound => Err(UnmapControlFromEvidenceError::ControlNotFound),
            UnmapOutcome::Rejected {
                unknown,
                not_mapped,
            } => Err(UnmapControlFromEvidenceError::Rejected {
                unknown,
                not_mapped,
            }),
        }
    }
}

enum UnmapOutcome {
    Unmapped(Vec<EvidenceId>),
    ControlNotFound,
    Rejected {
        unknown: Vec<EvidenceId>,
        not_mapped: Vec<EvidenceId>,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum UnmapControlFromEvidenceError {
    #[error("control is unavailable")]
    Unavailable,
    #[error("control was not found")]
    ControlNotFound,
    #[error("evidence mappings are invalid")]
    Rejected {
        unknown: Vec<EvidenceId>,
        not_mapped: Vec<EvidenceId>,
    },
    #[error("repository error")]
    Repository(#[from] RepositoryError),
}
