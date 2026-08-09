use std::sync::Arc;

use chrono::{DateTime, Utc};

use crate::{
    application::ExecutionMetadata,
    domain::{
        UserId, Workspace, WorkspaceAggregate, WorkspaceId, WorkspaceRole, WorkspaceWithRole,
    },
    repository::{ConflictKind, Error as RepositoryError, Postgres},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateOwnedWorkspace {
    pub workspace_id: WorkspaceId,
    pub actor_user_id: UserId,
    pub slug: Option<String>,
    pub name: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone)]
pub struct CreateOwnedWorkspaceHandler {
    repository: Arc<Postgres>,
}

impl CreateOwnedWorkspaceHandler {
    pub fn new(repository: Arc<Postgres>) -> Self {
        Self { repository }
    }

    pub async fn handle(
        &self,
        command: CreateOwnedWorkspace,
        _metadata: ExecutionMetadata,
    ) -> Result<WorkspaceWithRole, CreateOwnedWorkspaceError> {
        self.repository
            .in_transaction(async move |context| {
                let repository = context.workspace_aggregates();
                if let Some(existing) = repository.get_for_member(command.actor_user_id).await? {
                    if existing.workspace().id != command.workspace_id {
                        return Err(RepositoryError::Conflict(
                            ConflictKind::WorkspaceMembershipExists,
                        ));
                    }
                    return workspace_for_replayed_command(&existing, &command);
                }
                if let Some(existing) = repository.get(command.workspace_id).await? {
                    return workspace_for_replayed_command(&existing, &command);
                }

                let aggregate = WorkspaceAggregate::create_owned(
                    Workspace {
                        id: command.workspace_id,
                        slug: command.slug,
                        name: command.name,
                        created_at: command.created_at,
                    },
                    command.actor_user_id,
                );
                repository.save(&aggregate).await?;
                Ok(WorkspaceWithRole {
                    workspace: aggregate.workspace().clone(),
                    role: WorkspaceRole::Owner,
                })
            })
            .await
            .map_err(CreateOwnedWorkspaceError::from)
    }
}

fn workspace_for_replayed_command(
    existing: &WorkspaceAggregate,
    command: &CreateOwnedWorkspace,
) -> Result<WorkspaceWithRole, RepositoryError> {
    let workspace = existing.workspace();
    if workspace.slug == command.slug
        && workspace.name == command.name
        && workspace.created_at == command.created_at
        && existing.role_for(command.actor_user_id) == Some(WorkspaceRole::Owner)
    {
        return Ok(WorkspaceWithRole {
            workspace: workspace.clone(),
            role: WorkspaceRole::Owner,
        });
    }
    Err(RepositoryError::InvariantViolation(
        "workspace command replay changed its intent",
    ))
}

#[derive(Debug, thiserror::Error)]
pub enum CreateOwnedWorkspaceError {
    #[error("a workspace with this slug already exists")]
    SlugTaken,
    #[error("the user already belongs to a workspace")]
    UserAlreadyHasWorkspace,
    #[error("workspace repository error")]
    Repository(RepositoryError),
}

impl From<RepositoryError> for CreateOwnedWorkspaceError {
    fn from(error: RepositoryError) -> Self {
        match error {
            RepositoryError::Conflict(ConflictKind::WorkspaceSlugTaken) => Self::SlugTaken,
            RepositoryError::Conflict(ConflictKind::WorkspaceMembershipExists) => {
                Self::UserAlreadyHasWorkspace
            }
            other => Self::Repository(other),
        }
    }
}
