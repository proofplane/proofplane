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
    pub agent_connection_id: Option<Uuid>,
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

#[derive(Clone)]
pub struct OAuthAccessAuthenticator {
    repository: Arc<repository::Postgres>,
    verifier: paseto::OAuthTokenVerifier,
    resource: String,
}

impl OAuthAccessAuthenticator {
    pub fn new(
        repository: Arc<repository::Postgres>,
        verifier: paseto::OAuthTokenVerifier,
        resource: String,
    ) -> Self {
        Self {
            repository,
            verifier,
            resource,
        }
    }

    pub async fn authenticate(&self, raw: &str) -> Result<Option<ApiTokenContext>, Error> {
        let verified = match self.verifier.verify::<crate::routes::oauth::TokenClaims>(
            raw,
            &self.resource,
            false,
        ) {
            Ok(value) => value,
            Err(_) => return Ok(None),
        };
        let claims = verified.claims;
        let client = self
            .repository
            .get()
            .await
            .map_err(repository::Error::from)
            .map_err(Error::Repository)?;
        let row = client
            .query_opt(
                "SELECT 1 FROM agent_connections a
             JOIN workspace_memberships m ON m.workspace_id=a.workspace_id AND m.user_id=a.user_id
             WHERE a.id=$1 AND a.user_id=$2 AND a.workspace_id=$3 AND a.client_id=$4
               AND a.resource=$5 AND a.permissions=$6 AND a.revoked_at IS NULL",
                &[
                    &claims.connection_id,
                    &claims.user_id,
                    &claims.workspace_id,
                    &claims.client_id,
                    &claims.resource,
                    &claims.permissions,
                ],
            )
            .await
            .map_err(repository::Error::from)
            .map_err(Error::Repository)?;
        if row.is_none() || claims.resource != self.resource {
            return Ok(None);
        }
        let permissions = claims
            .permissions
            .iter()
            .map(|value| value.parse())
            .collect::<Result<WorkspacePermissions, _>>()
            .map_err(|_| paseto::Error::Verify)?;
        let _ = client
            .execute(
                "UPDATE agent_connections SET last_used_at=now() WHERE id=$1",
                &[&claims.connection_id],
            )
            .await;
        Ok(Some(ApiTokenContext {
            user_id: claims.user_id.into(),
            api_token_id: claims.connection_id.into(),
            workspace_id: claims.workspace_id.into(),
            permissions,
            agent_connection_id: Some(claims.connection_id),
        }))
    }
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
            agent_connection_id: None,
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
            agent_connection_id: None,
        };

        assert!(context.allows(workspace_id, WorkspacePermission::ReadControls));
        assert!(!context.allows(workspace_id, WorkspacePermission::WriteControls));
        assert!(!context.allows(
            WorkspaceId::from(Uuid::new_v4()),
            WorkspacePermission::ReadControls
        ));
    }
}
