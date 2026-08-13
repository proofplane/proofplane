use crate::{
    application::ExecutionMetadata,
    authentication::AgentConnectionContext,
    domain::{
        canonical_permissions, AgentConnection, AgentConnectionActivation,
        AgentConnectionConsumption, AgentConnectionId, AgentConnectionRevocation,
        AgentConnectionUse, Sha256Digest, UserId, WorkspaceId, WorkspacePermission,
        WorkspacePermissions,
    },
    persistence::{param, ConflictKind, Error as RepositoryError, Postgres},
};
use chrono::{DateTime, Utc};
use std::sync::Arc;
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct RequestAgentConnection {
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
pub struct DenyAgentConnection {
    pub continuation_token: String,
}
#[derive(Debug, Clone)]
pub struct ConsumeAgentConnectionContinuation {
    pub continuation_token: String,
    pub nonce: String,
}
#[derive(Debug, Clone, Copy)]
pub struct ActivateAgentConnection {
    pub connection_id: AgentConnectionId,
}
#[derive(Debug, Clone)]
pub struct AuthorizeAgentConnection {
    pub connection_id: AgentConnectionId,
    pub workspace_id: WorkspaceId,
    pub auth0_subject: String,
    pub auth0_client_id: String,
    pub resource: String,
    pub permissions: Vec<WorkspacePermission>,
}
#[derive(Debug, Clone, Copy)]
pub struct UseAgentConnection {
    pub connection_id: AgentConnectionId,
}
#[derive(Debug, Clone, Copy)]
pub struct RevokeAgentConnection {
    pub connection_id: AgentConnectionId,
    pub user_id: Option<UserId>,
}
#[allow(
    clippy::large_enum_variant,
    reason = "the approved outcome owns the connection snapshot"
)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConsumeAgentConnectionOutcome {
    Approved(AgentConnection),
    Invalid,
}
#[allow(
    clippy::large_enum_variant,
    reason = "the activation outcome owns the connection snapshot"
)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActivateAgentConnectionOutcome {
    Activated(AgentConnection),
    Rejected,
}

#[derive(Clone)]
pub struct RequestAgentConnectionHandler {
    repository: Arc<Postgres>,
}
#[derive(Clone)]
pub struct DenyAgentConnectionHandler {
    repository: Arc<Postgres>,
}
#[derive(Clone)]
pub struct ConsumeAgentConnectionContinuationHandler {
    repository: Arc<Postgres>,
}
#[derive(Clone)]
pub struct ActivateAgentConnectionHandler {
    repository: Arc<Postgres>,
}
#[derive(Clone)]
pub struct AuthorizeAgentConnectionHandler {
    repository: Arc<Postgres>,
}
#[derive(Clone)]
pub struct UseAgentConnectionHandler {
    repository: Arc<Postgres>,
}
#[derive(Clone)]
pub struct RevokeAgentConnectionHandler {
    repository: Arc<Postgres>,
}
macro_rules! new {
    ($($name:ident),+ $(,)?) => {
        $(impl $name {
            pub fn new(repository: Arc<Postgres>) -> Self { Self { repository } }
        })+
    };
}
new!(
    RequestAgentConnectionHandler,
    DenyAgentConnectionHandler,
    ConsumeAgentConnectionContinuationHandler,
    ActivateAgentConnectionHandler,
    AuthorizeAgentConnectionHandler,
    UseAgentConnectionHandler,
    RevokeAgentConnectionHandler
);

