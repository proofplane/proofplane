use std::sync::Arc;

use chrono::{DateTime, Utc};
use thiserror::Error;
use uuid::Uuid;

use crate::{
    domain::{
        AgentAuthorizationTransactionId, AgentConnection, AgentConnectionId,
        NewPendingAgentConnection, Sha256Digest, UserAgentConnection, UserId, WorkspaceId,
        WorkspacePermission, WorkspacePermissions,
    },
    repository::{ConflictKind, Error as RepositoryError, Postgres},
};

#[derive(Clone)]
pub struct AgentConnectionService {
    repository: Arc<Postgres>,
}

#[derive(Debug, Error)]
pub enum AgentConnectionError {
    #[error("agent connection request was rejected by policy")]
    PolicyRejected,
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
pub struct CreatePendingConnectionPayload {
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

#[derive(Debug, Clone)]
pub struct FindReusableConnectionPayload {
    pub auth0_subject: String,
    pub auth0_client_id: String,
    pub resource: String,
    pub permissions: Vec<WorkspacePermission>,
}

#[derive(Debug, Clone)]
pub struct AuthorizeMcpConnectionPayload {
    pub connection_id: AgentConnectionId,
    pub workspace_id: WorkspaceId,
    pub auth0_subject: String,
    pub auth0_client_id: String,
    pub resource: String,
    pub permissions: Vec<WorkspacePermission>,
}

#[derive(Debug, Clone)]
pub struct ConsumeContinuationPayload {
    pub continuation_token: String,
    pub nonce: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(
    clippy::large_enum_variant,
    reason = "the approved policy outcome intentionally owns the repository result"
)]
pub enum ConsumeContinuationOutcome {
    Approved(AgentConnection),
    Invalid,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(
    clippy::large_enum_variant,
    reason = "the activated policy outcome intentionally owns the repository result"
)]
pub enum ActivationOutcome {
    Activated(AgentConnection),
    Rejected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AgentConnectionContext {
    pub user_id: UserId,
    pub connection_id: AgentConnectionId,
    pub workspace_id: WorkspaceId,
    pub permissions: WorkspacePermissions,
}

impl AgentConnectionService {
    pub fn new(repository: Arc<Postgres>) -> Self {
        Self { repository }
    }

    pub async fn create_pending(
        &self,
        payload: CreatePendingConnectionPayload,
    ) -> Result<AgentConnection, AgentConnectionError> {
        let user = self
            .repository
            .get_user(payload.user_id)
            .await?
            .ok_or(AgentConnectionError::PolicyRejected)?;
        if user.auth0_sub != payload.auth0_subject {
            return Err(AgentConnectionError::PolicyRejected);
        }
        if self
            .repository
            .get_membership_role(payload.workspace_id, payload.user_id)
            .await?
            .is_none()
        {
            return Err(AgentConnectionError::PolicyRejected);
        }

        Ok(self
            .repository
            .create_pending_agent_connection(&NewPendingAgentConnection {
                id: AgentConnectionId::from(Uuid::new_v4()),
                transaction_id: AgentAuthorizationTransactionId::from(Uuid::new_v4()),
                user_id: payload.user_id,
                workspace_id: payload.workspace_id,
                auth0_subject: payload.auth0_subject,
                auth0_client_id: payload.auth0_client_id,
                client_display_name: payload.client_display_name,
                resource: payload.resource,
                permissions: payload.permissions,
                pending_expires_at: payload.expires_at,
                continuation_digest: digest_secret(&payload.continuation_token),
                nonce_digest: digest_secret(&payload.nonce),
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
        payload: ConsumeContinuationPayload,
    ) -> Result<ConsumeContinuationOutcome, AgentConnectionError> {
        let connection = self
            .repository
            .consume_agent_connection_continuation(
                digest_secret(&payload.continuation_token),
                digest_secret(&payload.nonce),
            )
            .await?;
        Ok(match connection {
            Some(connection) => ConsumeContinuationOutcome::Approved(connection),
            None => ConsumeContinuationOutcome::Invalid,
        })
    }

    pub async fn find_reusable(
        &self,
        payload: FindReusableConnectionPayload,
    ) -> Result<Option<AgentConnection>, AgentConnectionError> {
        let connection = self
            .repository
            .find_reusable_agent_connection(
                &payload.auth0_subject,
                &payload.auth0_client_id,
                &payload.resource,
            )
            .await?;
        Ok(connection.filter(|connection| connection.permissions == payload.permissions))
    }

    pub async fn authorize_mcp_connection(
        &self,
        payload: AuthorizeMcpConnectionPayload,
    ) -> Result<Option<AgentConnectionContext>, AgentConnectionError> {
        let permission_strings = payload
            .permissions
            .iter()
            .map(|permission| permission.as_str().to_owned())
            .collect::<Vec<_>>();
        let Some(connection) = self
            .repository
            .authorize_agent_connection(
                payload.connection_id,
                payload.workspace_id,
                &payload.auth0_subject,
                &payload.auth0_client_id,
                &payload.resource,
                &permission_strings,
            )
            .await?
        else {
            return Ok(None);
        };

        Ok(Some(AgentConnectionContext {
            user_id: connection.user_id,
            connection_id: connection.id,
            workspace_id: connection.workspace_id,
            permissions: WorkspacePermissions::from_iter(connection.permissions),
        }))
    }

    pub async fn activate(
        &self,
        id: AgentConnectionId,
    ) -> Result<ActivationOutcome, AgentConnectionError> {
        let connection = self.repository.activate_agent_connection(id).await?;
        Ok(match connection {
            Some(connection) => ActivationOutcome::Activated(connection),
            None => ActivationOutcome::Rejected,
        })
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

    pub async fn list_for_user(
        &self,
        user_id: UserId,
    ) -> Result<Vec<UserAgentConnection>, AgentConnectionError> {
        Ok(self.repository.list_user_agent_connections(user_id).await?)
    }

    pub async fn revoke_for_user(
        &self,
        user_id: UserId,
        id: AgentConnectionId,
    ) -> Result<bool, AgentConnectionError> {
        Ok(self
            .repository
            .revoke_user_agent_connection(user_id, id)
            .await?)
    }
}

pub fn digest_secret(value: &str) -> Sha256Digest {
    Sha256Digest::digest(value.as_bytes())
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
