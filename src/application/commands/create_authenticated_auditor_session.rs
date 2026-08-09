use std::sync::Arc;

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::{Duration, Utc};
use uuid::Uuid;

use crate::{
    application::ExecutionMetadata,
    domain::{AuditorAccessGrantId, AuditorSession, AuditorSessionId, Sha256Digest, WorkspaceId},
    repository::Postgres,
};

const SESSION_TTL_DAYS: i64 = 7;

#[derive(Debug, Clone)]
pub struct CreateAuthenticatedAuditorSession {
    pub workspace_id: WorkspaceId,
    pub grant_id: AuditorAccessGrantId,
    pub auth0_subject: String,
}

#[derive(Debug)]
pub struct CreatedAuthenticatedAuditorSession {
    pub session: AuditorSession,
    pub raw_session: String,
}

#[derive(Clone)]
pub struct CreateAuthenticatedAuditorSessionHandler {
    repository: Arc<Postgres>,
}

impl CreateAuthenticatedAuditorSessionHandler {
    pub fn new(repository: Arc<Postgres>) -> Self {
        Self { repository }
    }

    pub async fn handle(
        &self,
        command: CreateAuthenticatedAuditorSession,
        _metadata: ExecutionMetadata,
    ) -> Result<CreatedAuthenticatedAuditorSession, CreateAuthenticatedAuditorSessionError> {
        if command.auth0_subject.trim().is_empty() {
            return Err(CreateAuthenticatedAuditorSessionError::Unavailable);
        }
        let raw_session =
            random_value().map_err(|_| CreateAuthenticatedAuditorSessionError::Random)?;
        let digest = Sha256Digest::digest(raw_session.as_bytes());
        let now = Utc::now();
        self.repository
            .in_unit_of_work(async move |context| {
                let grants = context.auditor_access_grants();
                let Some(grant) = grants.get(command.grant_id, command.workspace_id).await? else {
                    return Ok(None);
                };
                if grant.ensure_active_at(now).is_err() {
                    return Ok(None);
                }
                let session = AuditorSession::create(
                    AuditorSessionId::from(Uuid::new_v4()),
                    grant.id,
                    grant.workspace_id,
                    grant.auditor_email.clone(),
                    digest,
                    command.auth0_subject,
                    now + Duration::days(SESSION_TTL_DAYS),
                    grant.period,
                    now,
                )
                .map_err(|_| {
                    crate::repository::Error::InvariantViolation(
                        "auditor session creation is invalid",
                    )
                })?;
                context.auditor_sessions().save(&session).await?;
                Ok(Some(session))
            })
            .await?
            .map(|session| CreatedAuthenticatedAuditorSession {
                session,
                raw_session,
            })
            .ok_or(CreateAuthenticatedAuditorSessionError::Unavailable)
    }
}

fn random_value() -> Result<String, getrandom::Error> {
    let mut bytes = [0_u8; 32];
    getrandom::fill(&mut bytes)?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

#[derive(Debug, thiserror::Error)]
pub enum CreateAuthenticatedAuditorSessionError {
    #[error("auditor access is unavailable")]
    Unavailable,
    #[error("random generation failed")]
    Random,
    #[error("repository error")]
    Repository(#[from] crate::repository::Error),
}
