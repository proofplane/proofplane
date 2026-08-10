use crate::{
    application::ExecutionMetadata,
    domain::{AuditorAccessGrantId, AuditorAuthTransactionId, Sha256Digest},
    persistence::Postgres,
};
use chrono::Utc;
use secrecy::{ExposeSecret, SecretString};
use std::sync::Arc;
#[derive(Debug)]
pub struct ClaimAuditorAuthTransaction {
    pub state: SecretString,
}
#[derive(Debug)]
pub struct ClaimedAuditorAuthTransaction {
    pub id: AuditorAuthTransactionId,
    pub grant_id: AuditorAccessGrantId,
    pub nonce_digest: Sha256Digest,
    pub pkce_verifier: SecretString,
    pub expires_at: chrono::DateTime<Utc>,
    pub consumed_at: chrono::DateTime<Utc>,
}
#[derive(Clone)]
pub struct ClaimAuditorAuthTransactionHandler {
    repository: Arc<Postgres>,
}
impl ClaimAuditorAuthTransactionHandler {
    pub fn new(repository: Arc<Postgres>) -> Self {
        Self { repository }
    }
    pub async fn handle(
        &self,
        command: ClaimAuditorAuthTransaction,
        _metadata: ExecutionMetadata,
    ) -> Result<ClaimedAuditorAuthTransaction, ClaimAuditorAuthTransactionError> {
        if command.state.expose_secret().trim().is_empty() {
            return Err(ClaimAuditorAuthTransactionError::Unavailable);
        }
        let digest = Sha256Digest::digest(command.state.expose_secret().as_bytes());
        self.repository
            .in_unit_of_work(async move |unit_of_work| {
                let Some(transaction_id) = unit_of_work
                    .reads()
                    .auditor_auth_transactions()
                    .resolve_id_by_state_digest(digest)
                    .await?
                else {
                    return Ok(None);
                };
                let Some(mut transaction) = unit_of_work
                    .auditor_auth_transactions()
                    .get(transaction_id)
                    .await?
                else {
                    return Ok(None);
                };
                if transaction.claim(Utc::now()).is_err() {
                    return Ok(None);
                }
                unit_of_work
                    .auditor_auth_transactions()
                    .save(&transaction)
                    .await?;
                Ok(Some(ClaimedAuditorAuthTransaction {
                    id: transaction.id(),
                    grant_id: transaction.grant_id(),
                    nonce_digest: transaction.nonce_digest(),
                    pkce_verifier: transaction.pkce_verifier().clone(),
                    expires_at: transaction.expires_at(),
                    consumed_at: transaction.consumed_at().ok_or(
                        crate::persistence::Error::InvariantViolation(
                            "claimed transaction must record consumption",
                        ),
                    )?,
                }))
            })
            .await?
            .ok_or(ClaimAuditorAuthTransactionError::Unavailable)
    }
}
#[derive(Debug, thiserror::Error)]
pub enum ClaimAuditorAuthTransactionError {
    #[error("auditor authentication transaction is unavailable")]
    Unavailable,
    #[error("repository error")]
    Repository(#[from] crate::persistence::Error),
}
