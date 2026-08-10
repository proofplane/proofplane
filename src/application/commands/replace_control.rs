use std::sync::Arc;

use chrono::Utc;

use crate::{
    application::ExecutionMetadata,
    authentication::AgentConnectionContext,
    domain::{
        ControlDefinition, ControlError, ControlId, DomainError, FrameworkRequirementId,
        WorkspacePermission,
    },
    persistence::{ConflictKind, Error as RepositoryError, Postgres},
    read_models::ControlDetail,
};

#[derive(Debug, Clone)]
pub struct ReplaceControl {
    pub connection: AgentConnectionContext,
    pub control_id: ControlId,
    pub code: String,
    pub title: String,
    pub description: String,
    pub framework_requirement_ids: Vec<FrameworkRequirementId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplacedControl {
    pub control: ControlDetail,
}

#[derive(Clone)]
pub struct ReplaceControlHandler {
    repository: Arc<Postgres>,
}

impl ReplaceControlHandler {
    pub fn new(repository: Arc<Postgres>) -> Self {
        Self { repository }
    }

    pub async fn handle(
        &self,
        command: ReplaceControl,
        _metadata: ExecutionMetadata,
    ) -> Result<ReplacedControl, ReplaceControlError> {
        if !command
            .connection
            .permissions
            .has(WorkspacePermission::WriteControls)
        {
            return Err(ReplaceControlError::Unavailable);
        }
        let definition = ControlDefinition::new(command.code, command.title, command.description)
            .into_result()
            .map_err(ReplaceControlError::InvalidDefinition)?;
        let control_id = command.control_id;
        let requirement_ids = command.framework_requirement_ids;

        let outcome = self
            .repository
            .in_unit_of_work(async move |unit_of_work| {
                let workspace = unit_of_work.workspace(command.connection.workspace_id);
                if !workspace
                    .reads()
                    .frameworks()
                    .requirements_exist(&requirement_ids)
                    .await?
                {
                    return Ok(ReplaceOutcome::InvalidFrameworkRequirementReferences);
                }
                let repository = workspace.aggregates().controls();
                let Some(mut control) = repository.get(control_id).await? else {
                    return Ok(ReplaceOutcome::Unavailable);
                };
                match control.replace(definition, requirement_ids, Utc::now()) {
                    Ok(()) => {}
                    Err(ControlError::DuplicateFrameworkRequirementReference(_)) => {
                        return Ok(ReplaceOutcome::InvalidFrameworkRequirementReferences);
                    }
                    Err(
                        ControlError::InvalidRehydration | ControlError::InvalidReplacementTime,
                    ) => {
                        return Err(RepositoryError::InvariantViolation(
                            "replacement control snapshot is invalid",
                        ));
                    }
                }
                repository.save(&control).await?;
                let read_model = workspace.reads().controls().get(control_id).await?.ok_or(
                    RepositoryError::InvariantViolation(
                        "replaced control must be readable in its transaction",
                    ),
                )?;
                Ok(ReplaceOutcome::Replaced(read_model))
            })
            .await
            .map_err(ReplaceControlError::from)?;

        match outcome {
            ReplaceOutcome::Replaced(control) => Ok(ReplacedControl { control }),
            ReplaceOutcome::Unavailable => Err(ReplaceControlError::Unavailable),
            ReplaceOutcome::InvalidFrameworkRequirementReferences => {
                Err(ReplaceControlError::InvalidFrameworkRequirementReferences)
            }
        }
    }
}

enum ReplaceOutcome {
    Replaced(ControlDetail),
    Unavailable,
    InvalidFrameworkRequirementReferences,
}

#[derive(Debug, thiserror::Error)]
pub enum ReplaceControlError {
    #[error("control is unavailable")]
    Unavailable,
    #[error("control definition is invalid")]
    InvalidDefinition(Vec<DomainError>),
    #[error("framework requirement references are invalid")]
    InvalidFrameworkRequirementReferences,
    #[error("a control with this code already exists in the workspace")]
    CodeTaken,
    #[error("repository error")]
    Repository(RepositoryError),
}

impl From<RepositoryError> for ReplaceControlError {
    fn from(error: RepositoryError) -> Self {
        match error {
            RepositoryError::Conflict(ConflictKind::ControlCodeTaken) => Self::CodeTaken,
            other => Self::Repository(other),
        }
    }
}
