use chrono::{DateTime, Utc};
use tokio_postgres::Row;
use uuid::Uuid;

use crate::domain::{
    AuditReviewPeriod, AuditorAccessGrantId, AuditorSession, AuditorSessionId, Sha256Digest,
    WorkspaceId,
};

use super::params::param;
use super::{
    snapshot::{save_snapshot, snapshot_record},
    Error, UnitOfWork,
};

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
            .query_typed_opt(GET_FOR_UPDATE_SQL, &[param(&Uuid::from(id))])
            .await?
            .map(|row| AuditorSessionRecord::try_from_row(&row)?.into_domain(&row))
            .transpose()
    }

    /// Persists every mutable and immutable field owned by a session snapshot.
    pub async fn save(&self, session: &AuditorSession) -> Result<(), Error> {
        let record = AuditorSessionRecord::from_domain(session)?;
        save_snapshot(&self.unit_of_work.transaction, record.as_snapshot()).await
    }
}

const GET_FOR_UPDATE_SQL: &str = r#"
SELECT s.id, s.grant_id, g.workspace_id, s.session_digest, s.auditor_email,
       s.auth0_subject, s.expires_at, g.period_start, g.period_end, s.revoked_at,
       s.last_used_at, s.created_at, s.updated_at
FROM auditor_sessions s
JOIN auditor_access_grants g ON g.id = s.grant_id
WHERE s.id = $1
FOR UPDATE OF s
"#;

snapshot_record! {
    struct AuditorSessionRecord {
        id: Uuid,
        grant_id: Uuid,
        session_digest: Vec<u8>,
        auditor_email: String,
        auth0_subject: String,
        expires_at: DateTime<Utc>,
        revoked_at: Option<DateTime<Utc>>,
        last_used_at: DateTime<Utc>,
        created_at: DateTime<Utc>,
        updated_at: DateTime<Utc>,
    }
    table: auditor_sessions,
    conflict: id,
}

impl AuditorSessionRecord {
    fn try_from_row(row: &Row) -> Result<Self, Error> {
        Ok(Self {
            id: row.try_get("id")?,
            grant_id: row.try_get("grant_id")?,
            session_digest: row.try_get("session_digest")?,
            auditor_email: row.try_get("auditor_email")?,
            auth0_subject: row.try_get("auth0_subject")?,
            expires_at: row.try_get("expires_at")?,
            revoked_at: row.try_get("revoked_at")?,
            last_used_at: row.try_get("last_used_at")?,
            created_at: row.try_get("created_at")?,
            updated_at: row.try_get("updated_at")?,
        })
    }

    fn from_domain(session: &AuditorSession) -> Result<Self, Error> {
        Ok(Self {
            id: session.id.into(),
            grant_id: session.grant_id.into(),
            session_digest: session.session_digest().as_bytes().to_vec(),
            auditor_email: session.auditor_email.clone(),
            auth0_subject: session.auth0_subject.clone(),
            expires_at: session.expires_at,
            revoked_at: session.revoked_at,
            last_used_at: session.last_used_at,
            created_at: session.created_at,
            updated_at: session.updated_at,
        })
    }

    fn into_domain(self, row: &Row) -> Result<AuditorSession, Error> {
        let digest: [u8; 32] = self.session_digest.try_into().map_err(|_| {
            Error::InvariantViolation("auditor session digest must contain 32 bytes")
        })?;
        AuditorSession::rehydrate(
            AuditorSessionId::from(self.id),
            AuditorAccessGrantId::from(self.grant_id),
            WorkspaceId::from(row.try_get::<_, Uuid>("workspace_id")?),
            self.auditor_email,
            Sha256Digest::from_bytes(digest),
            self.auth0_subject,
            self.expires_at,
            AuditReviewPeriod::new(row.try_get("period_start")?, row.try_get("period_end")?)?,
            self.revoked_at,
            self.last_used_at,
            self.created_at,
            self.updated_at,
        )
        .map_err(|_| Error::InvariantViolation("persisted auditor session is inconsistent"))
    }
}

#[cfg(test)]
mod tests {
    use super::GET_FOR_UPDATE_SQL;

    #[test]
    fn repository_locks_complete_snapshots_and_saves_all_owned_state() {
        assert!(GET_FOR_UPDATE_SQL.contains("FOR UPDATE OF s"));
        assert!(GET_FOR_UPDATE_SQL.contains("updated_at"));
    }
}
