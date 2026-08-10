use std::sync::Arc;

use chrono::{DateTime, Utc};

use crate::{
    application::ExecutionMetadata,
    domain::{UserId, Workspace, WorkspaceId, WorkspaceRole},
    persistence::{ConflictKind, Error as RepositoryError, Postgres},
    read_models::{WorkspaceDetails, WorkspaceWithRole},
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
            .in_unit_of_work(async move |unit_of_work| {
                let existing_workspace_id = unit_of_work
                    .reads()
                    .workspaces()
                    .resolve_id_for_member(command.actor_user_id)
                    .await?;
                if let Some(existing_workspace_id) = existing_workspace_id {
                    let existing = unit_of_work
                        .reads()
                        .workspaces()
                        .get(existing_workspace_id)
                        .await?
                        .ok_or(RepositoryError::InvariantViolation(
                            "workspace membership must reference an existing workspace",
                        ))?;
                    if existing.id != command.workspace_id {
                        return Err(RepositoryError::Conflict(
                            ConflictKind::WorkspaceMembershipExists,
                        ));
                    }
                    let role = unit_of_work
                        .reads()
                        .workspaces()
                        .role_for(existing.id, command.actor_user_id)
                        .await?;
                    return workspace_for_replayed_command(existing, role, &command);
                }
                if let Some(existing) = unit_of_work
                    .reads()
                    .workspaces()
                    .get(command.workspace_id)
                    .await?
                {
                    let role = unit_of_work
                        .reads()
                        .workspaces()
                        .role_for(existing.id, command.actor_user_id)
                        .await?;
                    return workspace_for_replayed_command(existing, role, &command);
                }

                let workspace = Workspace::create_owned(
                    command.workspace_id,
                    command.slug,
                    command.name,
                    command.created_at,
                    command.actor_user_id,
                );
                unit_of_work
                    .aggregates()
                    .workspaces()
                    .save(&workspace)
                    .await?;
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
    existing: WorkspaceDetails,
    role: Option<WorkspaceRole>,
    command: &CreateOwnedWorkspace,
) -> Result<WorkspaceWithRole, RepositoryError> {
    if existing.slug == command.slug
        && existing.name == command.name
        && existing.created_at == command.created_at
        && role == Some(WorkspaceRole::Owner)
    {
        return Ok(WorkspaceWithRole {
            workspace: existing,
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
