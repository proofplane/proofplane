use crate::{
    application::ExecutionMetadata,
    domain::{AuditorSession, AuditorSessionTransition, Sha256Digest},
    persistence::Postgres,
};
use chrono::Utc;
use secrecy::{ExposeSecret, SecretString};
use std::sync::Arc;

#[derive(Debug)]
pub struct RevokeAuditorSession {
    pub raw_session: SecretString,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RevokeAuditorSessionResult {
    pub session: AuditorSession,
    pub transition: AuditorSessionTransition,
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
            .in_unit_of_work(async move |unit_of_work| {
                let Some(session_id) = unit_of_work
                    .reads()
                    .auditor_sessions()
                    .resolve_id_by_digest(digest)
                    .await?
                else {
                    return Ok(None);
                };
                let Some(mut session) = unit_of_work
                    .aggregates()
                    .auditor_sessions()
                    .get(session_id)
                    .await?
                else {
                    return Ok(None);
                };
                let result = session.revoke(Utc::now()).map_err(|_| {
                    crate::persistence::Error::InvariantViolation(
                        "auditor session revocation is invalid",
                    )
                })?;
                unit_of_work
                    .aggregates()
                    .auditor_sessions()
                    .save(&session)
                    .await?;
                Ok(Some(RevokeAuditorSessionResult {
                    session,
                    transition: result,
                }))
            })
            .await?
            .map(|result| match result.transition {
                AuditorSessionTransition::Revoked | AuditorSessionTransition::AlreadyRevoked => {
                    Ok(result)
                }
                AuditorSessionTransition::Used => Err(RevokeAuditorSessionError::Repository(
                    crate::persistence::Error::InvariantViolation(
                        "session revocation returned an invalid transition",
                    ),
                )),
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
    Repository(#[from] crate::persistence::Error),
}
