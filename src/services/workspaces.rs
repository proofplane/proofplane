use std::sync::Arc;

use thiserror::Error;

use crate::{
    domain::{
        AddMemberPayload, CreateWorkspacePayload, UserId, WorkspaceId, WorkspaceMembership,
        WorkspaceRole, WorkspaceWithRole,
    },
    repository::{NewWorkspaceMembership, Postgres},
    services::Error as ServiceError,
};

/// Human management-plane operations on workspaces. Authorization for the human
/// plane is answered from Postgres (`workspace_memberships`), which is the
/// transactional source of truth — no SpiceDB projection is involved. SpiceDB
/// stays the engine for the actor data plane only.
#[derive(Clone)]
pub struct WorkspaceService {
    repository: Arc<Postgres>,
}

#[derive(Debug, Error)]
pub enum MemberError {
    #[error("workspace membership not found")]
    NotFound,

    #[error("the target user has never authenticated")]
    TargetUserNotFound,

    #[error("the workspace must retain at least one owner")]
    LastOwner,

    #[error("repository error")]
    Repository(#[from] crate::repository::Error),
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
    ) -> Result<WorkspaceWithRole, ServiceError> {
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

    pub async fn list_for_user(
        &self,
        user_id: UserId,
    ) -> Result<Vec<WorkspaceWithRole>, ServiceError> {
        Ok(self
            .repository
            .list_workspaces_with_role_for_user(user_id)
            .await?)
    }

    /// A caller may manage members when they are an `owner` or `admin` of the
    /// workspace. Returns `false` for non-members and unknown workspaces alike,
    /// which the route maps to 404 so neither is distinguishable.
    pub async fn can_manage_members(
        &self,
        workspace_id: WorkspaceId,
        user_id: UserId,
    ) -> Result<bool, ServiceError> {
        Ok(matches!(
            self.repository
                .get_membership_role(workspace_id, user_id)
                .await?,
            Some(WorkspaceRole::Owner | WorkspaceRole::Admin)
        ))
    }

    pub async fn add_member(
        &self,
        workspace_id: WorkspaceId,
        payload: AddMemberPayload,
    ) -> Result<WorkspaceMembership, MemberError> {
        if !self.repository.user_exists(payload.user_id).await? {
            return Err(MemberError::TargetUserNotFound);
        }

        let AddMemberPayload { user_id, role } = payload;
        Ok(self
            .repository
            .in_transaction(async move |context| {
                context
                    .insert_workspace_membership(&NewWorkspaceMembership {
                        user_id,
                        workspace_id,
                        role,
                    })
                    .await
            })
            .await?)
    }

    pub async fn remove_member(
        &self,
        workspace_id: WorkspaceId,
        target_user_id: UserId,
    ) -> Result<(), MemberError> {
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
            RemoveOutcome::Removed => Ok(()),
        }
    }
}
