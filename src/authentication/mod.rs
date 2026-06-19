use std::sync::Arc;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    authentication::auth0::{TokenVerifier, VerifyError},
    domain::{
        canonical_permissions, ApiTokenId, DomainError, ProvisionUserPayload, UserId, WorkspaceId,
        WorkspacePermission, WorkspacePermissions,
    },
    repository,
};

pub mod auth0;
pub mod paseto;

/// Custom claims carried by user-owned API tokens. Issuance and verification
/// share this type so claim names and permission serialization cannot drift.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserApiTokenClaims {
    pub version: u8,
    pub workspace_id: Uuid,
    pub permissions: Vec<String>,
}

impl UserApiTokenClaims {
    pub const VERSION: u8 = 1;

    pub fn new(workspace_id: WorkspaceId, permissions: &[WorkspacePermission]) -> Self {
        let permissions = WorkspacePermissions::from_iter(permissions.iter().copied());
        Self {
            version: Self::VERSION,
            workspace_id: Uuid::from(workspace_id),
            permissions: permissions
                .iter()
                .map(|permission| permission.as_str().to_owned())
                .collect(),
        }
    }

    fn validate(&self) -> Result<WorkspacePermissions, ApiTokenClaimError> {
        if self.version != Self::VERSION {
            return Err(ApiTokenClaimError::UnsupportedVersion {
                version: self.version,
            });
        }

        let mut parsed = Vec::with_capacity(self.permissions.len());
        for permission in &self.permissions {
            parsed.push(
                permission
                    .parse::<WorkspacePermission>()
                    .map_err(ApiTokenClaimError::Permission)?,
            );
        }

        let canonical = canonical_permissions(parsed).map_err(ApiTokenClaimError::Permission)?;
        let canonical_strings = canonical
            .iter()
            .map(|permission| permission.as_str())
            .collect::<Vec<_>>();
        let claim_strings = self
            .permissions
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>();
        if claim_strings != canonical_strings {
            return Err(ApiTokenClaimError::NonCanonicalPermissionOrder);
        }

        Ok(WorkspacePermissions::from_iter(canonical))
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
enum ApiTokenClaimError {
    #[error("unsupported API token claim version {version}")]
    UnsupportedVersion { version: u8 },
    #[error("invalid API token permission claims")]
    Permission(#[source] DomainError),
    #[error("API token permissions are not in canonical order")]
    NonCanonicalPermissionOrder,
}

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
    verifier: paseto::ApiTokenVerifier,
    repository: Arc<repository::Postgres>,
}

impl ApiTokenAuthenticator {
    pub fn new(verifier: paseto::ApiTokenVerifier, repository: Arc<repository::Postgres>) -> Self {
        Self {
            verifier,
            repository,
        }
    }

    pub async fn authenticate(&self, raw_token: &str) -> Result<Option<ApiTokenContext>, Error> {
        let verified = match self.verifier.verify::<UserApiTokenClaims>(raw_token) {
            Ok(verified) => verified,
            Err(_) => return Ok(None),
        };
        let permissions = match verified.claims.validate() {
            Ok(permissions) => permissions,
            Err(_) => return Ok(None),
        };
        let api_token_id = ApiTokenId::from(verified.token_id);
        let user_id = UserId::from(verified.subject);
        let workspace_id = WorkspaceId::from(verified.claims.workspace_id);

        let Some(stored) = self
            .repository
            .get_api_token(api_token_id)
            .await
            .map_err(Error::Repository)?
        else {
            return Ok(None);
        };

        if stored.token.revoked_at.is_some() {
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
            permissions,
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
    Paseto(#[source] paseto::Error),
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use crate::domain::{ApiTokenId, UserId};

    use super::{
        ApiTokenClaimError, ApiTokenContext, UserApiTokenClaims, WorkspaceId, WorkspacePermission,
        WorkspacePermissions,
    };

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

    #[test]
    fn api_token_claim_validation_rejects_non_canonical_claims() {
        let workspace_id = Uuid::new_v4();
        assert!(UserApiTokenClaims {
            version: 2,
            workspace_id,
            permissions: vec!["read_controls".to_owned()],
        }
        .validate()
        .is_err());
        assert!(UserApiTokenClaims {
            version: 1,
            workspace_id,
            permissions: vec!["delete_everything".to_owned()],
        }
        .validate()
        .is_err());
        assert_eq!(
            UserApiTokenClaims {
                version: 1,
                workspace_id,
                permissions: vec!["read_controls".to_owned(), "read_controls".to_owned()],
            }
            .validate(),
            Err(ApiTokenClaimError::Permission(
                crate::domain::DomainError::DuplicatePermission {
                    permission: "read_controls".to_owned(),
                }
            ))
        );
        assert!(UserApiTokenClaims {
            version: 1,
            workspace_id,
            permissions: vec!["write_controls".to_owned(), "read_controls".to_owned()],
        }
        .validate()
        .is_err());
        assert!(UserApiTokenClaims {
            version: 1,
            workspace_id,
            permissions: vec!["read_controls".to_owned(), "write_controls".to_owned()],
        }
        .validate()
        .is_ok());
    }

    #[test]
    fn malformed_permission_arrays_do_not_deserialize_as_claims() {
        assert!(
            serde_json::from_value::<UserApiTokenClaims>(serde_json::json!({
                "version": 1,
                "workspace_id": Uuid::new_v4(),
                "permissions": "read_controls"
            }))
            .is_err()
        );
        assert!(
            serde_json::from_value::<UserApiTokenClaims>(serde_json::json!({
                "version": 1,
                "workspace_id": Uuid::new_v4(),
                "permissions": ["read_controls", 1]
            }))
            .is_err()
        );
    }
}
