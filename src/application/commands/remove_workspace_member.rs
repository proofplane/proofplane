use std::sync::Arc;

use crate::{
    application::ExecutionMetadata,
    domain::{UserId, WorkspaceId, WorkspaceMemberError},
    repository::Postgres,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RemoveWorkspaceMember {
    pub actor_user_id: UserId,
    pub target_user_id: UserId,
}

#[derive(Clone)]
pub struct RemoveWorkspaceMemberHandler {
    repository: Arc<Postgres>,
}

impl RemoveWorkspaceMemberHandler {
    pub fn new(repository: Arc<Postgres>) -> Self {
        Self { repository }
    }

    pub async fn handle(
        &self,
        command: RemoveWorkspaceMember,
        _metadata: ExecutionMetadata,
    ) -> Result<WorkspaceId, RemoveWorkspaceMemberError> {
        let outcome = self
            .repository
            .in_unit_of_work(async move |context| {
                let repository = context.workspaces();
                let Some(mut aggregate) = repository.get_for_member(command.actor_user_id).await?
                else {
                    return Ok(RemoveOutcome::Unavailable);
                };
                let workspace_id = aggregate.id();
                match aggregate.remove_member(command.actor_user_id, command.target_user_id) {
                    Ok(()) => {
                        repository.save(&aggregate).await?;
                        Ok(RemoveOutcome::Removed(workspace_id))
                    }
                    Err(WorkspaceMemberError::Unavailable) => Ok(RemoveOutcome::Unavailable),
                    Err(WorkspaceMemberError::NotFound) => Ok(RemoveOutcome::NotFound),
                    Err(WorkspaceMemberError::LastOwner) => Ok(RemoveOutcome::LastOwner),
                }
            })
            .await?;

        match outcome {
            RemoveOutcome::Removed(workspace_id) => Ok(workspace_id),
            RemoveOutcome::Unavailable => Err(RemoveWorkspaceMemberError::Unavailable),
            RemoveOutcome::NotFound => Err(RemoveWorkspaceMemberError::NotFound),
            RemoveOutcome::LastOwner => Err(RemoveWorkspaceMemberError::LastOwner),
        }
    }
}

enum RemoveOutcome {
    Removed(WorkspaceId),
    Unavailable,
    NotFound,
    LastOwner,
}

#[derive(Debug, thiserror::Error)]
pub enum RemoveWorkspaceMemberError {
    #[error("the actor may not manage workspace members")]
    Unavailable,
    #[error("workspace membership not found")]
    NotFound,
    #[error("the workspace must retain at least one owner")]
    LastOwner,
    #[error("workspace repository error")]
    Repository(#[from] crate::repository::Error),
}
