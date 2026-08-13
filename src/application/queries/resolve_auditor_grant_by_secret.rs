use std::sync::Arc;

use secrecy::{ExposeSecret, SecretString};
use uuid::Uuid;

use crate::{
    application::queries::resolve_active_auditor_grant::ResolvedActiveAuditorGrant,
    authentication::opaque_token::parse_auditor_invite_secret,
    domain::{AuditReviewPeriod, WorkspaceId},
    persistence::{param, Error, Postgres},
};

#[derive(Debug)]
pub struct ResolveAuditorGrantBySecret {
    pub workspace_id: WorkspaceId,
    pub raw_secret: SecretString,
}

#[derive(Clone)]
pub struct ResolveAuditorGrantBySecretHandler {
    repository: Arc<Postgres>,
}

impl ResolveAuditorGrantBySecretHandler {
    pub fn new(repository: Arc<Postgres>) -> Self {
        Self { repository }
    }

    pub async fn handle(
        &self,
        query: ResolveAuditorGrantBySecret,
    ) -> Result<Option<ResolvedActiveAuditorGrant>, Error> {
        let Ok(digest) = parse_auditor_invite_secret(query.raw_secret.expose_secret()) else {
            return Ok(None);
        };
        let row = self
            .repository
            .get()
            .await?
            .query_typed_opt(
                SQL,
                &[
                    param(&Uuid::from(query.workspace_id)),
                    param(&digest.as_bytes().as_slice()),
                ],
            )
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
WHERE workspace_id = $1 AND secret_digest = $2
  AND revoked_at IS NULL AND expires_at > now()
"#;
