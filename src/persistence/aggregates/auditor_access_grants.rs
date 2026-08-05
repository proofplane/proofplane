use chrono::{DateTime, Utc};
use tokio_postgres::Row;
use uuid::Uuid;

use crate::{
    authentication::opaque_token::AuditorInviteSecretDigest,
    domain::{
        AuditReviewPeriod, AuditorAccessGrant, AuditorAccessGrantId, Sha256Digest, WorkspaceId,
    },
};

use super::params::param;
use super::{
    snapshot::{save_snapshot, snapshot_record},
    Error, Postgres, UnitOfWork, WorkspaceUnitOfWork,
};

enum RepositoryConnection<'a> {
    Postgres(&'a Postgres),
    Transaction(&'a UnitOfWork<'a>),
    WorkspaceTransaction(&'a WorkspaceUnitOfWork<'a>),
}

pub struct AuditorAccessGrantRepository<'a> {
    connection: RepositoryConnection<'a>,
}

impl Postgres {
    pub fn auditor_access_grants(&self) -> AuditorAccessGrantRepository<'_> {
        AuditorAccessGrantRepository {
            connection: RepositoryConnection::Postgres(self),
        }
    }
}

impl<'a> UnitOfWork<'a> {
    pub fn auditor_access_grants(&'a self) -> AuditorAccessGrantRepository<'a> {
        AuditorAccessGrantRepository {
            connection: RepositoryConnection::Transaction(self),
        }
    }
}

impl<'a> WorkspaceUnitOfWork<'a> {
    pub fn auditor_access_grants(&'a self) -> AuditorAccessGrantRepository<'a> {
        AuditorAccessGrantRepository {
            connection: RepositoryConnection::WorkspaceTransaction(self),
        }
    }
}

impl AuditorAccessGrantRepository<'_> {
    pub async fn get(
        &self,
        grant_id: AuditorAccessGrantId,
        workspace_id: WorkspaceId,
    ) -> Result<Option<AuditorAccessGrant>, Error> {
        let id = Uuid::from(grant_id);
        let workspace_id = Uuid::from(workspace_id);
        let rows = match self.connection {
            RepositoryConnection::Postgres(postgres) => {
                postgres
                    .get()
                    .await?
                    .query_typed(GET_SQL, &[param(&id), param(&workspace_id)])
                    .await?
            }
            RepositoryConnection::Transaction(unit_of_work) => {
                unit_of_work
                    .transaction
                    .query_typed(GET_FOR_UPDATE_SQL, &[param(&id), param(&workspace_id)])
                    .await?
            }
            RepositoryConnection::WorkspaceTransaction(workspace) => {
                if workspace_id != Uuid::from(workspace.workspace_id) {
                    return Ok(None);
                }
                workspace
                    .transaction
                    .query_typed(GET_FOR_UPDATE_SQL, &[param(&id), param(&workspace_id)])
                    .await?
            }
        };
        rows.into_iter()
            .next()
            .map(|row| AuditorAccessGrantRecord::try_from_row(&row)?.into_domain())
            .transpose()
    }

    pub async fn save(&self, grant: &AuditorAccessGrant) -> Result<(), Error> {
        let record = AuditorAccessGrantRecord::from_domain(grant)?;
        match self.connection {
            RepositoryConnection::Postgres(_) => Err(Error::InvariantViolation(
                "auditor access grants must be saved in a transaction",
            )),
            RepositoryConnection::Transaction(unit_of_work) => {
                save_snapshot(&unit_of_work.transaction, record.as_snapshot()).await
            }
            RepositoryConnection::WorkspaceTransaction(workspace) => {
                save_snapshot(workspace.transaction, record.as_snapshot()).await
            }
        }
    }
}

const COLUMNS: &str = "id, workspace_id, auditor_email, secret_digest, created_by_user_id, created_via_agent_connection_id, created_at, expires_at, period_start, period_end, revoked_at";
const GET_SQL: &str = "SELECT id, workspace_id, auditor_email, secret_digest, created_by_user_id, created_via_agent_connection_id, created_at, expires_at, period_start, period_end, revoked_at FROM auditor_access_grants WHERE id = $1 AND workspace_id = $2";
const GET_FOR_UPDATE_SQL: &str = "SELECT id, workspace_id, auditor_email, secret_digest, created_by_user_id, created_via_agent_connection_id, created_at, expires_at, period_start, period_end, revoked_at FROM auditor_access_grants WHERE id = $1 AND workspace_id = $2 FOR UPDATE";

snapshot_record! {
    struct AuditorAccessGrantRecord {
        id: Uuid,
        workspace_id: Uuid,
        auditor_email: String,
        secret_digest: Vec<u8>,
        created_by_user_id: Uuid,
        created_via_agent_connection_id: Uuid,
        created_at: DateTime<Utc>,
        expires_at: DateTime<Utc>,
        period_start: DateTime<Utc>,
        period_end: DateTime<Utc>,
        revoked_at: Option<DateTime<Utc>>,
    }
    table: auditor_access_grants,
    conflict: id,
}

impl AuditorAccessGrantRecord {
    fn try_from_row(row: &Row) -> Result<Self, Error> {
        Ok(Self {
            id: row.try_get("id")?,
            workspace_id: row.try_get("workspace_id")?,
            auditor_email: row.try_get("auditor_email")?,
            secret_digest: row.try_get("secret_digest")?,
            created_by_user_id: row.try_get("created_by_user_id")?,
            created_via_agent_connection_id: row.try_get("created_via_agent_connection_id")?,
            created_at: row.try_get("created_at")?,
            expires_at: row.try_get("expires_at")?,
            period_start: row.try_get("period_start")?,
            period_end: row.try_get("period_end")?,
            revoked_at: row.try_get("revoked_at")?,
        })
    }
    fn into_domain(self) -> Result<AuditorAccessGrant, Error> {
        let digest = self.secret_digest.try_into().map_err(|_| {
            Error::InvariantViolation("auditor access grant digest must contain 32 bytes")
        })?;
        AuditorAccessGrant::rehydrate(
            self.id.into(),
            self.workspace_id.into(),
            self.auditor_email,
            Sha256Digest::from_bytes(digest),
            self.created_by_user_id.into(),
            self.created_via_agent_connection_id.into(),
            self.created_at,
            self.expires_at,
            AuditReviewPeriod::new(self.period_start, self.period_end)?,
            self.revoked_at,
        )
        .map_err(|_| Error::InvariantViolation("persisted auditor access grant is inconsistent"))
    }
    fn from_domain(grant: &AuditorAccessGrant) -> Result<Self, Error> {
        Ok(Self {
            id: grant.id.into(),
            workspace_id: grant.workspace_id.into(),
            auditor_email: grant.auditor_email.clone(),
            secret_digest: grant.secret_digest().as_bytes().to_vec(),
            created_by_user_id: grant.created_by_user_id.into(),
            created_via_agent_connection_id: grant.created_via_agent_connection_id.into(),
            created_at: grant.created_at,
            expires_at: grant.expires_at,
            period_start: grant.period.start,
            period_end: grant.period.end,
            revoked_at: grant.revoked_at,
        })
    }
}

impl Postgres {
    pub async fn get_active_auditor_access_grant_by_id(
        &self,
        grant_id: AuditorAccessGrantId,
    ) -> Result<Option<AuditorAccessGrant>, Error> {
        let row = self
            .get()
            .await?
            .query_typed_opt(
                &format!("SELECT {COLUMNS} FROM auditor_access_grants WHERE id = $1"),
                &[param(&Uuid::from(grant_id))],
            )
            .await?;
        let grant = row
            .map(|row| AuditorAccessGrantRecord::try_from_row(&row)?.into_domain())
            .transpose()?;
        Ok(grant.filter(|grant| grant.ensure_active_at(Utc::now()).is_ok()))
    }

