use std::sync::Arc;

use crate::{
    application::{
        commands::provision_user::{ProvisionUser, ProvisionUserError, ProvisionUserHandler},
        ExecutionMetadata,
    },
    authentication::auth0::{TokenVerifier, VerifiedClaims, VerifyError},
    domain::{AgentConnectionId, UserId, WorkspaceId, WorkspacePermissions},
    persistence,
};

pub mod auth0;
pub mod client_registration;
mod jwks;
pub mod opaque_token;
pub mod paseto;

/// Authenticated machine scope supplied to application operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AgentConnectionContext {
    pub user_id: UserId,
    pub connection_id: AgentConnectionId,
    pub workspace_id: WorkspaceId,
    pub permissions: WorkspacePermissions,
}

/// An authenticated human management-plane identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserContext {
    pub user_id: UserId,
    pub auth0_sub: String,
}

impl UserContext {
    pub fn new(user_id: UserId, auth0_sub: String) -> Self {
        Self { user_id, auth0_sub }
    }
}

pub struct UserAuthenticator<V: TokenVerifier<Claims = VerifiedClaims>> {
    verifier: Arc<V>,
    provision_user: ProvisionUserHandler,
}

impl<V: TokenVerifier<Claims = VerifiedClaims>> Clone for UserAuthenticator<V> {
    fn clone(&self) -> Self {
        Self {
            verifier: self.verifier.clone(),
            provision_user: self.provision_user.clone(),
        }
    }
}

impl<V: TokenVerifier<Claims = VerifiedClaims>> UserAuthenticator<V> {
    pub fn new(verifier: Arc<V>, repository: Arc<persistence::Postgres>) -> Self {
        Self {
            verifier,
            provision_user: ProvisionUserHandler::new(repository),
        }
    }

    pub async fn authenticate(&self, token: &str) -> Result<UserContext, AuthError> {
        let claims = self
            .verifier
            .verify(token)
            .await
            .map_err(AuthError::from_verify)?;

        let user = self
            .provision_user
            .handle(
                ProvisionUser {
                    auth0_sub: claims.sub,
                    email: claims.email,
                    name: claims.name,
                },
                ExecutionMetadata::background(),
            )
            .await
            .map_err(|error| match error {
                ProvisionUserError::Repository(error) => AuthError::Repository(error),
            })?;

        Ok(UserContext::new(user.id, user.auth0_sub))
    }
}

#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("token rejected")]
    Unauthorized(#[source] VerifyError),
    #[error("token verifier unavailable")]
    VerifierUnavailable(#[source] VerifyError),
    #[error("user provisioning failed")]
    Repository(#[source] persistence::Error),
}

impl AuthError {
    fn from_verify(error: VerifyError) -> Self {
        if error.is_token_rejection() {
            return Self::Unauthorized(error);
        }

        Self::VerifierUnavailable(error)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("credential repository error")]
    Repository(#[source] persistence::Error),
    #[error("PASETO initialization failed")]
    Paseto(#[from] paseto::Error),
}

#[cfg(test)]
mod tests {}
