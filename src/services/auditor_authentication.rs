use std::sync::Arc;

use secrecy::SecretString;
use thiserror::Error;
use uuid::Uuid;

use crate::{
    authentication::auth0::{
        AuditorIdentityExchange, AuditorIdentityProviderError, SharedAuditorIdentityProvider,
        VerifiedAuditorIdentity,
    },
    config::Auth0AuditorPortalConfig,
    domain::{AuditorAccessGrant, AuditorAuthTransactionId},
    repository::Postgres,
};

use super::{
    auditor_access_grants::normalize_email,
    auditor_access_sessions::{
        AuditorAccessSessionError, AuditorAccessSessionService, CreatedAuditorSession,
    },
    auditor_auth_transactions::{AuditorAuthTransactionError, AuditorAuthTransactionService},
};

#[derive(Clone)]
pub struct AuditorAuthenticationService {
    repository: Arc<Postgres>,
    transactions: AuditorAuthTransactionService,
    sessions: AuditorAccessSessionService,
    identity_provider: SharedAuditorIdentityProvider,
    config: Auth0AuditorPortalConfig,
}

pub struct CompletedAuditorAuthentication {
    pub transaction_id: AuditorAuthTransactionId,
    pub grant: AuditorAccessGrant,
    pub identity: VerifiedAuditorIdentity,
    pub created: CreatedAuditorSession,
}

#[derive(Debug, Error)]
pub enum AuditorAuthenticationError {
    #[error("auditor authentication was rejected")]
    Rejected,
    #[error("auditor authentication grant is unavailable")]
    GrantUnavailable,
    #[error("auditor identity provider is unavailable")]
    ProviderUnavailable,
    #[error("auditor authentication persistence is unavailable")]
    PersistenceUnavailable,
}

impl AuditorAuthenticationService {
    pub fn new(
        repository: Arc<Postgres>,
        transactions: AuditorAuthTransactionService,
        sessions: AuditorAccessSessionService,
        identity_provider: SharedAuditorIdentityProvider,
        config: Auth0AuditorPortalConfig,
    ) -> Self {
        Self {
            repository,
            transactions,
            sessions,
            identity_provider,
            config,
        }
    }

    pub async fn complete(
        &self,
        state: &str,
        authorization_code: &str,
    ) -> Result<CompletedAuditorAuthentication, AuditorAuthenticationError> {
        if authorization_code.trim().is_empty() {
            return Err(AuditorAuthenticationError::Rejected);
        }

        let claimed = self
            .transactions
            .claim(state)
            .await
            .map_err(|error| match error {
                AuditorAuthTransactionError::Unavailable => AuditorAuthenticationError::Rejected,
                AuditorAuthTransactionError::Random
                | AuditorAuthTransactionError::Repository(_) => {
                    AuditorAuthenticationError::PersistenceUnavailable
                }
            })?;
        let transaction_id = claimed.id;
        let grant_id = claimed.grant_id;

        let identity = self
            .identity_provider
            .exchange_and_verify(AuditorIdentityExchange {
                authorization_code: SecretString::from(authorization_code.to_owned()),
                redirect_uri: self.config.callback_url.clone(),
                pkce_verifier: claimed.pkce_verifier,
                expected_nonce_digest: claimed.nonce_digest,
            })
            .await
            .map_err(|error| match error {
                AuditorIdentityProviderError::Rejected => AuditorAuthenticationError::Rejected,
                AuditorIdentityProviderError::Unavailable => {
                    AuditorAuthenticationError::ProviderUnavailable
                }
            })?;

        let grant = self
            .repository
            .get_active_auditor_access_grant_by_id(grant_id)
            .await
            .map_err(|_| AuditorAuthenticationError::PersistenceUnavailable)?
            .ok_or(AuditorAuthenticationError::GrantUnavailable)?;
        let normalized_identity_email =
            normalize_email(&identity.email).map_err(|_| AuditorAuthenticationError::Rejected)?;
        if !identity.email_verified || normalized_identity_email != grant.auditor_email {
            return Err(AuditorAuthenticationError::Rejected);
        }

        let created = self
            .sessions
            .create_auth0_session(&grant, identity.subject.clone())
            .await
            .map_err(|error| match error {
                AuditorAccessSessionError::Unavailable => {
                    AuditorAuthenticationError::GrantUnavailable
                }
                AuditorAccessSessionError::Random => {
                    AuditorAuthenticationError::PersistenceUnavailable
                }
                AuditorAccessSessionError::Repository(_) => {
                    AuditorAuthenticationError::PersistenceUnavailable
                }
            })?;

        Ok(CompletedAuditorAuthentication {
            transaction_id,
            grant,
            identity,
            created,
        })
    }
}

impl std::fmt::Debug for CompletedAuditorAuthentication {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CompletedAuditorAuthentication")
            .field("transaction_id", &Uuid::from(self.transaction_id))
            .field("grant_id", &Uuid::from(self.grant.id))
            .field("session_id", &Uuid::from(self.created.session.id))
            .finish_non_exhaustive()
    }
}
