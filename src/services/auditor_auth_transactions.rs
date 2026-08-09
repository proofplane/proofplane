use std::{fmt, sync::Arc};

use thiserror::Error;
use url::Url;

use crate::{
    application::{
        commands::{
            claim_auditor_auth_transaction::{
                ClaimAuditorAuthTransaction, ClaimAuditorAuthTransactionHandler,
                ClaimedAuditorAuthTransaction,
            },
            start_auditor_auth_transaction::{
                StartAuditorAuthTransaction, StartAuditorAuthTransactionHandler,
            },
        },
        ExecutionMetadata,
    },
    config::Auth0AuditorPortalConfig,
    domain::{AuditorAccessGrant, AuditorAuthTransactionId},
    repository::Postgres,
};

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
        let start =
            StartAuditorAuthTransactionHandler::new(self.repository.clone(), self.config.clone())
                .handle(
                    StartAuditorAuthTransaction {
                        workspace_id: grant.workspace_id,
                        grant_id: grant.id,
                    },
                    ExecutionMetadata::background(),
                )
                .await
                .map_err(map_start_error)?;
        Ok(AuthorizationStart {
            transaction_id: start.transaction_id,
            redirect_url: start.into_redirect_url(),
        })
    }

    pub async fn claim(
        &self,
        state: &str,
    ) -> Result<ClaimedAuditorAuthTransaction, AuditorAuthTransactionError> {
        if state.trim().is_empty() {
            return Err(AuditorAuthTransactionError::Unavailable);
        }

        let claimed = ClaimAuditorAuthTransactionHandler::new(self.repository.clone())
            .handle(
                ClaimAuditorAuthTransaction {
                    state: secrecy::SecretString::from(state.to_owned()),
                },
                ExecutionMetadata::background(),
            )
            .await
            .map_err(map_claim_error)?;
        Ok(ClaimedAuditorAuthTransaction {
            id: claimed.id,
            grant_id: claimed.grant_id,
            nonce_digest: claimed.nonce_digest,
            pkce_verifier: claimed.pkce_verifier,
            expires_at: claimed.expires_at,
            consumed_at: claimed.consumed_at,
        })
    }
}

fn map_start_error(
    error: crate::application::commands::start_auditor_auth_transaction::StartAuditorAuthTransactionError,
) -> AuditorAuthTransactionError {
    match error { crate::application::commands::start_auditor_auth_transaction::StartAuditorAuthTransactionError::Unavailable => AuditorAuthTransactionError::Unavailable, crate::application::commands::start_auditor_auth_transaction::StartAuditorAuthTransactionError::Random => AuditorAuthTransactionError::Random, crate::application::commands::start_auditor_auth_transaction::StartAuditorAuthTransactionError::Repository(error) => AuditorAuthTransactionError::Repository(error) }
}
fn map_claim_error(
    error: crate::application::commands::claim_auditor_auth_transaction::ClaimAuditorAuthTransactionError,
) -> AuditorAuthTransactionError {
    match error { crate::application::commands::claim_auditor_auth_transaction::ClaimAuditorAuthTransactionError::Unavailable => AuditorAuthTransactionError::Unavailable, crate::application::commands::claim_auditor_auth_transaction::ClaimAuditorAuthTransactionError::Repository(error) => AuditorAuthTransactionError::Repository(error) }
}
