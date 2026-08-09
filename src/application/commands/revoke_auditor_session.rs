use crate::{application::ExecutionMetadata, domain::Sha256Digest, repository::Postgres};
use chrono::Utc;
use secrecy::{ExposeSecret, SecretString};
use std::sync::Arc;

#[derive(Debug)]
pub struct RevokeAuditorSession {
    pub raw_session: SecretString,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RevokeAuditorSessionResult {
    Revoked,
    AlreadyRevoked,
}
#[derive(Clone)]
pub struct RevokeAuditorSessionHandler {
    repository: Arc<Postgres>,
}
impl RevokeAuditorSessionHandler {
    pub fn new(repository: Arc<Postgres>) -> Self {
        Self { repository }
    }
    pub async fn handle(
        &self,
        command: RevokeAuditorSession,
        _metadata: ExecutionMetadata,
    ) -> Result<RevokeAuditorSessionResult, RevokeAuditorSessionError> {
        let digest = Sha256Digest::digest(command.raw_session.expose_secret().as_bytes());
        self.repository
            .in_transaction(async move |context| {
                let Some(mut session) = context.auditor_sessions().get(digest).await? else {
                    return Ok(None);
                };
                let result = session.revoke(Utc::now()).map_err(|_| {
                    crate::repository::Error::InvariantViolation(
                        "auditor session revocation is invalid",
                    )
                })?;
                context.auditor_sessions().save(&session).await?;
                Ok(Some(result))
            })
            .await?
            .map(|value| match value {
                crate::domain::AuditorSessionTransition::Revoked => {
                    Ok(RevokeAuditorSessionResult::Revoked)
                }
                crate::domain::AuditorSessionTransition::AlreadyRevoked => {
                    Ok(RevokeAuditorSessionResult::AlreadyRevoked)
                }
                crate::domain::AuditorSessionTransition::Used => {
                    Err(RevokeAuditorSessionError::Repository(
                        crate::repository::Error::InvariantViolation(
                            "session revocation returned an invalid transition",
                        ),
                    ))
                }
            })
            .transpose()?
            .ok_or(RevokeAuditorSessionError::Unavailable)
    }
}
#[derive(Debug, thiserror::Error)]
pub enum RevokeAuditorSessionError {
    #[error("auditor access is unavailable")]
    Unavailable,
    #[error("repository error")]
    Repository(#[from] crate::repository::Error),
}
