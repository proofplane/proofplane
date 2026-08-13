use std::sync::Arc;

use chrono::{DateTime, Utc};
use secrecy::{ExposeSecret, SecretString};
use uuid::Uuid;

use crate::{
    domain::{AuditReviewPeriod, AuditorAccessGrantId, AuditorSessionId, WorkspaceId},
    persistence::{param, Postgres},
};

/// A read-only locator for the session cookie. It deliberately returns no digest.
#[derive(Debug)]
pub struct ResolveAuditorSessionByDigest {
    pub raw_session: SecretString,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedAuditorSession {
    pub id: AuditorSessionId,
    pub grant_id: AuditorAccessGrantId,
    pub workspace_id: WorkspaceId,
    pub auditor_email: String,
    pub auth0_subject: String,
    pub expires_at: DateTime<Utc>,
    pub period: AuditReviewPeriod,
    pub last_used_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone)]
pub struct ResolveAuditorSessionByDigestHandler {
    repository: Arc<Postgres>,
}
impl ResolveAuditorSessionByDigestHandler {
    pub fn new(repository: Arc<Postgres>) -> Self {
        Self { repository }
    }
    pub async fn handle(
        &self,
        query: ResolveAuditorSessionByDigest,
    ) -> Result<Option<ResolvedAuditorSession>, ResolveAuditorSessionByDigestError> {
        let digest =
            crate::domain::Sha256Digest::digest(query.raw_session.expose_secret().as_bytes());
        let digest: &[u8] = digest.as_bytes();
        let client = self
            .repository
            .get()
            .await
            .map_err(crate::persistence::Error::from)?;
        let row = client
            .query_typed_opt(SQL, &[param(&digest)])
            .await
            .map_err(crate::persistence::Error::from)?;
        row.map(
            |row| -> Result<ResolvedAuditorSession, crate::persistence::Error> {
                Ok(ResolvedAuditorSession {
                    id: AuditorSessionId::from(row.try_get::<_, Uuid>("id")?),
                    grant_id: AuditorAccessGrantId::from(row.try_get::<_, Uuid>("grant_id")?),
                    workspace_id: WorkspaceId::from(row.try_get::<_, Uuid>("workspace_id")?),
                    auditor_email: row.try_get("auditor_email")?,
                    auth0_subject: row.try_get("auth0_subject")?,
                    expires_at: row.try_get("expires_at")?,
                    period: AuditReviewPeriod::new(
                        row.try_get("period_start")?,
                        row.try_get("period_end")?,
                    )
                    .map_err(|_| {
                        crate::persistence::Error::InvariantViolation(
                            "persisted auditor session period is invalid",
                        )
                    })?,
                    last_used_at: row.try_get("last_used_at")?,
                    created_at: row.try_get("created_at")?,
                })
            },
        )
        .transpose()
        .map_err(Into::into)
    }
}
const SQL: &str = r#"
SELECT s.id, s.grant_id, g.workspace_id, s.auditor_email, s.auth0_subject,
       s.expires_at, g.period_start, g.period_end, s.last_used_at, s.created_at
FROM auditor_sessions s JOIN auditor_access_grants g ON g.id = s.grant_id
WHERE s.session_digest = $1 AND s.revoked_at IS NULL AND s.expires_at > now()
  AND g.revoked_at IS NULL AND g.expires_at > now()
"#;
#[derive(Debug, thiserror::Error)]
pub enum ResolveAuditorSessionByDigestError {
    #[error("repository error")]
    Repository(#[from] crate::persistence::Error),
}
#[cfg(test)]
mod tests {
    use super::SQL;
    #[test]
    fn locator_is_read_only_and_conceals_inactive_sessions() {
        assert!(!SQL.contains("UPDATE"));
        assert!(SQL.contains("s.revoked_at IS NULL"));
        assert!(SQL.contains("g.revoked_at IS NULL"));
        assert!(SQL.contains("expires_at > now()"));
    }
}
