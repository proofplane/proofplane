use chrono::{DateTime, Utc};
use tokio_postgres::Row;
use uuid::Uuid;

use crate::domain::{
    AuditReviewPeriod, AuditorAccessGrantId, AuditorAccessOtp, AuditorAccessOtpId, AuditorSession,
    AuditorSessionId,
};

use super::{Error, Postgres};

const OTP_COLUMNS: &str = "id, grant_id, expires_at, consumed_at, failed_attempt_count, created_at";
const SESSION_COLUMNS: &str = "s.id, s.grant_id, g.workspace_id, s.auditor_email, s.expires_at, g.period_start, g.period_end, s.revoked_at, s.last_used_at, s.created_at";

pub struct NewAuditorAccessOtp {
    pub id: AuditorAccessOtpId,
    pub grant_id: AuditorAccessGrantId,
    pub code_digest: [u8; 32],
    pub expires_at: DateTime<Utc>,
}

pub struct NewAuditorSession {
    pub id: AuditorSessionId,
    pub grant_id: AuditorAccessGrantId,
    pub session_digest: [u8; 32],
    pub auditor_email: String,
    pub expires_at: DateTime<Utc>,
}

impl Postgres {
    pub async fn recent_auditor_otp_count(
        &self,
        grant_id: AuditorAccessGrantId,
    ) -> Result<i64, Error> {
        let client = self.get().await?;
        let row = client
            .query_one(
                r#"
SELECT count(*)::bigint AS count
FROM auditor_access_otps
WHERE grant_id = $1 AND created_at > now() - interval '10 minutes'
"#,
                &[&Uuid::from(grant_id)],
            )
            .await?;

        Ok(row.get("count"))
    }

    pub async fn create_auditor_otp(
        &self,
        otp: NewAuditorAccessOtp,
    ) -> Result<AuditorAccessOtp, Error> {
        let client = self.get().await?;
        let digest: &[u8] = &otp.code_digest;
        let row = client
            .query_one(
                &format!(
                    r#"
INSERT INTO auditor_access_otps (id, grant_id, code_digest, expires_at)
VALUES ($1, $2, $3, $4)
RETURNING {OTP_COLUMNS}
"#
                ),
                &[
                    &Uuid::from(otp.id),
                    &Uuid::from(otp.grant_id),
                    &digest,
                    &otp.expires_at,
                ],
            )
            .await?;

        auditor_otp_from_row(&row)
    }

    pub async fn delete_auditor_otp(&self, id: AuditorAccessOtpId) -> Result<bool, Error> {
        let client = self.get().await?;
        let deleted = client
            .execute(
                "DELETE FROM auditor_access_otps WHERE id = $1",
                &[&Uuid::from(id)],
            )
            .await?;

        Ok(deleted > 0)
    }

    pub async fn verify_auditor_otp_and_create_session(
        &self,
        grant_id: AuditorAccessGrantId,
        code_digest: [u8; 32],
        session: NewAuditorSession,
    ) -> Result<Option<AuditorSession>, Error> {
        let mut client = self.get().await?;
        let transaction = client.transaction().await?;
        let digest: &[u8] = &code_digest;
        let otp = transaction
            .query_opt(
                r#"
SELECT id, code_digest
FROM auditor_access_otps
WHERE grant_id = $1
  AND consumed_at IS NULL
  AND expires_at > now()
  AND failed_attempt_count < 5
ORDER BY created_at DESC
LIMIT 1
FOR UPDATE
"#,
                &[&Uuid::from(grant_id)],
            )
            .await?;
        let Some(otp) = otp else {
            transaction.commit().await?;
            return Ok(None);
        };
        let otp_id: Uuid = otp.get("id");
        let stored_digest: Vec<u8> = otp.get("code_digest");
        if stored_digest.as_slice() != digest {
            transaction
                .execute(
                    r#"
UPDATE auditor_access_otps
SET failed_attempt_count = failed_attempt_count + 1, updated_at = now()
WHERE id = $1
"#,
                    &[&otp_id],
                )
                .await?;
            transaction.commit().await?;
            return Ok(None);
        }

        let session_digest: &[u8] = &session.session_digest;
        let row = transaction
            .query_one(
                &format!(
                    r#"
WITH consumed AS (
    UPDATE auditor_access_otps
    SET consumed_at = now(), updated_at = now()
    WHERE id = $1 AND consumed_at IS NULL
    RETURNING id
), inserted AS (
    INSERT INTO auditor_sessions (id, grant_id, session_digest, auditor_email, expires_at)
    SELECT $2, $3, $4, $5, $6 FROM consumed
    RETURNING id, grant_id, auditor_email, expires_at, revoked_at, last_used_at, created_at
)
SELECT {SESSION_COLUMNS}
FROM inserted s
JOIN auditor_access_grants g ON g.id = s.grant_id
"#
                ),
                &[
                    &otp_id,
                    &Uuid::from(session.id),
                    &Uuid::from(session.grant_id),
                    &session_digest,
                    &session.auditor_email,
                    &session.expires_at,
                ],
            )
            .await?;
        transaction.commit().await?;

        auditor_session_from_row(&row).map(Some)
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

fn auditor_otp_from_row(row: &Row) -> Result<AuditorAccessOtp, Error> {
    Ok(AuditorAccessOtp {
        id: AuditorAccessOtpId::from(row.try_get::<_, Uuid>("id")?),
        grant_id: AuditorAccessGrantId::from(row.try_get::<_, Uuid>("grant_id")?),
        expires_at: row.try_get("expires_at")?,
        consumed_at: row.try_get("consumed_at")?,
        failed_attempt_count: row.try_get("failed_attempt_count")?,
        created_at: row.try_get("created_at")?,
    })
}

fn auditor_session_from_row(row: &Row) -> Result<AuditorSession, Error> {
    Ok(AuditorSession {
        id: AuditorSessionId::from(row.try_get::<_, Uuid>("id")?),
        grant_id: AuditorAccessGrantId::from(row.try_get::<_, Uuid>("grant_id")?),
        workspace_id: row.try_get::<_, Uuid>("workspace_id")?.into(),
        auditor_email: row.try_get("auditor_email")?,
        expires_at: row.try_get("expires_at")?,
        period: AuditReviewPeriod::new(row.try_get("period_start")?, row.try_get("period_end")?)?,
        revoked_at: row.try_get("revoked_at")?,
        last_used_at: row.try_get("last_used_at")?,
        created_at: row.try_get("created_at")?,
    })
}
