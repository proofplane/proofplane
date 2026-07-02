use chrono::{DateTime, Utc};

use super::{ids::uuid_id, AuditorAccessGrantId, WorkspaceId};

uuid_id!(AuditorAccessOtpId);
uuid_id!(AuditorSessionId);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditorAccessOtp {
    pub id: AuditorAccessOtpId,
    pub grant_id: AuditorAccessGrantId,
    pub expires_at: DateTime<Utc>,
    pub consumed_at: Option<DateTime<Utc>>,
    pub failed_attempt_count: i32,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditorSession {
    pub id: AuditorSessionId,
    pub grant_id: AuditorAccessGrantId,
    pub workspace_id: WorkspaceId,
    pub auditor_email: String,
    pub expires_at: DateTime<Utc>,
    pub revoked_at: Option<DateTime<Utc>>,
    pub last_used_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}
