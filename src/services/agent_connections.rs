//! Compatibility façade for adapters not yet cut over to typed operations.
pub use crate::authentication::AgentConnectionContext;
use crate::{
    application::{
        commands::agent_connections::{
            ActivateAgentConnection, ActivateAgentConnectionHandler,
            ActivateAgentConnectionOutcome, AgentConnectionCommandError, AuthorizeAgentConnection,
            AuthorizeAgentConnectionHandler, ConsumeAgentConnectionContinuation,
            ConsumeAgentConnectionContinuationHandler, ConsumeAgentConnectionOutcome,
            DenyAgentConnection, DenyAgentConnectionHandler, RequestAgentConnection,
            RequestAgentConnectionHandler, RevokeAgentConnection, RevokeAgentConnectionHandler,
            UseAgentConnection, UseAgentConnectionHandler,
        },
        queries::agent_connections::{
            FindReusableAgentConnection, FindReusableAgentConnectionHandler,
            ListUserAgentConnections, ListUserAgentConnectionsHandler,
        },
        ExecutionMetadata,
    },
    domain::{
        AgentConnection, AgentConnectionId, Sha256Digest, UserAgentConnection, UserId, WorkspaceId,
        WorkspacePermission,
    },
    repository::{ConflictKind, Error as RepositoryError, Postgres},
};
use chrono::{DateTime, Utc};
use std::sync::Arc;
use thiserror::Error;
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
            Self::AlreadyExists
        } else {
            Self::Repository(error)
        }
    }
}
impl From<AgentConnectionCommandError> for AgentConnectionError {
    fn from(error: AgentConnectionCommandError) -> Self {
        match error {
            AgentConnectionCommandError::AlreadyExists => Self::AlreadyExists,
            AgentConnectionCommandError::Denied
            | AgentConnectionCommandError::Invalid
            | AgentConnectionCommandError::Unavailable => Self::PolicyRejected,
            AgentConnectionCommandError::Repository(error) => error.into(),
        }
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
#[allow(
    clippy::large_enum_variant,
    reason = "the compatibility outcome preserves the legacy shape"
)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConsumeContinuationOutcome {
    Approved(AgentConnection),
    Invalid,
}
#[allow(
    clippy::large_enum_variant,
    reason = "the compatibility outcome preserves the legacy shape"
)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActivationOutcome {
    Activated(AgentConnection),
    Rejected,
}
impl AgentConnectionService {
    pub fn new(repository: Arc<Postgres>) -> Self {
        Self { repository }
    }
    pub async fn create_pending(
        &self,
        payload: CreatePendingConnectionPayload,
    ) -> Result<AgentConnection, AgentConnectionError> {
        Ok(RequestAgentConnectionHandler::new(self.repository.clone())
            .handle(
                RequestAgentConnection {
                    user_id: payload.user_id,
                    workspace_id: payload.workspace_id,
                    auth0_subject: payload.auth0_subject,
                    auth0_client_id: payload.auth0_client_id,
                    client_display_name: payload.client_display_name,
                    resource: payload.resource,
                    permissions: payload.permissions,
                    expires_at: payload.expires_at,
                    continuation_token: payload.continuation_token,
                    nonce: payload.nonce,
                },
                ExecutionMetadata::background(),
            )
            .await?)
    }
    pub async fn deny_pending(
        &self,
        continuation_token: &str,
    ) -> Result<bool, AgentConnectionError> {
        Ok(DenyAgentConnectionHandler::new(self.repository.clone())
            .handle(
                DenyAgentConnection {
                    continuation_token: continuation_token.to_owned(),
                },
                ExecutionMetadata::background(),
            )
            .await?)
    }
    pub async fn consume_continuation(
        &self,
        payload: ConsumeContinuationPayload,
    ) -> Result<ConsumeContinuationOutcome, AgentConnectionError> {
        Ok(
            match ConsumeAgentConnectionContinuationHandler::new(self.repository.clone())
                .handle(
                    ConsumeAgentConnectionContinuation {
                        continuation_token: payload.continuation_token,
                        nonce: payload.nonce,
                    },
                    ExecutionMetadata::background(),
                )
                .await?
            {
                ConsumeAgentConnectionOutcome::Approved(connection) => {
                    ConsumeContinuationOutcome::Approved(connection)
                }
                ConsumeAgentConnectionOutcome::Invalid => ConsumeContinuationOutcome::Invalid,
            },
        )
    }
    pub async fn find_reusable(
        &self,
        payload: FindReusableConnectionPayload,
    ) -> Result<Option<AgentConnection>, AgentConnectionError> {
        let result = FindReusableAgentConnectionHandler::new(self.repository.clone())
            .handle(FindReusableAgentConnection {
                auth0_subject: payload.auth0_subject,
                auth0_client_id: payload.auth0_client_id,
                resource: payload.resource,
                permissions: payload.permissions,
            })
            .await?;
        match result {
            Some(connection) => Ok(self
                .repository
                .agent_connections()
                .get(connection.id)
                .await?),
            None => Ok(None),
        }
    }
    pub async fn authorize_mcp_connection(
        &self,
        payload: AuthorizeMcpConnectionPayload,
    ) -> Result<Option<AgentConnectionContext>, AgentConnectionError> {
        Ok(
            AuthorizeAgentConnectionHandler::new(self.repository.clone())
                .handle(
                    AuthorizeAgentConnection {
                        connection_id: payload.connection_id,
                        workspace_id: payload.workspace_id,
                        auth0_subject: payload.auth0_subject,
                        auth0_client_id: payload.auth0_client_id,
                        resource: payload.resource,
                        permissions: payload.permissions,
                    },
                    ExecutionMetadata::background(),
                )
                .await?,
        )
    }
    pub async fn activate(
        &self,
        id: AgentConnectionId,
    ) -> Result<ActivationOutcome, AgentConnectionError> {
        Ok(
            match ActivateAgentConnectionHandler::new(self.repository.clone())
                .handle(
                    ActivateAgentConnection { connection_id: id },
                    ExecutionMetadata::background(),
                )
                .await?
            {
                ActivateAgentConnectionOutcome::Activated(connection) => {
                    ActivationOutcome::Activated(connection)
                }
                ActivateAgentConnectionOutcome::Rejected => ActivationOutcome::Rejected,
            },
        )
    }
    pub async fn touch_last_used(
        &self,
        id: AgentConnectionId,
    ) -> Result<bool, AgentConnectionError> {
        Ok(UseAgentConnectionHandler::new(self.repository.clone())
            .handle(
                UseAgentConnection { connection_id: id },
                ExecutionMetadata::background(),
            )
            .await?)
    }
    pub async fn revoke(&self, id: AgentConnectionId) -> Result<bool, AgentConnectionError> {
        Ok(RevokeAgentConnectionHandler::new(self.repository.clone())
            .handle(
                RevokeAgentConnection {
                    connection_id: id,
                    user_id: None,
                },
                ExecutionMetadata::background(),
            )
            .await?)
    }
    pub async fn list_for_user(
        &self,
        user_id: UserId,
    ) -> Result<Vec<UserAgentConnection>, AgentConnectionError> {
        Ok(
            ListUserAgentConnectionsHandler::new(self.repository.clone())
                .handle(ListUserAgentConnections { user_id })
                .await?
                .into_iter()
                .map(|value| UserAgentConnection {
                    id: value.id,
                    client_name: value.client_name,
                    status: value.status,
                    authorized_at: value.authorized_at,
                    last_used_at: value.last_used_at,
                })
                .collect(),
        )
    }
    pub async fn revoke_for_user(
        &self,
        user_id: UserId,
        id: AgentConnectionId,
    ) -> Result<bool, AgentConnectionError> {
        Ok(RevokeAgentConnectionHandler::new(self.repository.clone())
            .handle(
                RevokeAgentConnection {
                    connection_id: id,
                    user_id: Some(user_id),
                },
                ExecutionMetadata::background(),
            )
            .await?)
    }
}
pub fn digest_secret(value: &str) -> Sha256Digest {
    Sha256Digest::digest(value.as_bytes())
}
