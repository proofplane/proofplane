use chrono::{DateTime, Utc};

use super::{ids::uuid_id, AuditReviewPeriod, AuditorAccessGrantId, WorkspaceId};

uuid_id!(AuditorSessionId);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditorSession {
    pub id: AuditorSessionId,
    pub grant_id: AuditorAccessGrantId,
    pub workspace_id: WorkspaceId,
    pub auditor_email: String,
    pub auth0_subject: String,
    pub expires_at: DateTime<Utc>,
    pub period: AuditReviewPeriod,
    pub revoked_at: Option<DateTime<Utc>>,
    pub last_used_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}