#[derive(Debug, Error)]
pub enum AgentConnectionCommandError {
    #[error("agent connection request was rejected by policy")]
    Denied,
    #[error("a live agent connection already exists")]
    AlreadyExists,
    #[error("agent connection is unavailable")]
    Unavailable,
    #[error("agent connection request is invalid")]
    Invalid,
    #[error("repository error")]
    Repository(#[from] RepositoryError),
}
fn digest(value: &str) -> Sha256Digest {
    Sha256Digest::digest(value.as_bytes())
}
fn map_conflict(error: RepositoryError) -> AgentConnectionCommandError {
    if matches!(
        error,
        RepositoryError::Conflict(ConflictKind::AgentConnectionExists)
    ) {
        AgentConnectionCommandError::AlreadyExists
    } else {
        AgentConnectionCommandError::Repository(error)
    }
}

impl RequestAgentConnectionHandler {
    pub async fn handle(
        &self,
        command: RequestAgentConnection,
        _metadata: ExecutionMetadata,
    ) -> Result<AgentConnection, AgentConnectionCommandError> {
        let permissions = canonical_permissions(command.permissions)
            .map_err(|_| AgentConnectionCommandError::Invalid)?;
        if permissions.is_empty() {
            return Err(AgentConnectionCommandError::Invalid);
        }
        let now = Utc::now();
        let connection = AgentConnection::request(
            Uuid::new_v4().into(),
            Uuid::new_v4().into(),
            command.user_id,
            command.workspace_id,
            command.auth0_subject,
            command.auth0_client_id,
            command.client_display_name,
            command.resource,
            permissions,
            command.expires_at,
            digest(&command.continuation_token),
            digest(&command.nonce),
            now,
        )
        .map_err(|_| AgentConnectionCommandError::Invalid)?;
        self.repository
            .in_unit_of_work(async move |unit_of_work| {
                let auth0_sub = unit_of_work
                    .reads()
                    .users()
                    .auth0_sub(connection.user_id)
                    .await?
                    .ok_or(RepositoryError::InvariantViolation(
                        "agent connection requester must exist",
                    ))?;
                if auth0_sub != connection.auth0_subject
                    || unit_of_work
                        .get_membership_role(connection.workspace_id, connection.user_id)
                        .await?
                        .is_none()
                {
                    return Err(RepositoryError::InvariantViolation(
                        "agent connection requester is ineligible",
                    ));
                }
                unit_of_work
                    .aggregates()
                    .agent_connections()
                    .save(&connection)
                    .await?;
                Ok(connection)
            })
            .await
            .map_err(|error| match error {
                RepositoryError::InvariantViolation(
                    "agent connection requester must exist"
                    | "agent connection requester is ineligible",
                ) => AgentConnectionCommandError::Denied,
                error => map_conflict(error),
            })
    }
}

async fn continuation_connection(
    repository: &Postgres,
    continuation: Sha256Digest,
    nonce: Option<Sha256Digest>,
) -> Result<Option<AgentConnectionId>, RepositoryError> {
    let sql = if nonce.is_some() {
        "SELECT agent_connection_id FROM agent_authorization_transactions WHERE continuation_digest = $1 AND nonce_digest = $2"
    } else {
        "SELECT agent_connection_id FROM agent_authorization_transactions WHERE continuation_digest = $1"
    };
    let client = repository.get().await?;
    let row = match nonce {
        Some(nonce) => {
            client
                .query_typed_opt(
                    sql,
                    &[
                        param(&continuation.as_bytes().as_slice()),
                        param(&nonce.as_bytes().as_slice()),
                    ],
                )
                .await?
        }
        None => {
            client
                .query_typed_opt(sql, &[param(&continuation.as_bytes().as_slice())])
                .await?
        }
    };

    Ok(row.map(|row| AgentConnectionId::from(row.get::<_, Uuid>("agent_connection_id"))))
}
impl DenyAgentConnectionHandler {
    pub async fn handle(
        &self,
        command: DenyAgentConnection,
        _metadata: ExecutionMetadata,
    ) -> Result<bool, AgentConnectionCommandError> {
        let Some(id) =
            continuation_connection(&self.repository, digest(&command.continuation_token), None)
                .await?
        else {
            return Ok(false);
        };
        self.repository
            .in_unit_of_work(async move |unit_of_work| {
                let repository = unit_of_work.aggregates().agent_connections();
                let Some(mut connection) = repository.get(id).await? else {
                    return Ok(false);
                };
                if connection.status != crate::domain::AgentConnectionStatus::Pending {
                    return Ok(false);
                }
                connection.revoke(Utc::now()).map_err(|_| {
                    RepositoryError::InvariantViolation("agent connection denial must be valid")
                })?;
                repository.save(&connection).await?;
                Ok(true)
            })
            .await
            .map_err(Into::into)
    }
}
impl ConsumeAgentConnectionContinuationHandler {
    pub async fn handle(
        &self,
        command: ConsumeAgentConnectionContinuation,
        _metadata: ExecutionMetadata,
    ) -> Result<ConsumeAgentConnectionOutcome, AgentConnectionCommandError> {
        let continuation = digest(&command.continuation_token);
        let Some(id) =
            continuation_connection(&self.repository, continuation, Some(digest(&command.nonce)))
                .await?
        else {
            return Ok(ConsumeAgentConnectionOutcome::Invalid);
        };
        self.repository
            .in_unit_of_work(async move |unit_of_work| {
                let repository = unit_of_work.aggregates().agent_connections();
                let Some(mut connection) = repository.get(id).await? else {
                    return Ok(ConsumeAgentConnectionOutcome::Invalid);
                };
                let user_is_eligible = unit_of_work
                    .reads()
                    .users()
                    .auth0_sub(connection.user_id)
                    .await?
                    .is_some_and(|auth0_sub| auth0_sub == connection.auth0_subject)
                    && unit_of_work
                        .get_membership_role(connection.workspace_id, connection.user_id)
                        .await?
                        .is_some();
                if !user_is_eligible {
                    return Ok(ConsumeAgentConnectionOutcome::Invalid);
                }
                if connection.consume_continuation(continuation, digest(&command.nonce), Utc::now())
                    != AgentConnectionConsumption::Authorized
                {
                    return Ok(ConsumeAgentConnectionOutcome::Invalid);
                }
                repository.save(&connection).await?;
                Ok(ConsumeAgentConnectionOutcome::Approved(connection))
            })
            .await
            .map_err(Into::into)
    }
}
impl ActivateAgentConnectionHandler {
    pub async fn handle(
        &self,
        command: ActivateAgentConnection,
        _metadata: ExecutionMetadata,
    ) -> Result<ActivateAgentConnectionOutcome, AgentConnectionCommandError> {
        self.repository
            .in_unit_of_work(async move |unit_of_work| {
                let repository = unit_of_work.aggregates().agent_connections();
                let Some(mut connection) = repository.get(command.connection_id).await? else {
                    return Ok(ActivateAgentConnectionOutcome::Rejected);
                };
                if unit_of_work
                    .get_membership_role(connection.workspace_id, connection.user_id)
                    .await?
                    .is_none()
                {
                    return Ok(ActivateAgentConnectionOutcome::Rejected);
                }
                match connection.activate(Utc::now()) {
                    AgentConnectionActivation::Activated => {
                        repository.save(&connection).await?;
                        Ok(ActivateAgentConnectionOutcome::Activated(connection))
                    }
                    _ => Ok(ActivateAgentConnectionOutcome::Rejected),
                }
            })
            .await
            .map_err(Into::into)
    }
}
impl AuthorizeAgentConnectionHandler {
    pub async fn handle(
        &self,
        command: AuthorizeAgentConnection,
        metadata: ExecutionMetadata,
    ) -> Result<Option<AgentConnectionContext>, AgentConnectionCommandError> {
        let expected = canonical_permissions(command.permissions)
            .map_err(|_| AgentConnectionCommandError::Invalid)?;
        let result = self
            .repository
            .in_unit_of_work(async move |unit_of_work| {
                let repository = unit_of_work.aggregates().agent_connections();
                let Some(mut connection) = repository.get(command.connection_id).await? else {
                    return Ok(None);
                };
                if connection.workspace_id != command.workspace_id
                    || connection.auth0_subject != command.auth0_subject
                    || connection.auth0_client_id != command.auth0_client_id
                    || connection.resource != command.resource
                    || connection.permissions != expected
                {
                    return Ok(None);
                }
                let user_is_eligible = unit_of_work
                    .reads()
                    .users()
                    .auth0_sub(connection.user_id)
                    .await?
                    .is_some_and(|auth0_sub| auth0_sub == connection.auth0_subject)
                    && unit_of_work
                        .get_membership_role(connection.workspace_id, connection.user_id)
                        .await?
                        .is_some();
                if !user_is_eligible {
                    return Ok(None);
                }
                match connection.activate(Utc::now()) {
                    AgentConnectionActivation::Unavailable => return Ok(None),
                    AgentConnectionActivation::Activated => {}
                    AgentConnectionActivation::AlreadyActive => {}
                }
                if connection.use_at(Utc::now()) != AgentConnectionUse::Used {
                    return Ok(None);
                }
                let output = AgentConnectionContext {
                    user_id: connection.user_id,
                    connection_id: connection.id,
                    workspace_id: connection.workspace_id,
                    permissions: WorkspacePermissions::from_iter(
                        connection.permissions.iter().copied(),
                    ),
                };
                repository.save(&connection).await?;
                Ok(Some(output))
            })
            .await?;
        let _ = metadata;
        Ok(result)
    }
}
impl UseAgentConnectionHandler {
    pub async fn handle(
        &self,
        command: UseAgentConnection,
        _metadata: ExecutionMetadata,
    ) -> Result<bool, AgentConnectionCommandError> {
        self.repository
            .in_unit_of_work(async move |unit_of_work| {
                let repository = unit_of_work.aggregates().agent_connections();
                let Some(mut connection) = repository.get(command.connection_id).await? else {
                    return Ok(false);
                };
                if connection.use_at(Utc::now()) != AgentConnectionUse::Used {
                    return Ok(false);
                }
                repository.save(&connection).await?;
                Ok(true)
            })
            .await
            .map_err(Into::into)
    }
}
impl RevokeAgentConnectionHandler {
    pub async fn handle(
        &self,
        command: RevokeAgentConnection,
        _metadata: ExecutionMetadata,
    ) -> Result<bool, AgentConnectionCommandError> {
        self.repository
            .in_unit_of_work(async move |unit_of_work| {
                let repository = unit_of_work.aggregates().agent_connections();
                let Some(mut connection) = repository.get(command.connection_id).await? else {
                    return Ok(false);
                };
                if command
                    .user_id
                    .is_some_and(|user_id| user_id != connection.user_id)
                    || (command.user_id.is_some()
                        && !matches!(
                            connection.status,
                            crate::domain::AgentConnectionStatus::Authorized
                                | crate::domain::AgentConnectionStatus::Active
                        ))
                {
                    return Ok(false);
                }
                match connection.revoke(Utc::now()).map_err(|_| {
                    RepositoryError::InvariantViolation("agent connection revocation must be valid")
                })? {
                    AgentConnectionRevocation::Revoked => {
                        repository.save(&connection).await?;
                        Ok(true)
                    }
                    AgentConnectionRevocation::AlreadyRevoked => Ok(false),
                }
            })
            .await
            .map_err(Into::into)
    }
}
