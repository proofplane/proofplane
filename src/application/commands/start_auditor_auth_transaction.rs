use crate::{
    application::ExecutionMetadata,
    config::Auth0AuditorPortalConfig,
    domain::{
        AuditorAccessGrantId, AuditorAuthTransaction, AuditorAuthTransactionId, Sha256Digest,
        WorkspaceId,
    },
    persistence::Postgres,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::{Duration, Utc};
use secrecy::{ExposeSecret, SecretString};
use sha2::{Digest, Sha256};
use std::{fmt, sync::Arc};
use url::Url;
use uuid::Uuid;
const TRANSACTION_TTL_MINUTES: i64 = 10;
#[derive(Debug, Clone, Copy)]
pub struct StartAuditorAuthTransaction {
    pub workspace_id: WorkspaceId,
    pub grant_id: AuditorAccessGrantId,
}
pub struct StartedAuditorAuthTransaction {
    pub transaction_id: AuditorAuthTransactionId,
    redirect_url: Url,
}
impl StartedAuditorAuthTransaction {
    pub fn redirect_url(&self) -> &Url {
        &self.redirect_url
    }
    pub fn into_redirect_url(self) -> Url {
        self.redirect_url
    }
}
impl fmt::Debug for StartedAuditorAuthTransaction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StartedAuditorAuthTransaction")
            .field("transaction_id", &self.transaction_id)
            .field("redirect_url", &"[redacted]")
            .finish()
    }
}
#[derive(Clone)]
pub struct StartAuditorAuthTransactionHandler {
    repository: Arc<Postgres>,
    config: Auth0AuditorPortalConfig,
}
impl StartAuditorAuthTransactionHandler {
    pub fn new(repository: Arc<Postgres>, config: Auth0AuditorPortalConfig) -> Self {
        Self { repository, config }
    }
    pub async fn handle(
        &self,
        command: StartAuditorAuthTransaction,
        _metadata: ExecutionMetadata,
    ) -> Result<StartedAuditorAuthTransaction, StartAuditorAuthTransactionError> {
        let state = random_secret()?;
        let nonce = random_secret()?;
        let verifier = random_secret()?;
        let state_digest = Sha256Digest::digest(state.expose_secret().as_bytes());
        let nonce_digest = Sha256Digest::digest(nonce.expose_secret().as_bytes());
        let persisted_verifier = verifier.clone();
        let now = Utc::now();
        let expires_at = now + Duration::minutes(TRANSACTION_TTL_MINUTES);
        let transaction_id = AuditorAuthTransactionId::from(Uuid::new_v4());
        let email = self
            .repository
            .in_unit_of_work(async move |unit_of_work| {
                let workspace = unit_of_work.workspace(command.workspace_id);
                let Some(grant) = workspace
                    .reads()
                    .auditor_access_grants()
                    .get_active(command.grant_id, now)
                    .await?
                else {
                    return Ok(None);
                };
                let transaction = AuditorAuthTransaction::start(
                    transaction_id,
                    grant.id,
                    state_digest,
                    nonce_digest,
                    persisted_verifier,
                    now,
                    expires_at,
                )
                .map_err(|_| {
                    crate::persistence::Error::InvariantViolation(
                        "auditor authentication transaction creation is invalid",
                    )
                })?;
                unit_of_work
                    .auditor_auth_transactions()
                    .save(&transaction)
                    .await?;
                Ok(Some(grant.auditor_email))
            })
            .await?
            .ok_or(StartAuditorAuthTransactionError::Unavailable)?;
        Ok(StartedAuditorAuthTransaction {
            transaction_id,
            redirect_url: authorization_url(
                &self.config,
                state.expose_secret(),
                nonce.expose_secret(),
                &pkce_challenge(verifier.expose_secret()),
                &email,
            ),
        })
    }
}
fn random_secret() -> Result<SecretString, StartAuditorAuthTransactionError> {
    let mut bytes = [0_u8; 32];
    getrandom::fill(&mut bytes).map_err(|_| StartAuditorAuthTransactionError::Random)?;
    Ok(SecretString::from(URL_SAFE_NO_PAD.encode(bytes)))
}
fn pkce_challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}
fn authorization_url(
    config: &Auth0AuditorPortalConfig,
    state: &str,
    nonce: &str,
    challenge: &str,
    login_hint: &str,
) -> Url {
    let mut url = config.authorization_endpoint.clone();
    url.query_pairs_mut()
        .clear()
        .append_pair("client_id", &config.client_id)
        .append_pair("redirect_uri", config.callback_url.as_str())
        .append_pair("response_type", "code")
        .append_pair("scope", "openid email")
        .append_pair("state", state)
        .append_pair("nonce", nonce)
        .append_pair("code_challenge", challenge)
        .append_pair("code_challenge_method", "S256")
        .append_pair("connection", &config.connection)
        .append_pair("login_hint", login_hint)
        .append_pair("prompt", "login");
    url
}
#[derive(Debug, thiserror::Error)]
pub enum StartAuditorAuthTransactionError {
    #[error("auditor authentication transaction is unavailable")]
    Unavailable,
    #[error("random generation failed")]
    Random,
    #[error("repository error")]
    Repository(#[from] crate::persistence::Error),
}
#[cfg(test)]
mod tests {
    use super::pkce_challenge;
    #[test]
    fn pkce_challenge_is_rfc7636_sha256_base64url() {
        assert_eq!(
            pkce_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }
}
