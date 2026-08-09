use std::sync::Arc;

use chrono::Utc;

use crate::{
    application::ExecutionMetadata,
    authentication::AgentConnectionContext,
    domain::{
        ControlId, EvidenceAggregateError, EvidenceControlMappingState, EvidenceId,
        WorkspacePermission,
    },
    repository::{Error as RepositoryError, Postgres},
};

#[derive(Debug, Clone)]
pub struct MapEvidenceToControls {
    pub connection: AgentConnectionContext,
    pub evidence_id: EvidenceId,
    pub mappings: Vec<EvidenceControlMapping>,
}
#[derive(Debug, Clone)]
pub struct EvidenceControlMapping {
    pub control_id: ControlId,
    pub rationale: String,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MappedEvidenceToControls {
    pub control_ids: Vec<ControlId>,
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
            .in_agent_connection_workspace_context(
                command.connection.workspace_id,
                command.connection.user_id,
                command.connection.connection_id,
                async move |context| {
                    let repository = context.evidence();
                    let Some(mut evidence) = repository.get(evidence_id).await? else {
                        return Ok(MapOutcome::Unavailable);
                    };
                    let mut combined = evidence.mappings().to_vec();
                    let mut already_mapped = Vec::new();
                    let mut requested_ids = Vec::with_capacity(requested.len());
                    for mapping in requested {
                        requested_ids.push(mapping.control_id);
                        if combined
                            .iter()
                            .any(|existing| existing.control_id() == mapping.control_id)
                        {
                            already_mapped.push(mapping.control_id);
                            continue;
                        }
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
                    if !already_mapped.is_empty() || !context.controls_exist(&requested_ids).await?
                    {
                        return Ok(MapOutcome::Rejected);
                    }
                    evidence
                        .replace_mappings(combined)
                        .map_err(|error| match error {
                            EvidenceAggregateError::DuplicateControlMapping(_) => {
                                RepositoryError::InvariantViolation(
                                    "duplicate evidence control mapping",
                                )
                            }
                            _ => {
                                RepositoryError::InvariantViolation("evidence snapshot is invalid")
                            }
                        })?;
                    repository.save(&evidence).await?;
                    Ok(MapOutcome::Mapped(requested_ids))
                },
            )
            .await?;
        match result {
            MapOutcome::Mapped(control_ids) => Ok(MappedEvidenceToControls { control_ids }),
            MapOutcome::Unavailable => Err(MapEvidenceToControlsError::Unavailable),
            MapOutcome::Rejected => Err(MapEvidenceToControlsError::Rejected),
        }
    }
}
enum MapOutcome {
    Mapped(Vec<ControlId>),
    Unavailable,
    Rejected,
}
#[derive(Debug, thiserror::Error)]
pub enum MapEvidenceToControlsError {
    #[error("evidence is unavailable")]
    Unavailable,
    #[error("control mappings are invalid")]
    Rejected,
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
        repository::test_support,
    };

    use super::{
        EvidenceControlMapping, MapEvidenceToControls, MapEvidenceToControlsError,
        MapEvidenceToControlsHandler,
    };

    #[tokio::test]
    async fn mapping_rejects_foreign_or_unknown_parent_controls_without_partial_save() {
        let postgres = Arc::new(test_support::database().await);
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
                    mappings: vec![EvidenceControlMapping {
                        control_id: ControlId::from(foreign_control_id),
                        rationale: "Foreign control".into(),
                    }],
                },
                ExecutionMetadata::background(),
            )
            .await;

        assert!(matches!(result, Err(MapEvidenceToControlsError::Rejected)));
        let mappings = postgres
            .in_workspace_context_read(workspace.workspace_id, async |context| {
                context.list_evidence_control_mappings(evidence_id).await
            })
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
