use chrono::{DateTime, Utc};

use crate::authentication::opaque_token::AuditorInviteSecretDigest;

use super::{ids::uuid_id, AgentConnectionId, UserId, WorkspaceId};

uuid_id!(AuditorAccessGrantId);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditorAccessGrant {
    pub id: AuditorAccessGrantId,
    pub workspace_id: WorkspaceId,
    pub auditor_email: String,
    pub created_by_user_id: UserId,
    pub created_via_agent_connection_id: AgentConnectionId,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub period_start: DateTime<Utc>,
    pub period_end: DateTime<Utc>,
    pub revoked_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateAuditorAccessGrantPayload {
    pub id: AuditorAccessGrantId,
    pub secret_digest: AuditorInviteSecretDigest,
    pub auditor_email: String,
    pub expires_at: DateTime<Utc>,
    pub period_start: DateTime<Utc>,
    pub period_end: DateTime<Utc>,
}
