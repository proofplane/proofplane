use chrono::{DateTime, Utc};

use crate::domain::{
    UserId, WorkspaceId, WorkspaceInvitationDeliveryState, WorkspaceInvitationId, WorkspaceRole,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspacePeople {
    pub workspace_id: WorkspaceId,
    pub workspace_name: String,
    pub actor_role: WorkspaceRole,
    pub members: Vec<WorkspacePerson>,
    pub pending_invitations: Vec<PendingWorkspaceInvitation>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspacePerson {
    pub user_id: UserId,
    pub display_name: Option<String>,
    pub email: Option<String>,
    pub role: WorkspaceRole,
    pub joined_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingWorkspaceInvitation {
    pub id: WorkspaceInvitationId,
    pub invited_email: String,
    pub role: WorkspaceRole,
    pub generation: i64,
    pub expires_at: DateTime<Utc>,
    pub delivery_state: WorkspaceInvitationDeliveryState,
    pub queued_at: Option<DateTime<Utc>>,
    pub delivered_at: Option<DateTime<Utc>>,
    pub delivery_failed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurrentWorkspaceInvitation {
    pub id: WorkspaceInvitationId,
    pub workspace_id: WorkspaceId,
    pub invited_email: String,
    pub role: WorkspaceRole,
    pub generation: i64,
    pub expires_at: DateTime<Utc>,
    pub delivery_state: WorkspaceInvitationDeliveryState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceInvitationMetadata {
    pub id: WorkspaceInvitationId,
    pub invited_email: String,
    pub role: WorkspaceRole,
    pub generation: i64,
    pub expires_at: DateTime<Utc>,
    pub delivery_state: WorkspaceInvitationDeliveryState,
}

impl From<&CurrentWorkspaceInvitation> for WorkspaceInvitationMetadata {
    fn from(value: &CurrentWorkspaceInvitation) -> Self {
        Self {
            id: value.id,
            invited_email: value.invited_email.clone(),
            role: value.role,
            generation: value.generation,
            expires_at: value.expires_at,
            delivery_state: value.delivery_state,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceInvitationPreviewSource {
    pub invitation_id: WorkspaceInvitationId,
    pub workspace_id: WorkspaceId,
    pub workspace_name: String,
    pub invited_email: String,
    pub role: WorkspaceRole,
    pub generation: i64,
    pub expires_at: DateTime<Utc>,
    pub accepted_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
}
