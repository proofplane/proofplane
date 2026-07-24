use chrono::{DateTime, Utc};

use crate::authentication::opaque_token::AuditorInviteSecretDigest;

use super::{ids::uuid_id, AgentConnectionId, DomainError, UserId, WorkspaceId};

uuid_id!(AuditorAccessGrantId);

/// The window of evidence coverage an auditor grant exposes. Owns its ordering
/// invariant so no caller can construct an inverted period, mirroring
/// `CoverageWindow`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuditReviewPeriod {
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
}

impl AuditReviewPeriod {
    pub fn new(start: DateTime<Utc>, end: DateTime<Utc>) -> Result<Self, DomainError> {
        if end < start {
            return Err(DomainError::InvalidAuditReviewPeriod);
        }

        Ok(Self { start, end })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditorAccessGrant {
    pub id: AuditorAccessGrantId,
    pub workspace_id: WorkspaceId,
    pub auditor_email: String,
    pub created_by_user_id: UserId,
    pub created_via_agent_connection_id: AgentConnectionId,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub period: AuditReviewPeriod,
    pub revoked_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateAuditorAccessGrantPayload {
    pub id: AuditorAccessGrantId,
    pub secret_digest: AuditorInviteSecretDigest,
    pub auditor_email: String,
    pub expires_at: DateTime<Utc>,
    pub period: AuditReviewPeriod,
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};

    use super::AuditReviewPeriod;
    use crate::domain::DomainError;

    #[test]
    fn audit_review_period_accepts_ordered_and_instant_periods() {
        let start = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        let end = Utc.with_ymd_and_hms(2026, 3, 31, 0, 0, 0).unwrap();

        assert_eq!(AuditReviewPeriod::new(start, end).unwrap().end, end);
        assert_eq!(AuditReviewPeriod::new(start, start).unwrap().start, start);
    }

    #[test]
    fn audit_review_period_rejects_end_before_start() {
        let start = Utc.with_ymd_and_hms(2026, 3, 31, 0, 0, 0).unwrap();
        let end = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();

        assert_eq!(
            AuditReviewPeriod::new(start, end).unwrap_err(),
            DomainError::InvalidAuditReviewPeriod
        );
    }
}
