use chrono::{DateTime, Utc};

use crate::domain::{AuditReviewPeriod, AuditorAccessGrantId};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditorAccessGrantSummary {
    pub id: AuditorAccessGrantId,
    pub auditor_email: String,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub period: AuditReviewPeriod,
    pub revoked_at: Option<DateTime<Utc>>,
}