    pub async fn list_auditor_access_grants(
        &self,
        workspace_id: WorkspaceId,
    ) -> Result<Vec<AuditorAccessGrant>, Error> {
        let rows = self.get().await?
            .query_typed(
                &format!(
                    "SELECT {COLUMNS} FROM auditor_access_grants WHERE workspace_id = $1 ORDER BY created_at DESC, id DESC"
                ),
                &[param(&Uuid::from(workspace_id))],
            )
            .await?;
        rows.into_iter()
            .map(|row| AuditorAccessGrantRecord::try_from_row(&row)?.into_domain())
            .collect()
    }

    pub async fn get_active_auditor_access_grant_by_digest(
        &self,
        workspace_id: WorkspaceId,
        digest: AuditorInviteSecretDigest,
    ) -> Result<Option<AuditorAccessGrant>, Error> {
        let digest: &[u8] = digest.as_bytes();
        let row = self.get().await?
            .query_typed_opt(
                &format!(
                    "SELECT {COLUMNS} FROM auditor_access_grants WHERE workspace_id = $1 AND secret_digest = $2"
                ),
                &[param(&Uuid::from(workspace_id)), param(&digest)],
            )
            .await?;
        let grant = row
            .map(|row| AuditorAccessGrantRecord::try_from_row(&row)?.into_domain())
            .transpose()?;
        Ok(grant.filter(|grant| grant.ensure_active_at(Utc::now()).is_ok()))
    }
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, TimeZone, Utc};
    use uuid::Uuid;

    use super::{AuditorAccessGrantRecord, GET_FOR_UPDATE_SQL, GET_SQL};
    use crate::domain::{AuditReviewPeriod, AuditorAccessGrant, Sha256Digest};

    #[test]
    fn verification_and_transactional_reads_are_workspace_scoped_and_lock_distinctly() {
        assert!(GET_SQL.contains("workspace_id = $2"));
        assert!(!GET_SQL.contains("FOR UPDATE"));
        assert!(GET_FOR_UPDATE_SQL.contains("workspace_id = $2"));
        assert!(GET_FOR_UPDATE_SQL.contains("FOR UPDATE"));
    }

    #[test]
    fn record_round_trip_preserves_a_revoked_grant_snapshot() {
        let created_at = Utc.with_ymd_and_hms(2026, 8, 2, 12, 0, 0).unwrap();
        let mut grant = AuditorAccessGrant::issue(
            Uuid::new_v4().into(),
            Uuid::new_v4().into(),
            "auditor@example.com".to_owned(),
            Sha256Digest::digest(b"secret"),
            Uuid::new_v4().into(),
            Uuid::new_v4().into(),
            created_at,
            created_at + Duration::days(30),
            AuditReviewPeriod::new(created_at - Duration::days(90), created_at).unwrap(),
        )
        .unwrap();
        grant.revoke(created_at + Duration::seconds(1)).unwrap();

        assert_eq!(
            AuditorAccessGrantRecord::from_domain(&grant)
                .unwrap()
                .into_domain()
                .unwrap(),
            grant
        );
    }
}
