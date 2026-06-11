use std::sync::Arc;

use api_keys_simplified::{ApiKeyManagerV0, Environment, KeyStatus, SecureString};
use chrono::Utc;

use crate::{
    authentication::auth0::{TokenVerifier, VerifyError},
    domain::{ActorId, ActorWithApiCredential, ProvisionUserPayload, UserId, WorkspaceId},
    repository,
};

pub mod auth0;

const API_KEY_PREFIX: &str = "proof";

/// An authenticated actor acting within a workspace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActorContext {
    pub workspace_id: WorkspaceId,
    pub id: ActorId,
}

impl ActorContext {
    pub fn new(workspace_id: WorkspaceId, id: ActorId) -> Self {
        Self { workspace_id, id }
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

    pub async fn authenticate(
        &self,
        workspace_id: WorkspaceId,
        actor_id: ActorId,
        api_key: &str,
    ) -> Result<Option<ActorContext>, Error> {
        let Some(ActorWithApiCredential {
            actor,
            api_credential: credential,
        }) = self
            .repository
            .actor_with_api_credential(actor_id)
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

        let raw_key = SecureString::from(api_key);
        if credential.key_id != self.api_keys.key_id(&raw_key)
            || !self.api_keys.verify(&raw_key, &credential.credential_hash)
        {
            return Ok(None);
        }

        Ok(Some(ActorContext::new(workspace_id, actor.id)))
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
}

#[cfg(test)]
mod tests {
    use api_keys_simplified::{Environment, ExposeSecret, SecureString};

    use super::ApiKeyManager;

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
}
