use std::sync::Arc;

use thiserror::Error;

use crate::{
    domain::{CreateWorkspacePayload, UserId, WorkspaceId, WorkspaceRole, WorkspaceWithRole},
    repository::{ConflictKind, Error as RepositoryError, NewWorkspaceMembership, Postgres},
    services::Error as ServiceError,
};

/// Human management-plane operations on workspaces. Authorization is answered
/// from Postgres (`workspace_memberships`), the transactional source of truth for
/// human roles.
#[derive(Clone)]
pub struct WorkspaceService {
    repository: Arc<Postgres>,
}

#[derive(Debug, Error)]
pub enum MemberError {
    #[error("the actor may not manage workspace members")]
    Forbidden,

    #[error("workspace membership not found")]
    NotFound,

    #[error("the workspace must retain at least one owner")]
    LastOwner,

    #[error("repository error")]
    Repository(#[from] crate::repository::Error),
}

#[derive(Debug, Error)]
pub enum CreateWorkspaceError {
    #[error("a workspace with this slug already exists")]
    SlugTaken,

    #[error("the user already belongs to a workspace")]
    UserAlreadyHasWorkspace,

    #[error("repository error")]
    Repository(RepositoryError),
}

impl From<RepositoryError> for CreateWorkspaceError {
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

enum RemoveOutcome {
    Removed,
    NotFound,
    LastOwner,
}

impl WorkspaceService {
    pub fn new(repository: Arc<Postgres>) -> Self {
        Self { repository }
    }

    pub async fn create_owned(
        &self,
        user_id: UserId,
        payload: CreateWorkspacePayload,
    ) -> Result<WorkspaceWithRole, CreateWorkspaceError> {
        let workspace = self
            .repository
            .in_transaction(async move |context| {
                let workspace = context.create_workspace(&payload).await?;
                context
                    .insert_workspace_membership(&NewWorkspaceMembership {
                        user_id,
                        workspace_id: workspace.id,
                        role: WorkspaceRole::Owner,
                    })
                    .await?;

                Ok(workspace)
            })
            .await?;

        Ok(WorkspaceWithRole {
            workspace,
            role: WorkspaceRole::Owner,
        })
    }

    pub async fn get_for_user(
        &self,
        user_id: UserId,
    ) -> Result<Option<WorkspaceWithRole>, ServiceError> {
        Ok(self
            .repository
            .get_workspace_with_role_for_user(user_id)
            .await?)
    }

    /// Reads the actor's role and defers the decision to `WorkspaceMemberPolicy`.
    /// A non-member, an unknown workspace, and an under-privileged member all
    /// yield `Forbidden`, which the API layer maps to 404 so none is
    /// distinguishable — no existence is leaked.
    async fn authorize_member_management(
        &self,
        workspace_id: WorkspaceId,
        actor_user_id: UserId,
    ) -> Result<(), MemberError> {
        let role = self
            .repository
            .get_membership_role(workspace_id, actor_user_id)
            .await?;

        if WorkspaceMemberPolicy::can_manage_members(role) {
            Ok(())
        } else {
            Err(MemberError::Forbidden)
        }
    }

    pub async fn remove_member(
        &self,
        actor_user_id: UserId,
        target_user_id: UserId,
    ) -> Result<WorkspaceId, MemberError> {
        let workspace_id = self
            .repository
            .get_workspace_with_role_for_user(actor_user_id)
            .await?
            .ok_or(MemberError::Forbidden)?
            .workspace
            .id;
        self.authorize_member_management(workspace_id, actor_user_id)
            .await?;

        let outcome = self
            .repository
            .in_transaction(async move |context| {
                let membership = match context.get_membership(workspace_id, target_user_id).await? {
                    Some(membership) => membership,
                    None => return Ok(RemoveOutcome::NotFound),
                };

                if membership.role == WorkspaceRole::Owner
                    && context.count_workspace_owners(workspace_id).await? <= 1
                {
                    return Ok(RemoveOutcome::LastOwner);
                }

                context
                    .delete_workspace_membership(workspace_id, target_user_id)
                    .await?;

                Ok(RemoveOutcome::Removed)
            })
            .await?;

        match outcome {
            RemoveOutcome::NotFound => Err(MemberError::NotFound),
            RemoveOutcome::LastOwner => Err(MemberError::LastOwner),
            RemoveOutcome::Removed => Ok(workspace_id),
        }
    }
}

/// Authorization decisions for workspace member management, expressed as pure
/// functions over the actor's role. Keeping the rules here gives membership
/// policy one home as more checks are added, rather than scattering role
/// matching across routes and services.
pub struct WorkspaceMemberPolicy;

impl WorkspaceMemberPolicy {
    /// Owners and admins may add and remove members. A non-member (no role) may
    /// not.
    pub fn can_manage_members(role: Option<WorkspaceRole>) -> bool {
        matches!(role, Some(WorkspaceRole::Owner | WorkspaceRole::Admin))
    }

    /// Owners and admins may create actors and issue or revoke their keys.
    pub fn can_manage_actors(role: Option<WorkspaceRole>) -> bool {
        matches!(role, Some(WorkspaceRole::Owner | WorkspaceRole::Admin))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owners_and_admins_can_manage_members() {
        assert!(WorkspaceMemberPolicy::can_manage_members(Some(
            WorkspaceRole::Owner
        )));
        assert!(WorkspaceMemberPolicy::can_manage_members(Some(
            WorkspaceRole::Admin
        )));
    }

    #[test]
    fn non_members_cannot_manage_members() {
        assert!(!WorkspaceMemberPolicy::can_manage_members(None));
    }
}
