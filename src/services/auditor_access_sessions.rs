use std::sync::Arc;

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};
use thiserror::Error;
use uuid::Uuid;

use crate::{
    domain::{AuditorAccessGrant, AuditorSession, AuditorSessionId},
    repository::{NewAuditorSession, Postgres},
};

const SESSION_TTL_DAYS: i64 = 7;

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

        let raw_session = generate_session_token()?;
        let session = self
            .repository
            .create_active_auditor_session(NewAuditorSession {
                id: AuditorSessionId::from(Uuid::new_v4()),
                grant_id: grant.id,
                session_digest: digest(&raw_session),
                auditor_email: grant.auditor_email.clone(),
                auth0_subject,
                expires_at: seven_days_from_now(),
            })
            .await?
            .ok_or(AuditorAccessSessionError::Unavailable)?;

        Ok(CreatedAuditorSession {
            session,
            raw_session,
        })
    }

    pub async fn load_session(
        &self,
        raw_session: &str,
    ) -> Result<AuditorSession, AuditorAccessSessionError> {
        self.repository
            .get_active_auditor_session_by_digest(digest(raw_session))
            .await?
            .ok_or(AuditorAccessSessionError::Unavailable)
    }

    pub async fn revoke_session(
        &self,
        raw_session: &str,
    ) -> Result<Option<AuditorSession>, AuditorAccessSessionError> {
        Ok(self
            .repository
            .revoke_auditor_session_by_digest(digest(raw_session))
            .await?)
    }
}

fn seven_days_from_now() -> DateTime<Utc> {
    Utc::now() + chrono::Duration::days(SESSION_TTL_DAYS)
}

fn digest(value: &str) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    hasher.finalize().into()
}

fn generate_session_token() -> Result<String, AuditorAccessSessionError> {
    let mut bytes = [0; 32];
    getrandom::fill(&mut bytes).map_err(|_| AuditorAccessSessionError::Random)?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}
