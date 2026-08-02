use chrono::{DateTime, Utc};

use super::{ids::uuid_id, AuditReviewPeriod, AuditorAccessGrantId, Sha256Digest, WorkspaceId};

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
    session_digest: Sha256Digest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuditorSessionTransition {
    Used,
    Revoked,
    AlreadyRevoked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum AuditorSessionLifecycleError {
    #[error("auditor session creation is invalid")]
    InvalidCreation,
    #[error("persisted auditor session is inconsistent")]
    InvalidRehydration,
    #[error("auditor session is unavailable")]
    Unavailable,
    #[error("auditor session transition is invalid")]
    InvalidTransition,
}

impl AuditorSession {
    #[allow(clippy::too_many_arguments)]
    pub fn create(
        id: AuditorSessionId,
        grant_id: AuditorAccessGrantId,
        workspace_id: WorkspaceId,
        auditor_email: String,
        session_digest: Sha256Digest,
        auth0_subject: String,
        expires_at: DateTime<Utc>,
        period: AuditReviewPeriod,
        created_at: DateTime<Utc>,
    ) -> Result<Self, AuditorSessionLifecycleError> {
        if expires_at <= created_at
            || auth0_subject.trim().is_empty()
            || !valid_normalized_email(&auditor_email)
        {
            return Err(AuditorSessionLifecycleError::InvalidCreation);
        }
        Ok(Self {
            id,
            grant_id,
            workspace_id,
            auditor_email,
            auth0_subject,
            expires_at,
            period,
            revoked_at: None,
            last_used_at: created_at,
            created_at,
            session_digest,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn rehydrate(
        id: AuditorSessionId,
        grant_id: AuditorAccessGrantId,
        workspace_id: WorkspaceId,
        auditor_email: String,
        session_digest: Sha256Digest,
        auth0_subject: String,
        expires_at: DateTime<Utc>,
        period: AuditReviewPeriod,
        revoked_at: Option<DateTime<Utc>>,
        last_used_at: DateTime<Utc>,
        created_at: DateTime<Utc>,
    ) -> Result<Self, AuditorSessionLifecycleError> {
        let mut session = Self::create(
            id,
            grant_id,
            workspace_id,
            auditor_email,
            session_digest,
            auth0_subject,
            expires_at,
            period,
            created_at,
        )
        .map_err(|_| AuditorSessionLifecycleError::InvalidRehydration)?;
        if last_used_at < created_at || revoked_at.is_some_and(|value| value < created_at) {
            return Err(AuditorSessionLifecycleError::InvalidRehydration);
        }
        session.revoked_at = revoked_at;
        session.last_used_at = last_used_at;
        Ok(session)
    }

    pub fn ensure_active_at(&self, now: DateTime<Utc>) -> Result<(), AuditorSessionLifecycleError> {
        if self.revoked_at.is_some() || now >= self.expires_at {
            Err(AuditorSessionLifecycleError::Unavailable)
        } else {
            Ok(())
        }
    }

    pub fn touch(
        &mut self,
        used_at: DateTime<Utc>,
    ) -> Result<AuditorSessionTransition, AuditorSessionLifecycleError> {
        self.ensure_active_at(used_at)?;
        if used_at < self.created_at || used_at < self.last_used_at {
            return Err(AuditorSessionLifecycleError::InvalidTransition);
        }
        self.last_used_at = used_at;
        Ok(AuditorSessionTransition::Used)
    }

    pub fn revoke(
        &mut self,
        revoked_at: DateTime<Utc>,
    ) -> Result<AuditorSessionTransition, AuditorSessionLifecycleError> {
        if self.revoked_at.is_some() {
            return Ok(AuditorSessionTransition::AlreadyRevoked);
        }
        if revoked_at < self.created_at {
            return Err(AuditorSessionLifecycleError::InvalidTransition);
        }
        self.revoked_at = Some(revoked_at);
        Ok(AuditorSessionTransition::Revoked)
    }

    pub fn session_digest(&self) -> Sha256Digest {
        self.session_digest
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

    use super::{AuditorSession, AuditorSessionTransition};
    use crate::domain::{AuditReviewPeriod, Sha256Digest};

    #[test]
    fn session_owns_use_expiry_and_revocation() {
        let created_at = Utc.with_ymd_and_hms(2026, 8, 2, 12, 0, 0).unwrap();
        let expires_at = created_at + Duration::days(7);
        let mut session = AuditorSession::create(
            Uuid::new_v4().into(),
            Uuid::new_v4().into(),
            Uuid::new_v4().into(),
            "auditor@example.com".to_owned(),
            Sha256Digest::digest(b"session"),
            "email|auditor".to_owned(),
            expires_at,
            AuditReviewPeriod::new(created_at - Duration::days(90), created_at).unwrap(),
            created_at,
        )
        .unwrap();

        let used_at = created_at + Duration::seconds(1);
        assert_eq!(session.touch(used_at), Ok(AuditorSessionTransition::Used));
        assert_eq!(session.last_used_at, used_at);
        assert_eq!(
            session.revoke(used_at + Duration::seconds(1)),
            Ok(AuditorSessionTransition::Revoked)
        );
        assert_eq!(
            session.revoke(used_at + Duration::seconds(2)),
            Ok(AuditorSessionTransition::AlreadyRevoked)
        );
        assert!(session.touch(used_at + Duration::seconds(2)).is_err());
        assert!(AuditorSession::create(
            Uuid::new_v4().into(),
            Uuid::new_v4().into(),
            Uuid::new_v4().into(),
            "auditor@example.com".to_owned(),
            Sha256Digest::digest(b"expired"),
            "email|auditor".to_owned(),
            created_at,
            session.period,
            created_at,
        )
        .is_err());
    }
}
