use chrono::{DateTime, Utc};

use crate::domain::{UserId, WorkspaceId, WorkspaceInvitationId, WorkspaceRole};

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
    pub queued_generation: Option<i64>,
    pub queued_at: Option<DateTime<Utc>>,
    pub delivered_generation: Option<i64>,
    pub delivered_at: Option<DateTime<Utc>>,
    pub last_delivery_failure: Option<String>,
    pub delivery_failed_at: Option<DateTime<Utc>>,
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
