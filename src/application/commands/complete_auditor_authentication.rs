use secrecy::SecretString;

use crate::{
    application::{
        commands::{
            claim_auditor_auth_transaction::{
                ClaimAuditorAuthTransaction, ClaimAuditorAuthTransactionError,
                ClaimAuditorAuthTransactionHandler,
            },
            create_authenticated_auditor_session::{
                CreateAuthenticatedAuditorSession, CreateAuthenticatedAuditorSessionError,
                CreateAuthenticatedAuditorSessionHandler, CreatedAuthenticatedAuditorSession,
            },
            issue_auditor_access_grant::normalize_email,
        },
        queries::resolve_active_auditor_grant::{
            ResolveActiveAuditorGrant, ResolveActiveAuditorGrantHandler, ResolvedActiveAuditorGrant,
        },
        ExecutionMetadata,
    },
    authentication::auth0::{
        AuditorIdentityExchange, AuditorIdentityProviderError, SharedAuditorIdentityProvider,
        VerifiedAuditorIdentity,
    },
    config::Auth0AuditorPortalConfig,
    domain::AuditorAuthTransactionId,
};

#[derive(Debug)]
pub struct CompleteAuditorAuthentication {
    pub state: SecretString,
    pub authorization_code: SecretString,
}

#[derive(Clone)]
pub struct CompleteAuditorAuthenticationHandler {
    claim_transaction: ClaimAuditorAuthTransactionHandler,
    resolve_grant: ResolveActiveAuditorGrantHandler,
    create_session: CreateAuthenticatedAuditorSessionHandler,
    identity_provider: SharedAuditorIdentityProvider,
    config: Auth0AuditorPortalConfig,
}

pub struct CompletedAuditorAuthentication {
    pub transaction_id: AuditorAuthTransactionId,
    pub grant: ResolvedActiveAuditorGrant,
    pub identity: VerifiedAuditorIdentity,
    pub created: CreatedAuthenticatedAuditorSession,
}

impl CompleteAuditorAuthenticationHandler {
    pub fn new(
        claim_transaction: ClaimAuditorAuthTransactionHandler,
        resolve_grant: ResolveActiveAuditorGrantHandler,
        create_session: CreateAuthenticatedAuditorSessionHandler,
        identity_provider: SharedAuditorIdentityProvider,
        config: Auth0AuditorPortalConfig,
    ) -> Self {
        Self {
            claim_transaction,
            resolve_grant,
            create_session,
            identity_provider,
            config,
        }
    }

    pub async fn handle(
        &self,
        command: CompleteAuditorAuthentication,
        metadata: ExecutionMetadata,
    ) -> Result<CompletedAuditorAuthentication, CompleteAuditorAuthenticationError> {
        if command.authorization_code.expose_secret().trim().is_empty() {
            return Err(CompleteAuditorAuthenticationError::Rejected);
        }
        let claimed = self
            .claim_transaction
            .handle(
                ClaimAuditorAuthTransaction {
                    state: command.state,
                },
                metadata,
            )
            .await
            .map_err(|error| match error {
                ClaimAuditorAuthTransactionError::Unavailable => {
                    CompleteAuditorAuthenticationError::Rejected
                }
                ClaimAuditorAuthTransactionError::Repository(_) => {
                    CompleteAuditorAuthenticationError::PersistenceUnavailable
                }
            })?;
        let transaction_id = claimed.id;

        let identity = self
            .identity_provider
            .exchange_and_verify(AuditorIdentityExchange {
                authorization_code: command.authorization_code,
                redirect_uri: self.config.callback_url.clone(),
                pkce_verifier: claimed.pkce_verifier,
                expected_nonce_digest: claimed.nonce_digest,
            })
            .await
            .map_err(|error| match error {
                AuditorIdentityProviderError::Rejected => {
                    CompleteAuditorAuthenticationError::Rejected
                }
                AuditorIdentityProviderError::Unavailable => {
                    CompleteAuditorAuthenticationError::ProviderUnavailable
                }
            })?;

        let grant = self
            .resolve_grant
            .handle(ResolveActiveAuditorGrant {
                grant_id: claimed.grant_id,
            })
            .await
            .map_err(|_| CompleteAuditorAuthenticationError::PersistenceUnavailable)?
            .ok_or(CompleteAuditorAuthenticationError::GrantUnavailable)?;
        let identity_email = normalize_email(&identity.email)
            .map_err(|_| CompleteAuditorAuthenticationError::Rejected)?;
        if !identity.email_verified || identity_email != grant.auditor_email {
            return Err(CompleteAuditorAuthenticationError::Rejected);
        }

        let created = self
            .create_session
            .handle(
                CreateAuthenticatedAuditorSession {
                    workspace_id: grant.workspace_id,
                    grant_id: grant.id,
                    auth0_subject: identity.subject.clone(),
                },
                metadata,
            )
            .await
            .map_err(|error| match error {
                CreateAuthenticatedAuditorSessionError::Unavailable => {
                    CompleteAuditorAuthenticationError::GrantUnavailable
                }
                CreateAuthenticatedAuditorSessionError::Random
                | CreateAuthenticatedAuditorSessionError::Repository(_) => {
                    CompleteAuditorAuthenticationError::PersistenceUnavailable
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

use secrecy::ExposeSecret;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum CompleteAuditorAuthenticationError {
    #[error("auditor authentication was rejected")]
    Rejected,
    #[error("auditor authentication grant is unavailable")]
    GrantUnavailable,
    #[error("auditor identity provider is unavailable")]
    ProviderUnavailable,
    #[error("auditor authentication persistence is unavailable")]
    PersistenceUnavailable,
}
