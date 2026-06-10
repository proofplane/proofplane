use std::sync::Arc;

use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

use crate::{
    authorization::workspaces::WorkspaceAuthorizer,
    domain::{
        AddMemberPayload, CreateWorkspacePayload, UserId, WorkspaceId, WorkspaceMembership,
        WorkspaceRole, WorkspaceWithRole,
    },
    pubsub::{TopicName, MESSAGE_BUS_TOPIC},
    repository::{NewOutboxMessage, NewWorkspaceMembership, Postgres},
    services::Error as ServiceError,
    worker::{WORKSPACE_MEMBER_ADDED, WORKSPACE_MEMBER_REMOVED},
};

#[derive(Clone)]
pub struct WorkspaceService {
    repository: Arc<Postgres>,
    authorizer: WorkspaceAuthorizer,
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

/// Self-describing outbox payload identifying a SpiceDB membership tuple. The
/// outbox `event_type` (`workspace.member_added` / `workspace.member_removed`)
/// says whether to write or delete it; this payload says which tuple. The worker
/// rebuilds the exact relationship from these fields without any database
/// lookups, so producers and the reconciliation handler stay in lockstep.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceMembershipTuple {
    pub workspace_id: Uuid,
    pub subject_type: String,
    pub subject_id: String,
    pub relation: String,
}

enum RemoveOutcome {
    Removed(WorkspaceRole),
    NotFound,
    LastOwner,
}

impl WorkspaceService {
    pub fn new(repository: Arc<Postgres>, authorizer: WorkspaceAuthorizer) -> Self {
        Self {
            repository,
            authorizer,
        }
    }

    pub fn authorizer(&self) -> &WorkspaceAuthorizer {
        &self.authorizer
    }

    pub async fn create_owned(
        &self,
        user_id: UserId,
        request_id: Uuid,
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
                context
                    .append_outbox_message(&member_event_message(
                        WORKSPACE_MEMBER_ADDED,
                        workspace.id,
                        Uuid::from(user_id),
                        WorkspaceRole::Owner,
                        request_id,
                    ))
                    .await?;

                Ok(workspace)
            })
            .await?;

        self.write_role_best_effort(workspace.id, user_id, WorkspaceRole::Owner)
            .await;

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

    pub async fn add_member(
        &self,
        workspace_id: WorkspaceId,
        request_id: Uuid,
        payload: AddMemberPayload,
    ) -> Result<WorkspaceMembership, MemberError> {
        if !self.repository.user_exists(payload.user_id).await? {
            return Err(MemberError::TargetUserNotFound);
        }

        let AddMemberPayload { user_id, role } = payload;
        let membership = self
            .repository
            .in_transaction(async move |context| {
                let membership = context
                    .insert_workspace_membership(&NewWorkspaceMembership {
                        user_id,
                        workspace_id,
                        role,
                    })
                    .await?;
                context
                    .append_outbox_message(&member_event_message(
                        WORKSPACE_MEMBER_ADDED,
                        workspace_id,
                        Uuid::from(user_id),
                        role,
                        request_id,
                    ))
                    .await?;

                Ok(membership)
            })
            .await?;

        self.write_role_best_effort(workspace_id, user_id, role)
            .await;

        Ok(membership)
    }

    pub async fn remove_member(
        &self,
        workspace_id: WorkspaceId,
        target_user_id: UserId,
        request_id: Uuid,
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
                context
                    .append_outbox_message(&member_event_message(
                        WORKSPACE_MEMBER_REMOVED,
                        workspace_id,
                        Uuid::from(target_user_id),
                        membership.role,
                        request_id,
                    ))
                    .await?;

                Ok(RemoveOutcome::Removed(membership.role))
            })
            .await?;

        match outcome {
            RemoveOutcome::NotFound => Err(MemberError::NotFound),
            RemoveOutcome::LastOwner => Err(MemberError::LastOwner),
            RemoveOutcome::Removed(role) => {
                self.delete_role_best_effort(workspace_id, target_user_id, role)
                    .await;
                Ok(())
            }
        }
    }

    async fn write_role_best_effort(
        &self,
        workspace_id: WorkspaceId,
        user_id: UserId,
        role: WorkspaceRole,
    ) {
        if let Err(error) = self
            .authorizer
            .write_user_role(workspace_id, &Uuid::from(user_id).to_string(), role)
            .await
        {
            tracing::warn!(
                %error,
                "synchronous SpiceDB membership write failed; the outbox worker will reconcile"
            );
        }
    }

    async fn delete_role_best_effort(
        &self,
        workspace_id: WorkspaceId,
        user_id: UserId,
        role: WorkspaceRole,
    ) {
        if let Err(error) = self
            .authorizer
            .delete_user_role(workspace_id, &Uuid::from(user_id).to_string(), role)
            .await
        {
            tracing::warn!(
                %error,
                "synchronous SpiceDB membership delete failed; the outbox worker will reconcile"
            );
        }
    }
}

fn member_event_message(
    event_type: &str,
    workspace_id: WorkspaceId,
    subject_id: Uuid,
    role: WorkspaceRole,
    request_id: Uuid,
) -> NewOutboxMessage {
    let payload = WorkspaceMembershipTuple {
        workspace_id: Uuid::from(workspace_id),
        subject_type: "user".to_owned(),
        subject_id: subject_id.to_string(),
        relation: role.as_str().to_owned(),
    };

    NewOutboxMessage {
        topic: TopicName::new(MESSAGE_BUS_TOPIC),
        event_type: event_type.to_owned(),
        aggregate_type: "workspace_membership".to_owned(),
        aggregate_id: Uuid::from(workspace_id).to_string(),
        payload: serde_json::to_value(payload).expect("membership tuple payload serializes"),
        request_id: Some(request_id),
    }
}
