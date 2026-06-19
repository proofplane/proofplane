use std::sync::Arc;

use api_keys_simplified::{ApiKeyManagerV0, Environment, KeyStatus, SecureString};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    authentication::auth0::{TokenVerifier, VerifyError},
    domain::{
        canonical_permissions, ActorId, ActorPermissions, ApiTokenId, ProvisionUserPayload, UserId,
        WorkspaceId, WorkspacePermission,
    },
    repository,
};

pub mod auth0;
pub mod paseto;
pub(crate) mod signed_jwt;

const API_KEY_PREFIX: &str = "proof";

/// An authenticated actor acting within its home workspace, carrying the
/// permission grants resolved at authentication time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActorContext {
    pub workspace_id: WorkspaceId,
    pub id: ActorId,
    pub permissions: ActorPermissions,
}

impl ActorContext {
    pub fn new(workspace_id: WorkspaceId, id: ActorId, permissions: ActorPermissions) -> Self {
        Self {
            workspace_id,
            id,
            permissions,
        }
    }
}

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
        let permissions = ActorPermissions::from_iter(permissions.iter().copied());
        Self {
            version: Self::VERSION,
            workspace_id: Uuid::from(workspace_id),
            permissions: permissions
                .iter()
                .map(|permission| permission.as_str().to_owned())
                .collect(),
        }
    }

    fn validate(&self) -> Option<ActorPermissions> {
        if self.version != Self::VERSION {
            return None;
        }

        let mut parsed = Vec::with_capacity(self.permissions.len());
        for permission in &self.permissions {
            parsed.push(permission.parse::<WorkspacePermission>().ok()?);
        }

        let canonical = canonical_permissions(parsed).ok()?;
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
            return None;
        }

        Some(ActorPermissions::from_iter(canonical))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ApiTokenContext {
    pub user_id: UserId,
    pub api_token_id: ApiTokenId,
    pub workspace_id: WorkspaceId,
    pub permissions: ActorPermissions,
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
        let Some(permissions) = verified.claims.validate() else {
            return Ok(None);
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

        if stored.token.user_id != user_id
            || stored.token.workspace_id != workspace_id
            || stored.token.expires_at != verified.expires_at
            || stored.token.revoked_at.is_some()
            || stored.token.expires_at <= Utc::now()
            || ActorPermissions::from_iter(stored.permissions) != permissions
        {
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

#[derive(Clone)]
pub struct ApiKeyManager {
    manager: ApiKeyManagerV0,
}

impl ApiKeyManager {
    pub fn new() -> Result<Self, Error> {
        Ok(Self {
            manager: ApiKeyManagerV0::init_default_config(API_KEY_PREFIX)
                .map_err(Error::ApiKeyInit)?,
        })
    }

    pub fn issue(&self, environment: Environment) -> Result<IssuedApiKey, Error> {
        let api_key = self.manager.generate(environment).map_err(Error::ApiKey)?;
        let hash = api_key.expose_hash();

        Ok(IssuedApiKey {
            raw_key: api_key.key().clone(),
            key_id: hash.key_id().clone(),
            credential_hash: hash.hash().clone(),
        })
    }

    fn key_id(&self, raw_key: &SecureString) -> String {
        self.manager.extract_key_id(raw_key)
    }

    fn verify(&self, raw_key: &SecureString, credential_hash: &str) -> bool {
        matches!(
            self.manager.verify(raw_key, credential_hash),
            Ok(KeyStatus::Valid)
        )
    }
}

pub struct IssuedApiKey {
    pub raw_key: SecureString,
    pub key_id: String,
    pub credential_hash: String,
}

#[derive(Clone)]
pub struct ApiKeyAuthenticator {
    api_keys: ApiKeyManager,
    repository: Arc<repository::Postgres>,
}

impl ApiKeyAuthenticator {
    pub fn new(api_keys: ApiKeyManager, repository: Arc<repository::Postgres>) -> Self {
        Self {
            api_keys,
            repository,
        }
    }

    /// Resolves the credential by the `key_id` embedded in the presented key,
    /// scoped to the claimed actor, then verifies it. Returns an `ActorContext`
    /// bound to the actor's home workspace with its permission grants. An
    /// unknown, revoked, or expired key yields `None`.
    pub async fn authenticate(
        &self,
        actor_id: ActorId,
        api_key: &str,
    ) -> Result<Option<ActorContext>, Error> {
        let raw_key = SecureString::from(api_key);
        let key_id = self.api_keys.key_id(&raw_key);

        let Some((actor, credential, permissions)) = self
            .repository
            .actor_credential_by_key_id(actor_id, &key_id)
            .await
            .map_err(Error::Repository)?
        else {
            return Ok(None);
        };

        if credential.revoked_at.is_some()
            || credential
                .expires_at
                .is_some_and(|expires_at| expires_at <= Utc::now())
        {
            return Ok(None);
        }

        if !self.api_keys.verify(&raw_key, &credential.credential_hash) {
            return Ok(None);
        }

        Ok(Some(ActorContext::new(
            actor.workspace_id,
            actor.id,
            permissions,
        )))
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
    #[error("API key initialization failed")]
    ApiKeyInit(#[source] api_keys_simplified::InitError),
    #[error("API key operation failed")]
    ApiKey(#[source] api_keys_simplified::Error),
    #[error("credential repository error")]
    Repository(#[source] repository::Error),
    #[error("PASETO initialization failed")]
    Paseto(#[source] paseto::Error),
}

#[cfg(test)]
mod tests {
    use api_keys_simplified::{Environment, ExposeSecret, SecureString};
    use uuid::Uuid;

    use super::{
        ActorPermissions, ApiKeyManager, ApiTokenContext, UserApiTokenClaims, WorkspaceId,
        WorkspacePermission,
    };

    #[test]
    fn issuance_returns_generated_key_and_storable_hash_material() {
        let api_keys = ApiKeyManager::new().expect("API key manager builds");
        let issued = api_keys.issue(Environment::test()).expect("key issues");
        let raw_key = issued.raw_key.expose_secret();

        assert!(raw_key.starts_with("proof-test-"));
        assert_eq!(issued.key_id.len(), 32);
        assert!(issued.credential_hash.starts_with("$argon2id$"));
        assert!(!issued.credential_hash.contains(raw_key));
    }

    #[test]
    fn issued_key_verifies_and_wrong_or_malformed_keys_fail() {
        let api_keys = ApiKeyManager::new().expect("API key manager builds");
        let issued = api_keys.issue(Environment::test()).expect("key issues");

        assert!(api_keys.verify(&issued.raw_key, &issued.credential_hash));
        assert!(!api_keys.verify(
            &SecureString::from("pp_test_wrong"),
            &issued.credential_hash
        ));
        assert!(!api_keys.verify(
            &SecureString::from("not-an-api-key"),
            &issued.credential_hash
        ));
    }

    #[test]
    fn api_token_context_allows_matching_workspace_and_permission_only() {
        let workspace_id = WorkspaceId::from(Uuid::new_v4());
        let context = ApiTokenContext {
            user_id: crate::domain::UserId::from(Uuid::new_v4()),
            api_token_id: crate::domain::ApiTokenId::from(Uuid::new_v4()),
            workspace_id,
            permissions: ActorPermissions::from_iter([WorkspacePermission::ReadControls]),
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
        .is_none());
        assert!(UserApiTokenClaims {
            version: 1,
            workspace_id,
            permissions: vec!["delete_everything".to_owned()],
        }
        .validate()
        .is_none());
        assert!(UserApiTokenClaims {
            version: 1,
            workspace_id,
            permissions: vec!["read_controls".to_owned(), "read_controls".to_owned()],
        }
        .validate()
        .is_none());
        assert!(UserApiTokenClaims {
            version: 1,
            workspace_id,
            permissions: vec!["write_controls".to_owned(), "read_controls".to_owned()],
        }
        .validate()
        .is_none());
        assert!(UserApiTokenClaims {
            version: 1,
            workspace_id,
            permissions: vec!["read_controls".to_owned(), "write_controls".to_owned()],
        }
        .validate()
        .is_some());
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
