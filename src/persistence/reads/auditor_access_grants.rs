use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::{
    domain::{AuditReviewPeriod, AuditorAccessGrantId, WorkspaceId},
    persistence::Error,
    read_models::AuditorAccessGrantSummary,
};

use super::{ReadExecutor, TransactionalReadExecutor};

pub(crate) struct AuditorAccessGrantReads<'a, E> {
    executor: &'a E,
    workspace_id: Option<WorkspaceId>,
}
impl<'a, E> AuditorAccessGrantReads<'a, E> {
    pub(crate) fn new(executor: &'a E, workspace_id: Option<WorkspaceId>) -> Self {
        Self {
            executor,
            workspace_id,
        }
    }
}

impl<E: ReadExecutor> AuditorAccessGrantReads<'_, E> {
    pub async fn list(
        &self,
        workspace_id: WorkspaceId,
    ) -> Result<Vec<AuditorAccessGrantSummary>, Error> {
        self.executor.query("SELECT id, auditor_email, created_at, expires_at, period_start, period_end, revoked_at FROM auditor_access_grants WHERE workspace_id = $1 ORDER BY created_at DESC, id DESC", &[&Uuid::from(workspace_id)]).await?.into_iter().map(summary_from_row).collect()
    }
}

impl AuditorAccessGrantReads<'_, TransactionalReadExecutor<'_>> {
    pub async fn get_active(
        &self,
        id: AuditorAccessGrantId,
        now: DateTime<Utc>,
    ) -> Result<Option<AuditorAccessGrantSummary>, Error> {
        let workspace_id = self.workspace_id.ok_or(Error::InvariantViolation(
            "auditor grant read requires workspace scope",
        ))?;
        self.executor.query_opt("SELECT id, auditor_email, created_at, expires_at, period_start, period_end, revoked_at FROM auditor_access_grants WHERE id = $1 AND workspace_id = $2 AND revoked_at IS NULL AND expires_at > $3 FOR KEY SHARE", &[&Uuid::from(id), &Uuid::from(workspace_id), &now]).await?.map(summary_from_row).transpose()
    }
}

fn summary_from_row(row: tokio_postgres::Row) -> Result<AuditorAccessGrantSummary, Error> {
    Ok(AuditorAccessGrantSummary {
        id: row.try_get::<_, Uuid>("id")?.into(),
        auditor_email: row.try_get("auditor_email")?,
        created_at: row.try_get("created_at")?,
        expires_at: row.try_get("expires_at")?,
        period: AuditReviewPeriod::new(row.try_get("period_start")?, row.try_get("period_end")?)?,
        revoked_at: row.try_get("revoked_at")?,
    })
}
