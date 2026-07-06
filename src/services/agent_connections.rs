use std::sync::Arc;

use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};
use thiserror::Error;
use uuid::Uuid;

use crate::{
    domain::{
        canonical_permissions, AgentAuthorizationTransactionId, AgentConnection, AgentConnectionId,
        CreatePendingAgentConnection, SecretDigest, UserId, WorkspaceId, WorkspacePermission,
    },
    repository::{ConflictKind, Error as RepositoryError, Postgres},
};

#[derive(Clone)]
pub struct AgentConnectionService {
    repository: Arc<Postgres>,
}

#[derive(Debug, Error)]
pub enum AgentConnectionError {
    #[error("agent connection request is invalid: {0}")]
    Invalid(String),
    #[error("a live agent connection already exists")]
    AlreadyExists,
    #[error("repository error")]
    Repository(RepositoryError),
}

impl From<RepositoryError> for AgentConnectionError {
    fn from(error: RepositoryError) -> Self {
        if matches!(
            error,
            RepositoryError::Conflict(ConflictKind::AgentConnectionExists)
        ) {
            return Self::AlreadyExists;
        }
        Self::Repository(error)
    }
}

#[derive(Debug, Clone)]
pub struct CreatePendingConnection {
    pub user_id: UserId,
    pub workspace_id: WorkspaceId,
    pub auth0_subject: String,
    pub auth0_client_id: String,
    pub client_display_name: String,
    pub resource: String,
    pub permissions: Vec<WorkspacePermission>,
    pub expires_at: DateTime<Utc>,
    pub continuation_token: String,
    pub nonce: String,
}

impl AgentConnectionService {
    pub fn new(repository: Arc<Postgres>) -> Self {
        Self { repository }
    }

    pub async fn create_pending(
        &self,
        request: CreatePendingConnection,
    ) -> Result<AgentConnection, AgentConnectionError> {
        if request.expires_at <= Utc::now() {
            return Err(AgentConnectionError::Invalid(
                "expires_at must be in the future".to_owned(),
            ));
        }
        for (name, value) in [
            ("auth0_subject", request.auth0_subject.as_str()),
            ("auth0_client_id", request.auth0_client_id.as_str()),
            ("client_display_name", request.client_display_name.as_str()),
            ("resource", request.resource.as_str()),
            ("continuation_token", request.continuation_token.as_str()),
            ("nonce", request.nonce.as_str()),
        ] {
            if value.trim().is_empty() {
                return Err(AgentConnectionError::Invalid(format!(
                    "{name} must not be blank"
                )));
            }
        }
        let permissions = canonical_permissions(request.permissions)
            .map_err(|error| AgentConnectionError::Invalid(error.to_string()))?;
        if permissions.is_empty() {
            return Err(AgentConnectionError::Invalid(
                "permissions must not be empty".to_owned(),
            ));
        }
        let resource = url::Url::parse(&request.resource).map_err(|_| {
            AgentConnectionError::Invalid("resource must be an absolute URL".to_owned())
        })?;
        if resource.query().is_some()
            || resource.fragment().is_some()
            || resource.as_str() != request.resource
        {
            return Err(AgentConnectionError::Invalid(
                "resource must be a canonical URL without query or fragment".to_owned(),
            ));
        }
        let user = self
            .repository
            .get_user(request.user_id)
            .await?
            .ok_or_else(|| AgentConnectionError::Invalid("user does not exist".to_owned()))?;
        if user.auth0_sub != request.auth0_subject {
            return Err(AgentConnectionError::Invalid(
                "auth0_subject does not match the user".to_owned(),
            ));
        }
        if self
            .repository
            .get_membership_role(request.workspace_id, request.user_id)
            .await?
            .is_none()
        {
            return Err(AgentConnectionError::Invalid(
                "workspace membership does not exist".to_owned(),
            ));
        }

        Ok(self
            .repository
            .create_pending_agent_connection(&CreatePendingAgentConnection {
                id: AgentConnectionId::from(Uuid::new_v4()),
                transaction_id: AgentAuthorizationTransactionId::from(Uuid::new_v4()),
                user_id: request.user_id,
                workspace_id: request.workspace_id,
                auth0_subject: request.auth0_subject,
                auth0_client_id: request.auth0_client_id,
                client_display_name: request.client_display_name,
                resource: request.resource,
                permissions,
                pending_expires_at: request.expires_at,
                continuation_digest: digest_secret(&request.continuation_token),
                nonce_digest: digest_secret(&request.nonce),
            })
            .await?)
    }

    pub async fn deny_pending(
        &self,
        continuation_token: &str,
    ) -> Result<bool, AgentConnectionError> {
        Ok(self
            .repository
            .deny_pending_agent_connection(digest_secret(continuation_token))
            .await?)
    }

    pub async fn consume_continuation(
        &self,
        continuation_token: &str,
        nonce: &str,
    ) -> Result<Option<AgentConnection>, AgentConnectionError> {
        Ok(self
            .repository
            .consume_agent_connection_continuation(
                digest_secret(continuation_token),
                digest_secret(nonce),
            )
            .await?)
    }

    pub async fn find_reusable(
        &self,
        auth0_subject: &str,
        auth0_client_id: &str,
        resource: &str,
        permissions: Vec<WorkspacePermission>,
    ) -> Result<Option<AgentConnection>, AgentConnectionError> {
        let permissions = canonical_permissions(permissions)
            .map_err(|error| AgentConnectionError::Invalid(error.to_string()))?;
        let connection = self
            .repository
            .find_reusable_agent_connection(auth0_subject, auth0_client_id, resource)
            .await?;
        Ok(connection.filter(|connection| connection.permissions == permissions))
    }

    pub async fn activate(
        &self,
        id: AgentConnectionId,
    ) -> Result<Option<AgentConnection>, AgentConnectionError> {
        Ok(self.repository.activate_agent_connection(id).await?)
    }

    pub async fn touch_last_used(
        &self,
        id: AgentConnectionId,
    ) -> Result<bool, AgentConnectionError> {
        Ok(self
            .repository
            .touch_agent_connection_last_used_at(id)
            .await?)
    }

    pub async fn revoke(&self, id: AgentConnectionId) -> Result<bool, AgentConnectionError> {
        Ok(self.repository.revoke_agent_connection(id).await?)
    }
}

pub fn digest_secret(value: &str) -> SecretDigest {
    SecretDigest::from_bytes(Sha256::digest(value.as_bytes()).into())
}

#[cfg(test)]
mod tests {
    use super::digest_secret;

    #[test]
    fn secret_digest_is_deterministic_and_does_not_retain_plaintext() {
        let digest = digest_secret("continuation-secret");

        assert_eq!(digest, digest_secret("continuation-secret"));
        assert_ne!(
            digest.as_bytes().as_slice(),
            b"continuation-secret".as_slice()
        );
    }
}
