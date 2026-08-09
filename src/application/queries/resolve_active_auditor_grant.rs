use std::sync::Arc;

use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::{
    domain::{AuditReviewPeriod, AuditorAccessGrantId, WorkspaceId},
    repository::{Error, Postgres},
};

#[derive(Debug, Clone, Copy)]
pub struct ResolveActiveAuditorGrant {
    pub grant_id: AuditorAccessGrantId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedActiveAuditorGrant {
    pub id: AuditorAccessGrantId,
    pub workspace_id: WorkspaceId,
    pub auditor_email: String,
    pub expires_at: DateTime<Utc>,
    pub period: AuditReviewPeriod,
}

#[derive(Clone)]
pub struct ResolveActiveAuditorGrantHandler {
    repository: Arc<Postgres>,
}

impl ResolveActiveAuditorGrantHandler {
    pub fn new(repository: Arc<Postgres>) -> Self {
        Self { repository }
    }

    pub async fn handle(
        &self,
        query: ResolveActiveAuditorGrant,
    ) -> Result<Option<ResolvedActiveAuditorGrant>, Error> {
        let row = self
            .repository
            .get()
            .await?
            .query_opt(SQL, &[&Uuid::from(query.grant_id)])
            .await?;
        row.map(|row| {
            Ok(ResolvedActiveAuditorGrant {
                id: row.try_get::<_, Uuid>("id")?.into(),
                workspace_id: row.try_get::<_, Uuid>("workspace_id")?.into(),
                auditor_email: row.try_get("auditor_email")?,
                expires_at: row.try_get("expires_at")?,
                period: AuditReviewPeriod::new(
                    row.try_get("period_start")?,
                    row.try_get("period_end")?,
                )
                .map_err(|_| Error::InvariantViolation("auditor grant period is invalid"))?,
            })
        })
        .transpose()
    }
}

const SQL: &str = r#"
SELECT id, workspace_id, auditor_email, expires_at, period_start, period_end
FROM auditor_access_grants
WHERE id = $1 AND revoked_at IS NULL AND expires_at > now()
"#;

#[cfg(test)]
mod tests {
    use super::SQL;

    #[test]
    fn locator_is_read_only_and_conceals_inactive_grants_and_digests() {
        assert!(!SQL.contains("UPDATE"));
        assert!(SQL.contains("revoked_at IS NULL"));
        assert!(SQL.contains("expires_at > now()"));
        assert!(!SQL.contains("secret_digest"));
    }
}
