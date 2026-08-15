use std::sync::Arc;

use serde_json::Value;

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
    pub verified_management_identity: Option<VerifiedManagementIdentity>,
}

impl UserContext {
    pub fn new(
        user_id: UserId,
        auth0_sub: String,
        verified_management_identity: Option<VerifiedManagementIdentity>,
    ) -> Self {
        Self {
            user_id,
            auth0_sub,
            verified_management_identity,
        }
    }

    /// Returns the email authority required by operations that bind a user to a mailbox.
    pub fn require_verified_management_identity(
        &self,
    ) -> Result<&VerifiedManagementIdentity, VerifiedManagementIdentityError> {
        self.verified_management_identity
            .as_ref()
            .ok_or(VerifiedManagementIdentityError::Unavailable)
    }
}

/// A mailbox identity asserted by Auth0 and verified for management-plane use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedManagementIdentity {
    pub email: String,
}

impl VerifiedManagementIdentity {
    pub(crate) fn from_auth0_claims(
        email: Option<&Value>,
        email_verified: Option<&Value>,
    ) -> Option<Self> {
        (email_verified == Some(&Value::Bool(true)))
            .then(|| email?.as_str())
            .flatten()
            .and_then(normalize_email)
            .map(|email| Self { email })
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum VerifiedManagementIdentityError {
    #[error("verified management identity is unavailable")]
    Unavailable,
}

/// Normalizes the mailbox format accepted wherever Proofplane uses email authority.
pub fn normalize_email(value: &str) -> Option<String> {
    let email = value.trim().to_ascii_lowercase();
    email_address::EmailAddress::is_valid(&email).then_some(email)
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

        Ok(UserContext::new(
            user.id,
            user.auth0_sub,
            claims.verified_management_identity,
        ))
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
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn verified_management_identity_normalizes_only_verified_mailboxes() {
        let identity = VerifiedManagementIdentity::from_auth0_claims(
            Some(&Value::String("  Member@Example.COM ".to_owned())),
            Some(&Value::Bool(true)),
        );

        assert_eq!(
            identity,
            Some(VerifiedManagementIdentity {
                email: "member@example.com".to_owned(),
            })
        );

        for (email, verified) in [
            (None, Some(Value::Bool(true))),
            (
                Some(Value::String("   ".to_owned())),
                Some(Value::Bool(true)),
            ),
            (
                Some(Value::String("member".to_owned())),
                Some(Value::Bool(true)),
            ),
            (
                Some(Value::String("member @example.com".to_owned())),
                Some(Value::Bool(true)),
            ),
            (
                Some(Value::String("member@ example.com".to_owned())),
                Some(Value::Bool(true)),
            ),
            (
                Some(Value::String("member@@example.com".to_owned())),
                Some(Value::Bool(true)),
            ),
            (
                Some(Value::String("member@example.com".to_owned())),
                Some(Value::Bool(false)),
            ),
            (
                Some(Value::String("member@example.com".to_owned())),
                Some(Value::String("true".to_owned())),
            ),
            (Some(Value::Number(42.into())), Some(Value::Bool(true))),
        ] {
            assert_eq!(
                VerifiedManagementIdentity::from_auth0_claims(email.as_ref(), verified.as_ref()),
                None
            );
        }
    }

    #[test]
    fn context_only_rejects_missing_identity_when_an_operation_requests_it() {
        let context = UserContext::new(UserId::from(Uuid::new_v4()), "auth0|user".to_owned(), None);

        assert_eq!(
            context.require_verified_management_identity(),
            Err(VerifiedManagementIdentityError::Unavailable)
        );
    }
}
