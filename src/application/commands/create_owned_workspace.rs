use std::sync::Arc;

use chrono::{DateTime, Utc};

use crate::{
    application::ExecutionMetadata,
    domain::{UserId, Workspace, WorkspaceId, WorkspaceRole},
    projections::{WorkspaceDetails, WorkspaceWithRole},
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
                let repository = context.workspaces();
                if let Some(existing) = repository.get_for_member(command.actor_user_id).await? {
                    if existing.id() != command.workspace_id {
                        return Err(RepositoryError::Conflict(
                            ConflictKind::WorkspaceMembershipExists,
                        ));
                    }
                    return workspace_for_replayed_command(&existing, &command);
                }
                if let Some(existing) = repository.get(command.workspace_id).await? {
                    return workspace_for_replayed_command(&existing, &command);
                }

                let workspace = Workspace::create_owned(
                    command.workspace_id,
                    command.slug,
                    command.name,
                    command.created_at,
                    command.actor_user_id,
                );
                repository.save(&workspace).await?;
                Ok(WorkspaceWithRole {
                    workspace: WorkspaceDetails {
                        id: workspace.id(),
                        slug: workspace.slug().map(str::to_owned),
                        name: workspace.name().to_owned(),
                        created_at: workspace.created_at(),
                    },
                    role: WorkspaceRole::Owner,
                })
            })
            .await
            .map_err(CreateOwnedWorkspaceError::from)
    }
}

fn workspace_for_replayed_command(
    existing: &Workspace,
    command: &CreateOwnedWorkspace,
) -> Result<WorkspaceWithRole, RepositoryError> {
    if existing.slug() == command.slug.as_deref()
        && existing.name() == command.name
        && existing.created_at() == command.created_at
        && existing.role_for(command.actor_user_id) == Some(WorkspaceRole::Owner)
    {
        return Ok(WorkspaceWithRole {
            workspace: WorkspaceDetails {
                id: existing.id(),
                slug: existing.slug().map(str::to_owned),
                name: existing.name().to_owned(),
                created_at: existing.created_at(),
            },
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
