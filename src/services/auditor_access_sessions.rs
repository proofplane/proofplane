use std::sync::Arc;

use crate::application::{
    commands::create_authenticated_auditor_session::{
        CreateAuthenticatedAuditorSession, CreateAuthenticatedAuditorSessionError,
        CreateAuthenticatedAuditorSessionHandler,
    },
    ExecutionMetadata,
};
use chrono::Utc;
use thiserror::Error;

use crate::{
    domain::{AuditorAccessGrant, AuditorSession, Sha256Digest},
    repository::Postgres,
};

#[derive(Clone)]
pub struct AuditorAccessSessionService {
    repository: Arc<Postgres>,
}

#[derive(Debug, Error)]
pub enum AuditorAccessSessionError {
    #[error("auditor access is unavailable")]
    Unavailable,
    #[error("random generation failed")]
    Random,
    #[error("repository error")]
    Repository(#[from] crate::repository::Error),
}

#[derive(Debug)]
pub struct CreatedAuditorSession {
    pub session: AuditorSession,
    pub raw_session: String,
}

impl AuditorAccessSessionService {
    pub fn new(repository: Arc<Postgres>) -> Self {
        Self { repository }
    }

    pub async fn create_auth0_session(
        &self,
        grant: &AuditorAccessGrant,
        auth0_subject: String,
    ) -> Result<CreatedAuditorSession, AuditorAccessSessionError> {
        if auth0_subject.trim().is_empty() {
            return Err(AuditorAccessSessionError::Unavailable);
        }

        let created = CreateAuthenticatedAuditorSessionHandler::new(self.repository.clone())
            .handle(
                CreateAuthenticatedAuditorSession {
                    workspace_id: grant.workspace_id,
                    grant_id: grant.id,
                    auth0_subject,
                },
                ExecutionMetadata::background(),
            )
            .await
            .map_err(map_create_error)?;
        Ok(CreatedAuditorSession {
            session: created.session,
            raw_session: created.raw_session,
        })
    }

    pub async fn load_session(
        &self,
        raw_session: &str,
    ) -> Result<AuditorSession, AuditorAccessSessionError> {
        let digest = Sha256Digest::digest(raw_session.as_bytes());
        let now = Utc::now();
        self.repository
            .in_transaction(async move |context| {
                let Some(mut session) = context.auditor_sessions().get(digest).await? else {
                    return Ok(None);
                };
                if session.touch(now).is_err() {
                    return Ok(None);
                }
                context.auditor_sessions().save(&session).await?;
                Ok(Some(session))
            })
            .await?
            .ok_or(AuditorAccessSessionError::Unavailable)
    }

    pub async fn revoke_session(
        &self,
        raw_session: &str,
    ) -> Result<Option<AuditorSession>, AuditorAccessSessionError> {
        let digest = Sha256Digest::digest(raw_session.as_bytes());
        let now = Utc::now();
        self.repository
            .in_transaction(async move |context| {
                let Some(mut session) = context.auditor_sessions().get(digest).await? else {
                    return Ok(None);
                };
                let _ = session.revoke(now).map_err(|_| {
                    crate::repository::Error::InvariantViolation(
                        "auditor session revocation is invalid",
                    )
                })?;
                context.auditor_sessions().save(&session).await?;
                Ok(Some(session))
            })
            .await
            .map_err(Into::into)
    }
}

fn map_create_error(error: CreateAuthenticatedAuditorSessionError) -> AuditorAccessSessionError {
    match error {
        CreateAuthenticatedAuditorSessionError::Unavailable => {
            AuditorAccessSessionError::Unavailable
        }
        CreateAuthenticatedAuditorSessionError::Random => AuditorAccessSessionError::Random,
        CreateAuthenticatedAuditorSessionError::Repository(error) => {
            AuditorAccessSessionError::Repository(error)
        }
    }
}
