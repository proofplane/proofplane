use std::sync::Arc;

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};
use thiserror::Error;
use uuid::Uuid;

use crate::{
    domain::{AuditorAccessGrant, AuditorAccessOtpId, AuditorSession, AuditorSessionId},
    mailer::{AuditorOtpMail, MailError, SharedMailAdapter},
    repository::{NewAuditorAccessOtp, NewAuditorSession, Postgres},
};

const OTP_TTL_MINUTES: i64 = 10;
const SESSION_TTL_DAYS: i64 = 7;
const MAX_OTP_SENDS: i64 = 3;

#[derive(Clone)]
pub struct AuditorAccessSessionService {
    repository: Arc<Postgres>,
    mailer: SharedMailAdapter,
}

#[derive(Debug, Error)]
pub enum AuditorAccessSessionError {
    #[error("auditor access is unavailable")]
    Unavailable,
    #[error("too many auditor OTP requests")]
    RateLimited,
    #[error("auditor mail delivery failed")]
    Mail(#[from] MailError),
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
    pub fn new(repository: Arc<Postgres>, mailer: SharedMailAdapter) -> Self {
        Self { repository, mailer }
    }

    pub async fn request_otp(
        &self,
        grant: &AuditorAccessGrant,
    ) -> Result<(), AuditorAccessSessionError> {
        if self.repository.recent_auditor_otp_count(grant.id).await? >= MAX_OTP_SENDS {
            return Err(AuditorAccessSessionError::RateLimited);
        }

        let code = generate_otp_code()?;
        let otp_id = AuditorAccessOtpId::from(Uuid::new_v4());
        self.repository
            .create_auditor_otp(NewAuditorAccessOtp {
                id: otp_id,
                grant_id: grant.id,
                code_digest: digest(&code),
                expires_at: Utc::now() + chrono::Duration::minutes(OTP_TTL_MINUTES),
            })
            .await?;
        if let Err(error) = self
            .mailer
            .send_auditor_otp(&AuditorOtpMail {
                id: Uuid::from(otp_id),
                auditor_email: &grant.auditor_email,
                code: &code,
            })
            .await
        {
            self.repository.delete_auditor_otp(otp_id).await?;
            return Err(error.into());
        }

        Ok(())
    }

    pub async fn verify_otp(
        &self,
        grant: &AuditorAccessGrant,
        code: &str,
    ) -> Result<CreatedAuditorSession, AuditorAccessSessionError> {
        if code.len() != 6 || !code.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(AuditorAccessSessionError::Unavailable);
        }

        let raw_session = generate_session_token()?;
        let session = self
            .repository
            .verify_auditor_otp_and_create_session(
                grant.id,
                digest(code),
                NewAuditorSession {
                    id: AuditorSessionId::from(Uuid::new_v4()),
                    grant_id: grant.id,
                    session_digest: digest(&raw_session),
                    auditor_email: grant.auditor_email.clone(),
                    expires_at: seven_days_from_now(),
                },
            )
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

fn generate_otp_code() -> Result<String, AuditorAccessSessionError> {
    const BOUND: u32 = 1_000_000 * (u32::MAX / 1_000_000);
    loop {
        let mut bytes = [0; 4];
        getrandom::fill(&mut bytes).map_err(|_| AuditorAccessSessionError::Random)?;
        let value = u32::from_le_bytes(bytes);
        if value < BOUND {
            return Ok(format!("{:06}", value % 1_000_000));
        }
    }
}

fn generate_session_token() -> Result<String, AuditorAccessSessionError> {
    let mut bytes = [0; 32];
    getrandom::fill(&mut bytes).map_err(|_| AuditorAccessSessionError::Random)?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_otp_is_six_digits() {
        let code = generate_otp_code().unwrap();
        assert_eq!(code.len(), 6);
        assert!(code.bytes().all(|byte| byte.is_ascii_digit()));
    }
}
