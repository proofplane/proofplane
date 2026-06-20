use std::sync::Arc;

use chrono::Utc;
use uuid::Uuid;

use crate::{
    authentication::auth0::{TokenVerifier, VerifyError},
    authentication::opaque_token::parse,
    domain::{
        ApiTokenId, ProvisionUserPayload, UserId, WorkspaceId, WorkspacePermission,
        WorkspacePermissions,
    },
    repository,
};

pub mod auth0;
pub mod opaque_token;
pub mod paseto;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ApiTokenContext {
    pub user_id: UserId,
    pub api_token_id: ApiTokenId,
    pub workspace_id: WorkspaceId,
    pub permissions: WorkspacePermissions,
}

impl ApiTokenContext {
    pub fn allows(
        &self,
        workspace_id: WorkspaceId,
        required_permission: WorkspacePermission,
    ) -> bool {
        self.workspace_id == workspace_id && self.permissions.has(required_permission)
    }
}

#[derive(Clone)]
pub struct ApiTokenAuthenticator {
    repository: Arc<repository::Postgres>,
}

impl ApiTokenAuthenticator {
    pub fn new(repository: Arc<repository::Postgres>) -> Self {
        Self { repository }
    }

    pub async fn authenticate(&self, raw_token: &str) -> Result<Option<ApiTokenContext>, Error> {
        let digest = match parse(raw_token) {
            Ok(digest) => digest,
            Err(_) => return Ok(None),
        };

        let Some(stored) = self
            .repository
            .get_api_token_by_digest(digest)
            .await
            .map_err(Error::Repository)?
        else {
            return Ok(None);
        };
        let api_token_id = stored.token.id;
        let user_id = stored.token.user_id;
        let workspace_id = stored.token.workspace_id;

        if stored.token.revoked_at.is_some() || stored.token.expires_at <= Utc::now() {
            return Ok(None);
        }

        if self
            .repository
            .get_membership_role(workspace_id, user_id)
            .await
            .map_err(Error::Repository)?
            .is_none()
        {
            return Ok(None);
        }

        if let Err(error) = self
            .repository
            .touch_api_token_last_used_at(api_token_id)
            .await
        {
            tracing::warn!(
                %error,
                api_token_id = %Uuid::from(api_token_id),
                "API token last_used_at update failed"
            );
        }

        Ok(Some(ApiTokenContext {
            user_id,
            api_token_id,
            workspace_id,
            permissions: WorkspacePermissions::from_iter(stored.permissions),
        }))
    }
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

pub struct UserAuthenticator<V: TokenVerifier> {
    verifier: Arc<V>,
    repository: Arc<repository::Postgres>,
}

impl<V: TokenVerifier> Clone for UserAuthenticator<V> {
    fn clone(&self) -> Self {
        Self {
            verifier: self.verifier.clone(),
            repository: self.repository.clone(),
        }
    }
}

impl<V: TokenVerifier> UserAuthenticator<V> {
    pub fn new(verifier: Arc<V>, repository: Arc<repository::Postgres>) -> Self {
        Self {
            verifier,
            repository,
        }
    }

    pub async fn authenticate(&self, token: &str) -> Result<UserContext, AuthError> {
        let claims = self
            .verifier
            .verify(token)
            .await
            .map_err(AuthError::from_verify)?;

        let user = self
            .repository
            .upsert_user_by_auth0_sub(&ProvisionUserPayload {
                auth0_sub: claims.sub,
                email: claims.email,
                name: claims.name,
            })
            .await
            .map_err(AuthError::Repository)?;

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
    Repository(#[source] repository::Error),
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
    Repository(#[source] repository::Error),
    #[error("PASETO initialization failed")]
    Paseto(#[from] paseto::Error),
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use crate::domain::{ApiTokenId, UserId};

    use super::{ApiTokenContext, WorkspaceId, WorkspacePermission, WorkspacePermissions};

    #[test]
    fn api_token_context_allows_matching_workspace_and_permission_only() {
        let workspace_id = WorkspaceId::from(Uuid::new_v4());
        let context = ApiTokenContext {
            user_id: UserId::from(Uuid::new_v4()),
            api_token_id: ApiTokenId::from(Uuid::new_v4()),
            workspace_id,
            permissions: WorkspacePermissions::from_iter([WorkspacePermission::ReadControls]),
        };

        assert!(context.allows(workspace_id, WorkspacePermission::ReadControls));
        assert!(!context.allows(workspace_id, WorkspacePermission::WriteControls));
        assert!(!context.allows(
            WorkspaceId::from(Uuid::new_v4()),
            WorkspacePermission::ReadControls
        ));
    }
}
