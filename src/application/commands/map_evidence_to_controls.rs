use std::sync::Arc;

use chrono::Utc;

use crate::{
    application::ExecutionMetadata,
    authentication::AgentConnectionContext,
    domain::{
        ControlId, EvidenceControlMappingState, EvidenceError, EvidenceId, WorkspacePermission,
    },
    persistence::{Error as RepositoryError, Postgres},
    read_models::EvidenceControlMapping,
};

#[derive(Debug, Clone)]
pub struct MapEvidenceToControls {
    pub connection: AgentConnectionContext,
    pub evidence_id: EvidenceId,
    pub mappings: Vec<EvidenceControlMappingInput>,
}
#[derive(Debug, Clone)]
pub struct EvidenceControlMappingInput {
    pub control_id: ControlId,
    pub rationale: String,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MappedEvidenceToControls {
    pub control_ids: Vec<ControlId>,
    pub mappings: Vec<EvidenceControlMapping>,
}
#[derive(Clone)]
pub struct MapEvidenceToControlsHandler {
    repository: Arc<Postgres>,
}
impl MapEvidenceToControlsHandler {
    pub fn new(repository: Arc<Postgres>) -> Self {
        Self { repository }
    }
    pub async fn handle(
        &self,
        command: MapEvidenceToControls,
        _metadata: ExecutionMetadata,
    ) -> Result<MappedEvidenceToControls, MapEvidenceToControlsError> {
        if !command
            .connection
            .permissions
            .has(WorkspacePermission::WriteControls)
        {
            return Err(MapEvidenceToControlsError::Unavailable);
        }
        let evidence_id = command.evidence_id;
        let requested = command.mappings;
        let result = self
            .repository
            .in_unit_of_work(async move |unit_of_work| {
                let workspace = unit_of_work.workspace(command.connection.workspace_id);
                let repository = workspace.aggregates().evidence();
                let Some(mut evidence) = repository.get(evidence_id).await? else {
                    return Ok(MapOutcome::Unavailable);
                };
                let mut combined = evidence.mappings().to_vec();
                let mut requested_ids = Vec::with_capacity(requested.len());
                for mapping in &requested {
                    requested_ids.push(mapping.control_id);
                }
                let existing_controls = workspace
                    .reads()
                    .controls()
                    .existing_ids(&requested_ids)
                    .await?;
                let unknown = requested_ids
                    .iter()
                    .copied()
                    .filter(|id| !existing_controls.contains(id))
                    .collect::<Vec<_>>();
                let already_mapped = requested_ids
                    .iter()
                    .copied()
                    .filter(|id| {
                        evidence
                            .mappings()
                            .iter()
                            .any(|mapping| mapping.control_id() == *id)
                    })
                    .collect::<Vec<_>>();
                if !unknown.is_empty() || !already_mapped.is_empty() {
                    return Ok(MapOutcome::Rejected {
                        unknown,
                        already_mapped,
                    });
                }
                for mapping in requested {
                    let mapping = EvidenceControlMappingState::new(
                        mapping.control_id,
                        mapping.rationale,
                        Utc::now(),
                    )
                    .into_result()
                    .map_err(|_| {
                        RepositoryError::InvariantViolation("evidence mapping is invalid")
                    })?;
                    combined.push(mapping);
                }
                evidence
                    .replace_mappings(combined)
                    .map_err(|error| match error {
                        EvidenceError::DuplicateControlMapping(_) => {
                            RepositoryError::InvariantViolation(
                                "duplicate evidence control mapping",
                            )
                        }
                        _ => RepositoryError::InvariantViolation("evidence snapshot is invalid"),
                    })?;
                repository.save(&evidence).await?;
                let reads = workspace.reads();
                let control_reads = reads.controls();
                let mut saved = Vec::with_capacity(requested_ids.len());
                for control_id in requested_ids.iter().copied() {
                    saved.push(
                        control_reads
                            .get_evidence_mapping(evidence_id, control_id)
                            .await?
                            .ok_or(RepositoryError::InvariantViolation(
                                "saved evidence mapping must be readable",
                            ))?,
                    );
                }
                Ok(MapOutcome::Mapped {
                    requested_ids,
                    mappings: saved,
                })
            })
            .await?;
        match result {
            MapOutcome::Mapped {
                requested_ids: control_ids,
                mappings,
            } => Ok(MappedEvidenceToControls {
                control_ids,
                mappings,
            }),
            MapOutcome::Unavailable => Err(MapEvidenceToControlsError::EvidenceNotFound),
            MapOutcome::Rejected {
                unknown,
                already_mapped,
            } => Err(MapEvidenceToControlsError::Rejected {
                unknown,
                already_mapped,
            }),
        }
    }
}
enum MapOutcome {
    Mapped {
        requested_ids: Vec<ControlId>,
        mappings: Vec<EvidenceControlMapping>,
    },
    Unavailable,
    Rejected {
        unknown: Vec<ControlId>,
        already_mapped: Vec<ControlId>,
    },
}
#[derive(Debug, thiserror::Error)]
pub enum MapEvidenceToControlsError {
    #[error("evidence is unavailable")]
    Unavailable,
    #[error("evidence was not found")]
    EvidenceNotFound,
    #[error("control mappings are invalid")]
    Rejected {
        unknown: Vec<ControlId>,
        already_mapped: Vec<ControlId>,
    },
    #[error("repository error")]
    Repository(#[from] RepositoryError),
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use uuid::Uuid;

    use crate::{
        application::ExecutionMetadata,
        authentication::AgentConnectionContext,
        domain::{ControlId, WorkspacePermission, WorkspacePermissions},
        persistence::test_support,
    };

    use super::{
        EvidenceControlMappingInput, MapEvidenceToControls, MapEvidenceToControlsError,
        MapEvidenceToControlsHandler,
    };

    #[tokio::test]
    async fn mapping_rejects_foreign_or_unknown_parent_controls_without_partial_save() {
        let database = test_support::database().await;
        let postgres = Arc::new(database.postgres);
        let workspace = test_support::workspace(&postgres, "mapping owner").await;
        let foreign = test_support::workspace(&postgres, "mapping foreign").await;
        let evidence_id =
            test_support::evidence(&postgres, workspace.workspace_id, "Evidence").await;
        let foreign_control_id = Uuid::new_v4();
        let client = postgres.get().await.unwrap();
        client
            .execute(
                "INSERT INTO controls (id, workspace_id, code, title, description) VALUES ($1, $2, 'C1', 'Foreign', 'Description')",
                &[&foreign_control_id, &Uuid::from(foreign.workspace_id)],
            )
            .await
            .unwrap();
        let handler = MapEvidenceToControlsHandler::new(Arc::clone(&postgres));

        let result = handler
            .handle(
                MapEvidenceToControls {
                    connection: connection(&workspace),
                    evidence_id,
                    mappings: vec![EvidenceControlMappingInput {
                        control_id: ControlId::from(foreign_control_id),
                        rationale: "Foreign control".into(),
                    }],
                },
                ExecutionMetadata::background(),
            )
            .await;

        assert!(matches!(
            result,
            Err(MapEvidenceToControlsError::Rejected { unknown, already_mapped })
                if unknown == vec![ControlId::from(foreign_control_id)] && already_mapped.is_empty()
        ));
        let mappings = postgres
            .workspace_reads(workspace.workspace_id)
            .await
            .unwrap()
            .controls()
            .list_evidence_mappings(evidence_id)
            .await
            .unwrap()
            .unwrap();
        assert!(mappings.is_empty());
    }

    fn connection(workspace: &test_support::TestWorkspace) -> AgentConnectionContext {
        AgentConnectionContext {
            user_id: workspace.user_id,
            connection_id: workspace.agent_connection_id,
            workspace_id: workspace.workspace_id,
            permissions: WorkspacePermissions::from_iter([WorkspacePermission::WriteControls]),
        }
    }
}
