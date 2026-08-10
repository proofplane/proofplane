use std::{collections::HashMap, sync::Arc};

use chrono::Utc;

use crate::{
    application::ExecutionMetadata,
    authentication::AgentConnectionContext,
    domain::{ControlId, Evidence, EvidenceControlMappingState, EvidenceId, WorkspacePermission},
    persistence::{Error as RepositoryError, Postgres},
};

#[derive(Debug, Clone)]
pub struct MapControlToEvidence {
    pub connection: AgentConnectionContext,
    pub control_id: ControlId,
    pub mappings: Vec<ControlEvidenceMapping>,
}

#[derive(Debug, Clone)]
pub struct ControlEvidenceMapping {
    pub evidence_id: EvidenceId,
    pub rationale: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MappedControlToEvidence {
    pub evidence_ids: Vec<EvidenceId>,
}

#[derive(Clone)]
pub struct MapControlToEvidenceHandler {
    repository: Arc<Postgres>,
}

impl MapControlToEvidenceHandler {
    pub fn new(repository: Arc<Postgres>) -> Self {
        Self { repository }
    }

    pub async fn handle(
        &self,
        command: MapControlToEvidence,
        _metadata: ExecutionMetadata,
    ) -> Result<MappedControlToEvidence, MapControlToEvidenceError> {
        if !command
            .connection
            .permissions
            .has(WorkspacePermission::WriteControls)
        {
            return Err(MapControlToEvidenceError::Unavailable);
        }
        let control_id = command.control_id;
        let requested_ids = command
            .mappings
            .iter()
            .map(|mapping| mapping.evidence_id)
            .collect::<Vec<_>>();
        let rationale_by_evidence = command
            .mappings
            .into_iter()
            .map(|mapping| (mapping.evidence_id, mapping.rationale))
            .collect::<HashMap<_, _>>();
        let mut lock_order = requested_ids.clone();
        lock_order.sort_unstable_by_key(|id| uuid::Uuid::from(*id));

        let outcome = self
            .repository
            .in_unit_of_work(async move |unit_of_work| {
                let workspace = unit_of_work.workspace(command.connection.workspace_id);
                if !workspace
                    .reads()
                    .controls()
                    .ids_exist(&[control_id])
                    .await?
                {
                    return Ok(MapOutcome::ControlNotFound);
                }
                let repository = workspace.aggregates().evidence();
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
                let already_mapped = requested_ids
                    .iter()
                    .copied()
                    .filter(|id| {
                        evidence.iter().any(|item| {
                            item.id() == *id
                                && item
                                    .mappings()
                                    .iter()
                                    .any(|mapping| mapping.control_id() == control_id)
                        })
                    })
                    .collect::<Vec<_>>();
                if !unknown.is_empty() || !already_mapped.is_empty() {
                    return Ok(MapOutcome::Rejected {
                        unknown,
                        already_mapped,
                    });
                }
                for mut item in evidence {
                    let rationale = rationale_by_evidence.get(&item.id()).ok_or(
                        RepositoryError::InvariantViolation(
                            "evidence mapping rationale must be present",
                        ),
                    )?;
                    let mut mappings = item.mappings().to_vec();
                    mappings.push(
                        EvidenceControlMappingState::new(control_id, rationale.clone(), Utc::now())
                            .into_result()
                            .map_err(|_| {
                                RepositoryError::InvariantViolation("evidence mapping is invalid")
                            })?,
                    );
                    item.replace_mappings(mappings).map_err(|_| {
                        RepositoryError::InvariantViolation("evidence mappings are invalid")
                    })?;
                    repository.save(&item).await?;
                }
                Ok(MapOutcome::Mapped(requested_ids))
            })
            .await?;

        match outcome {
            MapOutcome::Mapped(evidence_ids) => Ok(MappedControlToEvidence { evidence_ids }),
            MapOutcome::ControlNotFound => Err(MapControlToEvidenceError::ControlNotFound),
            MapOutcome::Rejected {
                unknown,
                already_mapped,
            } => Err(MapControlToEvidenceError::Rejected {
                unknown,
                already_mapped,
            }),
        }
    }
}

enum MapOutcome {
    Mapped(Vec<EvidenceId>),
    ControlNotFound,
    Rejected {
        unknown: Vec<EvidenceId>,
        already_mapped: Vec<EvidenceId>,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum MapControlToEvidenceError {
    #[error("control is unavailable")]
    Unavailable,
    #[error("control was not found")]
    ControlNotFound,
    #[error("evidence mappings are invalid")]
    Rejected {
        unknown: Vec<EvidenceId>,
        already_mapped: Vec<EvidenceId>,
    },
    #[error("repository error")]
    Repository(#[from] RepositoryError),
}
