use chrono::{DateTime, Utc};
use tokio_postgres::Row;
use uuid::Uuid;

use crate::domain::{
    AuditReviewPeriod, AuditorAccessGrantId, AuditorSession, AuditorSessionId, Sha256Digest,
};

use super::{Error, Postgres};

const SESSION_COLUMNS: &str = "s.id, s.grant_id, g.workspace_id, s.session_digest, s.auditor_email, s.auth0_subject, s.expires_at, g.period_start, g.period_end, s.revoked_at, s.last_used_at, s.created_at";

pub struct NewAuditorSession {
    pub id: AuditorSessionId,
    pub grant_id: AuditorAccessGrantId,
    pub session_digest: [u8; 32],
    pub auditor_email: String,
    pub auth0_subject: String,
    pub expires_at: DateTime<Utc>,
}

impl Postgres {
    pub async fn create_active_auditor_session(
        &self,
        session: NewAuditorSession,
    ) -> Result<Option<AuditorSession>, Error> {
        let client = self.get().await?;
        let session_digest: &[u8] = &session.session_digest;
        let rows = client
            .query(
                &format!(
                    r#"
WITH inserted AS (
    INSERT INTO auditor_sessions (
        id, grant_id, session_digest, auditor_email, auth0_subject, expires_at
    )
    SELECT $1, g.id, $3, g.auditor_email, $4, $5
    FROM auditor_access_grants g
    WHERE g.id = $2
      AND g.revoked_at IS NULL
      AND g.expires_at > now()
      AND g.auditor_email = $6
      AND btrim($4) <> ''
    RETURNING id, grant_id, auditor_email, auth0_subject, expires_at, revoked_at, last_used_at, created_at
)
SELECT {SESSION_COLUMNS}
FROM inserted s
JOIN auditor_access_grants g ON g.id = s.grant_id
"#
                ),
                &[
                    &Uuid::from(session.id),
                    &Uuid::from(session.grant_id),
                    &session_digest,
                    &session.auth0_subject,
                    &session.expires_at,
                    &session.auditor_email,
                ],
            )
            .await?;

        rows.into_iter()
            .next()
            .map(|row| auditor_session_from_row(&row))
            .transpose()
    }

    pub async fn get_active_auditor_session_by_digest(
        &self,
        session_digest: [u8; 32],
    ) -> Result<Option<AuditorSession>, Error> {
        let client = self.get().await?;
        let digest: &[u8] = &session_digest;
        let rows = client
            .query(
                &format!(
                    r#"
UPDATE auditor_sessions s
SET last_used_at = now(), updated_at = now()
FROM auditor_access_grants g
WHERE s.grant_id = g.id
  AND s.session_digest = $1
  AND s.revoked_at IS NULL
  AND s.expires_at > now()
  AND g.revoked_at IS NULL
  AND g.expires_at > now()
RETURNING {SESSION_COLUMNS}
"#
                ),
                &[&digest],
            )
            .await?;

        rows.into_iter()
            .next()
            .map(|row| auditor_session_from_row(&row))
            .transpose()
    }

    pub async fn revoke_auditor_session_by_digest(
        &self,
        session_digest: [u8; 32],
    ) -> Result<Option<AuditorSession>, Error> {
        let client = self.get().await?;
        let digest: &[u8] = &session_digest;
        let rows = client
            .query(
                &format!(
                    r#"
UPDATE auditor_sessions s
SET revoked_at = COALESCE(s.revoked_at, now()), updated_at = now()
FROM auditor_access_grants g
WHERE s.grant_id = g.id AND s.session_digest = $1
RETURNING {SESSION_COLUMNS}
"#
                ),
                &[&digest],
            )
            .await?;

        rows.into_iter()
            .next()
            .map(|row| auditor_session_from_row(&row))
            .transpose()
    }
}

fn auditor_session_from_row(row: &Row) -> Result<AuditorSession, Error> {
    let session_digest = row.try_get::<_, Vec<u8>>("session_digest")?;
    let session_digest = session_digest
        .try_into()
        .map_err(|_| Error::InvariantViolation("auditor session digest must contain 32 bytes"))?;
    AuditorSession::rehydrate(
        row.try_get::<_, Uuid>("id")?.into(),
        row.try_get::<_, Uuid>("grant_id")?.into(),
        row.try_get::<_, Uuid>("workspace_id")?.into(),
        row.try_get("auditor_email")?,
        Sha256Digest::from_bytes(session_digest),
        row.try_get("auth0_subject")?,
        row.try_get("expires_at")?,
        AuditReviewPeriod::new(row.try_get("period_start")?, row.try_get("period_end")?)?,
        row.try_get("revoked_at")?,
        row.try_get("last_used_at")?,
        row.try_get("created_at")?,
    )
    .map_err(|_| Error::InvariantViolation("persisted auditor session is inconsistent"))
}
