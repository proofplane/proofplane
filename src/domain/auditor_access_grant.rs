use chrono::{DateTime, Utc};

use super::{ids::uuid_id, AgentConnectionId, DomainError, Sha256Digest, UserId, WorkspaceId};

uuid_id!(AuditorAccessGrantId);

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
    secret_digest: Sha256Digest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuditorAccessGrantRevocation {
    Revoked,
    AlreadyRevoked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum AuditorAccessGrantLifecycleError {
    #[error("auditor access grant issuance is invalid")]
    InvalidIssuance,
    #[error("persisted auditor access grant is inconsistent")]
    InvalidRehydration,
    #[error("auditor access grant is unavailable")]
    Unavailable,
    #[error("auditor access grant revocation is invalid")]
    InvalidRevocation,
}

impl AuditorAccessGrant {
    #[allow(clippy::too_many_arguments)]
    pub fn issue(
        id: AuditorAccessGrantId,
        workspace_id: WorkspaceId,
        auditor_email: String,
        secret_digest: Sha256Digest,
        created_by_user_id: UserId,
        created_via_agent_connection_id: AgentConnectionId,
        created_at: DateTime<Utc>,
        expires_at: DateTime<Utc>,
        period: AuditReviewPeriod,
    ) -> Result<Self, AuditorAccessGrantLifecycleError> {
        if expires_at <= created_at || !valid_normalized_email(&auditor_email) {
            return Err(AuditorAccessGrantLifecycleError::InvalidIssuance);
        }

        Ok(Self {
            id,
            workspace_id,
            auditor_email,
            created_by_user_id,
            created_via_agent_connection_id,
            created_at,
            expires_at,
            period,
            revoked_at: None,
            secret_digest,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn rehydrate(
        id: AuditorAccessGrantId,
        workspace_id: WorkspaceId,
        auditor_email: String,
        secret_digest: Sha256Digest,
        created_by_user_id: UserId,
        created_via_agent_connection_id: AgentConnectionId,
        created_at: DateTime<Utc>,
        expires_at: DateTime<Utc>,
        period: AuditReviewPeriod,
        revoked_at: Option<DateTime<Utc>>,
    ) -> Result<Self, AuditorAccessGrantLifecycleError> {
        let mut grant = Self::issue(
            id,
            workspace_id,
            auditor_email,
            secret_digest,
            created_by_user_id,
            created_via_agent_connection_id,
            created_at,
            expires_at,
            period,
        )
        .map_err(|_| AuditorAccessGrantLifecycleError::InvalidRehydration)?;
        if revoked_at.is_some_and(|revoked_at| revoked_at < created_at) {
            return Err(AuditorAccessGrantLifecycleError::InvalidRehydration);
        }
        grant.revoked_at = revoked_at;
        Ok(grant)
    }

    pub fn ensure_active_at(
        &self,
        now: DateTime<Utc>,
    ) -> Result<(), AuditorAccessGrantLifecycleError> {
        if self.revoked_at.is_some() || now >= self.expires_at {
            Err(AuditorAccessGrantLifecycleError::Unavailable)
        } else {
            Ok(())
        }
    }

    pub fn revoke(
        &mut self,
        revoked_at: DateTime<Utc>,
    ) -> Result<AuditorAccessGrantRevocation, AuditorAccessGrantLifecycleError> {
        if self.revoked_at.is_some() {
            return Ok(AuditorAccessGrantRevocation::AlreadyRevoked);
        }
        if revoked_at < self.created_at {
            return Err(AuditorAccessGrantLifecycleError::InvalidRevocation);
        }
        self.revoked_at = Some(revoked_at);
        Ok(AuditorAccessGrantRevocation::Revoked)
    }

    pub fn secret_digest(&self) -> Sha256Digest {
        self.secret_digest
    }
}

fn valid_normalized_email(value: &str) -> bool {
    let Some((local, domain)) = value.split_once('@') else {
        return false;
    };
    !local.is_empty()
        && !domain.is_empty()
        && !domain.contains('@')
        && value == value.trim()
        && value.bytes().all(|byte| !byte.is_ascii_uppercase())
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, TimeZone, Utc};
    use uuid::Uuid;

    use super::{AuditReviewPeriod, AuditorAccessGrant, AuditorAccessGrantRevocation};
    use crate::domain::{DomainError, Sha256Digest};

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

    #[test]
    fn grant_owns_active_and_revoked_lifecycle() {
        let created_at = Utc.with_ymd_and_hms(2026, 8, 2, 12, 0, 0).unwrap();
        let expires_at = created_at + Duration::days(30);
        let mut grant = AuditorAccessGrant::issue(
            Uuid::new_v4().into(),
            Uuid::new_v4().into(),
            "auditor@example.com".to_owned(),
            Sha256Digest::digest(b"invite"),
            Uuid::new_v4().into(),
            Uuid::new_v4().into(),
            created_at,
            expires_at,
            AuditReviewPeriod::new(created_at - Duration::days(90), created_at).unwrap(),
        )
        .unwrap();

        assert_eq!(grant.ensure_active_at(created_at), Ok(()));
        assert_eq!(
            grant.revoke(created_at + Duration::seconds(1)),
            Ok(AuditorAccessGrantRevocation::Revoked)
        );
        assert_eq!(
            grant.revoke(created_at + Duration::seconds(2)),
            Ok(AuditorAccessGrantRevocation::AlreadyRevoked)
        );
        assert!(grant
            .ensure_active_at(created_at + Duration::seconds(2))
            .is_err());
    }
}
