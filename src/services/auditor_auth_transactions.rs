use std::{fmt, sync::Arc};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::{DateTime, Utc};
use secrecy::{ExposeSecret, SecretString};
use sha2::{Digest, Sha256};
use thiserror::Error;
use url::Url;
use uuid::Uuid;

use crate::{
    config::Auth0AuditorPortalConfig,
    domain::{AuditorAccessGrant, AuditorAuthTransactionId, Sha256Digest},
    repository::{ClaimedAuditorAuthTransaction, NewAuditorAuthTransaction, Postgres},
};

const TRANSACTION_TTL_MINUTES: i64 = 10;
const RANDOM_VALUE_BYTES: usize = 32;

#[derive(Clone)]
pub struct AuditorAuthTransactionService {
    repository: Arc<Postgres>,
    config: Auth0AuditorPortalConfig,
}

pub struct AuthorizationStart {
    pub transaction_id: AuditorAuthTransactionId,
    redirect_url: Url,
}

impl AuthorizationStart {
    pub fn redirect_url(&self) -> &Url {
        &self.redirect_url
    }

    pub fn into_redirect_url(self) -> Url {
        self.redirect_url
    }
}

impl fmt::Debug for AuthorizationStart {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AuthorizationStart")
            .field("transaction_id", &self.transaction_id)
            .field("redirect_url", &"[redacted]")
            .finish()
    }
}

#[derive(Debug, Error)]
pub enum AuditorAuthTransactionError {
    #[error("auditor authentication transaction is unavailable")]
    Unavailable,
    #[error("auditor authentication random generation failed")]
    Random,
    #[error("repository error")]
    Repository(#[from] crate::repository::Error),
}

impl AuditorAuthTransactionService {
    pub fn new(repository: Arc<Postgres>, config: Auth0AuditorPortalConfig) -> Self {
        Self { repository, config }
    }

    pub async fn start(
        &self,
        grant: &AuditorAccessGrant,
    ) -> Result<AuthorizationStart, AuditorAuthTransactionError> {
        let state = random_secret()?;
        let nonce = random_secret()?;
        let pkce_verifier = random_secret()?;
        let transaction_id = AuditorAuthTransactionId::from(Uuid::new_v4());
        let expires_at = transaction_expires_at();

        let created = self
            .repository
            .create_auditor_auth_transaction(NewAuditorAuthTransaction {
                id: transaction_id,
                grant_id: grant.id,
                state_digest: digest_secret(&state),
                nonce_digest: digest_secret(&nonce),
                pkce_verifier: pkce_verifier.clone(),
                expires_at,
            })
            .await?;
        if !created {
            return Err(AuditorAuthTransactionError::Unavailable);
        }

        Ok(AuthorizationStart {
            transaction_id,
            redirect_url: self.authorization_url(
                state.expose_secret(),
                nonce.expose_secret(),
                pkce_challenge(pkce_verifier.expose_secret()),
                &grant.auditor_email,
            ),
        })
    }

    pub async fn claim(
        &self,
        state: &str,
    ) -> Result<ClaimedAuditorAuthTransaction, AuditorAuthTransactionError> {
        if state.trim().is_empty() {
            return Err(AuditorAuthTransactionError::Unavailable);
        }

        self.repository
            .claim_auditor_auth_transaction(Sha256Digest::digest(state.as_bytes()))
            .await?
            .ok_or(AuditorAuthTransactionError::Unavailable)
    }

    fn authorization_url(
        &self,
        state: &str,
        nonce: &str,
        code_challenge: String,
        login_hint: &str,
    ) -> Url {
        let mut url = self.config.authorization_endpoint.clone();
        url.query_pairs_mut()
            .clear()
            .append_pair("client_id", &self.config.client_id)
            .append_pair("redirect_uri", self.config.callback_url.as_str())
            .append_pair("response_type", "code")
            .append_pair("scope", "openid email")
            .append_pair("state", state)
            .append_pair("nonce", nonce)
            .append_pair("code_challenge", &code_challenge)
            .append_pair("code_challenge_method", "S256")
            .append_pair("connection", &self.config.connection)
            .append_pair("login_hint", login_hint)
            .append_pair("prompt", "login");
        url
    }
}

fn transaction_expires_at() -> DateTime<Utc> {
    Utc::now() + chrono::Duration::minutes(TRANSACTION_TTL_MINUTES)
}

fn random_secret() -> Result<SecretString, AuditorAuthTransactionError> {
    let mut bytes = [0_u8; RANDOM_VALUE_BYTES];
    getrandom::fill(&mut bytes).map_err(|_| AuditorAuthTransactionError::Random)?;
    Ok(SecretString::from(URL_SAFE_NO_PAD.encode(bytes)))
}

fn digest_secret(secret: &SecretString) -> Sha256Digest {
    Sha256Digest::digest(secret.expose_secret().as_bytes())
}

fn pkce_challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_protocol_secrets_are_rfc7636_length_and_independent() {
        let first = random_secret().unwrap();
        let second = random_secret().unwrap();

        assert_eq!(first.expose_secret().len(), 43);
        assert_eq!(second.expose_secret().len(), 43);
        assert_ne!(first.expose_secret(), second.expose_secret());
    }

    #[test]
    fn pkce_challenge_uses_sha256_and_base64url_without_padding() {
        assert_eq!(
            pkce_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }
}
