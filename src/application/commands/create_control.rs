use std::sync::Arc;

use chrono::Utc;
use uuid::Uuid;

use crate::{
    application::ExecutionMetadata,
    authentication::AgentConnectionContext,
    domain::{
        Control, ControlDefinition, ControlId, DomainError, FrameworkRequirementId,
        WorkspacePermission,
    },
    projections::ControlDetail,
    repository::{ConflictKind, Error as RepositoryError, Postgres},
};

#[derive(Debug, Clone)]
pub struct CreateControl {
    pub connection: AgentConnectionContext,
    pub code: String,
    pub title: String,
    pub description: String,
    pub framework_requirement_ids: Vec<FrameworkRequirementId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreatedControl {
    pub control: ControlDetail,
}

#[derive(Clone)]
pub struct CreateControlHandler {
    repository: Arc<Postgres>,
}

impl CreateControlHandler {
    pub fn new(repository: Arc<Postgres>) -> Self {
        Self { repository }
    }

    pub async fn handle(
        &self,
        command: CreateControl,
        _metadata: ExecutionMetadata,
    ) -> Result<CreatedControl, CreateControlError> {
        if !command
            .connection
            .permissions
            .has(WorkspacePermission::WriteControls)
        {
            return Err(CreateControlError::Unavailable);
        }
        let definition = ControlDefinition::new(command.code, command.title, command.description)
            .into_result()
            .map_err(CreateControlError::InvalidDefinition)?;
        let control_id = ControlId::from(Uuid::new_v4());
        let control = Control::define(
            control_id,
            command.connection.workspace_id,
            definition,
            command.framework_requirement_ids,
            Utc::now(),
        )
        .map_err(|_| CreateControlError::InvalidFrameworkRequirementReferences)?;

        let outcome = self
            .repository
            .in_unit_of_work(async move |unit_of_work| {
                let workspace = unit_of_work.for_workspace(command.connection.workspace_id);
                if !workspace
                    .framework_requirements_exist(control.framework_requirement_ids())
                    .await?
                {
                    return Ok(CreateOutcome::InvalidFrameworkRequirementReferences);
                }
                workspace.controls().save(&control).await?;
                let projection = workspace
                    .control_projections()
                    .get(control_id)
                    .await?
                    .ok_or(RepositoryError::InvariantViolation(
                        "created control must be readable in its transaction",
                    ))?;
                Ok(CreateOutcome::Created(projection))
            })
            .await
            .map_err(CreateControlError::from)?;

        match outcome {
            CreateOutcome::Created(control) => Ok(CreatedControl { control }),
            CreateOutcome::InvalidFrameworkRequirementReferences => {
                Err(CreateControlError::InvalidFrameworkRequirementReferences)
            }
        }
    }
}

enum CreateOutcome {
    Created(ControlDetail),
    InvalidFrameworkRequirementReferences,
}

#[derive(Debug, thiserror::Error)]
pub enum CreateControlError {
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

impl From<RepositoryError> for CreateControlError {
    fn from(error: RepositoryError) -> Self {
        match error {
            RepositoryError::Conflict(ConflictKind::ControlCodeTaken) => Self::CodeTaken,
            other => Self::Repository(other),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use deadpool_postgres::{Manager, Pool};
    use tokio_postgres::{Config, NoTls};
    use uuid::Uuid;

    use crate::{
        application::ExecutionMetadata,
        authentication::AgentConnectionContext,
        domain::{AgentConnectionId, UserId, WorkspaceId, WorkspacePermissions},
        repository::Postgres,
    };

    use super::{CreateControl, CreateControlError, CreateControlHandler};

    #[tokio::test]
    async fn create_conceals_invalid_input_without_write_controls_permission() {
        let result = handler()
            .handle(
                CreateControl {
                    connection: connection(),
                    code: String::new(),
                    title: String::new(),
                    description: String::new(),
                    framework_requirement_ids: Vec::new(),
                },
                ExecutionMetadata::background(),
            )
            .await;

        assert!(matches!(result, Err(CreateControlError::Unavailable)));
    }

    fn handler() -> CreateControlHandler {
        let pool = Pool::builder(Manager::new(Config::new(), NoTls))
            .build()
            .unwrap();
        CreateControlHandler::new(Arc::new(Postgres::new(pool)))
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
