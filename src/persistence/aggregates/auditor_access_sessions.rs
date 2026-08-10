use chrono::{DateTime, Utc};
use tokio_postgres::Row;
use uuid::Uuid;

use crate::domain::{
    AuditReviewPeriod, AuditorAccessGrantId, AuditorSession, AuditorSessionId, Sha256Digest,
    WorkspaceId,
};

use super::{Error, UnitOfWork};

/// Complete-snapshot persistence for the auditor session aggregate.
pub struct AuditorSessionRepository<'a> {
    unit_of_work: &'a UnitOfWork<'a>,
}

impl<'a> UnitOfWork<'a> {
    pub fn auditor_sessions(&'a self) -> AuditorSessionRepository<'a> {
        AuditorSessionRepository { unit_of_work: self }
    }
}

impl AuditorSessionRepository<'_> {
    /// Locks and rehydrates the complete session selected by its primary key.
    pub async fn get(&self, id: AuditorSessionId) -> Result<Option<AuditorSession>, Error> {
        self.unit_of_work
            .transaction
            .query_opt(GET_FOR_UPDATE_SQL, &[&Uuid::from(id)])
            .await?
            .map(session_from_row)
            .transpose()
    }

    /// Persists every mutable and immutable field owned by a session snapshot.
    pub async fn save(&self, session: &AuditorSession) -> Result<(), Error> {
        let record = SessionRecord::from(session);
        let affected = self
            .unit_of_work
            .transaction
            .execute(
                SAVE_SQL,
                &[
                    &record.id,
                    &record.grant_id,
                    &record.session_digest,
                    &record.auditor_email,
                    &record.auth0_subject,
                    &record.expires_at,
                    &record.revoked_at,
                    &record.last_used_at,
                    &record.created_at,
                ],
            )
            .await?;
        if affected != 1 {
            return Err(Error::InvariantViolation(
                "auditor session snapshot save affected an unexpected row count",
            ));
        }
        Ok(())
    }
}

const GET_FOR_UPDATE_SQL: &str = r#"
SELECT s.id, s.grant_id, g.workspace_id, s.session_digest, s.auditor_email,
       s.auth0_subject, s.expires_at, g.period_start, g.period_end, s.revoked_at,
       s.last_used_at, s.created_at
FROM auditor_sessions s
JOIN auditor_access_grants g ON g.id = s.grant_id
WHERE s.id = $1
FOR UPDATE OF s
"#;

const SAVE_SQL: &str = r#"
INSERT INTO auditor_sessions (
    id, grant_id, session_digest, auditor_email, auth0_subject, expires_at,
    revoked_at, last_used_at, created_at
) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
ON CONFLICT (id) DO UPDATE SET
    grant_id = EXCLUDED.grant_id,
    session_digest = EXCLUDED.session_digest,
    auditor_email = EXCLUDED.auditor_email,
    auth0_subject = EXCLUDED.auth0_subject,
    expires_at = EXCLUDED.expires_at,
    revoked_at = EXCLUDED.revoked_at,
    last_used_at = EXCLUDED.last_used_at,
    created_at = EXCLUDED.created_at,
    updated_at = now()
"#;

struct SessionRecord {
    id: Uuid,
    grant_id: Uuid,
    session_digest: Vec<u8>,
    auditor_email: String,
    auth0_subject: String,
    expires_at: DateTime<Utc>,
    revoked_at: Option<DateTime<Utc>>,
    last_used_at: DateTime<Utc>,
    created_at: DateTime<Utc>,
}

impl From<&AuditorSession> for SessionRecord {
    fn from(session: &AuditorSession) -> Self {
        Self {
            id: session.id.into(),
            grant_id: session.grant_id.into(),
            session_digest: session.session_digest().as_bytes().to_vec(),
            auditor_email: session.auditor_email.clone(),
            auth0_subject: session.auth0_subject.clone(),
            expires_at: session.expires_at,
            revoked_at: session.revoked_at,
            last_used_at: session.last_used_at,
            created_at: session.created_at,
        }
    }
}

fn session_from_row(row: Row) -> Result<AuditorSession, Error> {
    let digest: [u8; 32] = row
        .try_get::<_, Vec<u8>>("session_digest")?
        .try_into()
        .map_err(|_| Error::InvariantViolation("auditor session digest must contain 32 bytes"))?;
    AuditorSession::rehydrate(
        AuditorSessionId::from(row.try_get::<_, Uuid>("id")?),
        AuditorAccessGrantId::from(row.try_get::<_, Uuid>("grant_id")?),
        WorkspaceId::from(row.try_get::<_, Uuid>("workspace_id")?),
        row.try_get("auditor_email")?,
        Sha256Digest::from_bytes(digest),
        row.try_get("auth0_subject")?,
        row.try_get("expires_at")?,
        AuditReviewPeriod::new(row.try_get("period_start")?, row.try_get("period_end")?)?,
        row.try_get("revoked_at")?,
        row.try_get("last_used_at")?,
        row.try_get("created_at")?,
    )
    .map_err(|_| Error::InvariantViolation("persisted auditor session is inconsistent"))
}

#[cfg(test)]
mod tests {
    use super::{GET_FOR_UPDATE_SQL, SAVE_SQL};

    #[test]
    fn repository_locks_complete_snapshots_and_saves_all_owned_state() {
        assert!(GET_FOR_UPDATE_SQL.contains("FOR UPDATE OF s"));
        for field in ["revoked_at", "last_used_at", "created_at", "session_digest"] {
            assert!(SAVE_SQL.contains(field));
        }
    }
}
