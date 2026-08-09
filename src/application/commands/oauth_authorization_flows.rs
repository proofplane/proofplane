use std::sync::Arc;

use chrono::{DateTime, Utc};

use crate::{
    application::ExecutionMetadata,
    domain::{
        canonical_permissions, AgentConnectionId, OAuthAuthorizationFlow,
        OAuthAuthorizationRequestId, Sha256Digest, UserId, WorkspaceId, WorkspacePermission,
    },
    repository::{Error as RepositoryError, Postgres},
};

#[derive(Debug, Clone)]
pub struct RequestOAuthAuthorization {
    pub request_id: OAuthAuthorizationRequestId,
    pub client_id: String,
    pub client_name: String,
    pub redirect_uri: String,
    pub code_challenge: String,
    pub state: String,
    pub resource: String,
    pub scopes: Vec<WorkspacePermission>,
    pub csrf_token: String,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}
#[derive(Debug, Clone)]
pub struct AttachOAuthAuthorizationSubject {
    pub csrf_token: String,
    pub auth0_subject: String,
    pub user_id: UserId,
    pub attached_at: DateTime<Utc>,
}
#[derive(Debug, Clone, Copy)]
pub struct CancelOAuthAuthorization {
    pub request_id: OAuthAuthorizationRequestId,
    pub cancelled_at: DateTime<Utc>,
}
#[derive(Debug, Clone)]
pub struct ApproveOAuthAuthorization {
    pub request_id: OAuthAuthorizationRequestId,
    pub code: String,
    pub agent_connection_id: AgentConnectionId,
    pub workspace_id: WorkspaceId,
    pub approved_at: DateTime<Utc>,
    pub code_expires_at: DateTime<Utc>,
}
#[derive(Debug, Clone)]
pub struct ConsumeOAuthAuthorizationCode {
    pub code: String,
    pub client_id: String,
    pub redirect_uri: String,
    pub consumed_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OAuthAuthorizationCodeGrant {
    pub request_id: OAuthAuthorizationRequestId,
    pub agent_connection_id: AgentConnectionId,
    pub workspace_id: WorkspaceId,
    pub client_id: String,
    pub redirect_uri: String,
    pub code_challenge: String,
    pub resource: String,
    pub scopes: Vec<WorkspacePermission>,
    pub auth0_subject: String,
    pub user_id: UserId,
    pub expires_at: DateTime<Utc>,
}

#[derive(Clone)]
pub struct RequestOAuthAuthorizationHandler {
    repository: Arc<Postgres>,
}
#[derive(Clone)]
pub struct AttachOAuthAuthorizationSubjectHandler {
    repository: Arc<Postgres>,
}
#[derive(Clone)]
pub struct CancelOAuthAuthorizationHandler {
    repository: Arc<Postgres>,
}
#[derive(Clone)]
pub struct ApproveOAuthAuthorizationHandler {
    repository: Arc<Postgres>,
}
#[derive(Clone)]
pub struct ConsumeOAuthAuthorizationCodeHandler {
    repository: Arc<Postgres>,
}

macro_rules! handlers { ($($handler:ident),+ $(,)?) => { $(impl $handler { pub fn new(repository: Arc<Postgres>) -> Self { Self { repository } } })+ }; }
handlers!(
    RequestOAuthAuthorizationHandler,
    AttachOAuthAuthorizationSubjectHandler,
    CancelOAuthAuthorizationHandler,
    ApproveOAuthAuthorizationHandler,
    ConsumeOAuthAuthorizationCodeHandler
);

#[derive(Debug, thiserror::Error)]
pub enum OAuthAuthorizationCommandError {
    #[error("OAuth authorization flow is unavailable")]
    Unavailable,
    #[error("OAuth authorization command is invalid")]
    Invalid,
    #[error("OAuth authorization persistence failed")]
    Repository(#[from] RepositoryError),
}

impl RequestOAuthAuthorizationHandler {
    pub async fn handle(
        &self,
        command: RequestOAuthAuthorization,
        _metadata: ExecutionMetadata,
    ) -> Result<OAuthAuthorizationFlow, OAuthAuthorizationCommandError> {
        let scopes = canonical_permissions(command.scopes)
            .map_err(|_| OAuthAuthorizationCommandError::Invalid)?;
        let flow = OAuthAuthorizationFlow::request(
            command.request_id,
            command.client_id,
            command.client_name,
            command.redirect_uri,
            command.code_challenge,
            command.state,
            command.resource,
            scopes,
            Sha256Digest::digest(command.csrf_token.as_bytes()),
            command.created_at,
            command.expires_at,
        )
        .map_err(|_| OAuthAuthorizationCommandError::Invalid)?;
        self.repository
            .in_unit_of_work(async move |context| {
                let repository = context.oauth_authorization_flows();
                if let Some(existing) = repository.get(flow.id()).await? {
                    if existing == flow {
                        return Ok(existing);
                    }
                    return Err(RepositoryError::InvariantViolation(
                        "OAuth authorization request replay changed its intent",
                    ));
                }
                repository.save(&flow).await?;
                Ok(flow)
            })
            .await
            .map_err(Into::into)
    }
}
impl AttachOAuthAuthorizationSubjectHandler {
    pub async fn handle(
        &self,
        command: AttachOAuthAuthorizationSubject,
        _metadata: ExecutionMetadata,
    ) -> Result<OAuthAuthorizationFlow, OAuthAuthorizationCommandError> {
        let digest = Sha256Digest::digest(command.csrf_token.as_bytes());
        self.repository
            .in_unit_of_work(async move |context| {
                let repository = context.oauth_authorization_flows();
                let Some(mut flow) = repository.get_by_csrf_digest(digest).await? else {
                    return Ok(None);
                };
                if flow
                    .attach_subject(command.auth0_subject, command.user_id, command.attached_at)
                    .is_err()
                {
                    return Ok(None);
                }
                repository.save(&flow).await?;
                Ok(Some(flow))
            })
            .await?
            .ok_or(OAuthAuthorizationCommandError::Unavailable)
    }
}
impl CancelOAuthAuthorizationHandler {
    pub async fn handle(
        &self,
        command: CancelOAuthAuthorization,
        _metadata: ExecutionMetadata,
    ) -> Result<OAuthAuthorizationFlow, OAuthAuthorizationCommandError> {
        self.repository
            .in_unit_of_work(async move |context| {
                let repository = context.oauth_authorization_flows();
                let Some(mut flow) = repository.get(command.request_id).await? else {
                    return Ok(None);
                };
                if flow.cancel(command.cancelled_at).is_err() {
                    return Ok(None);
                }
                repository.save(&flow).await?;
                Ok(Some(flow))
            })
            .await?
            .ok_or(OAuthAuthorizationCommandError::Unavailable)
    }
}
impl ApproveOAuthAuthorizationHandler {
    pub async fn handle(
        &self,
        command: ApproveOAuthAuthorization,
        _metadata: ExecutionMetadata,
    ) -> Result<OAuthAuthorizationFlow, OAuthAuthorizationCommandError> {
        let code_digest = Sha256Digest::digest(command.code.as_bytes());
        self.repository
            .in_unit_of_work(async move |context| {
                let repository = context.oauth_authorization_flows();
                let Some(mut flow) = repository.get(command.request_id).await? else {
                    return Ok(None);
                };
                if flow
                    .approve_and_issue_code(
                        code_digest,
                        command.agent_connection_id,
                        command.workspace_id,
                        command.approved_at,
                        command.code_expires_at,
                    )
                    .is_err()
                {
                    return Ok(None);
                }
                repository.save(&flow).await?;
                Ok(Some(flow))
            })
            .await?
            .ok_or(OAuthAuthorizationCommandError::Unavailable)
    }
}
impl ConsumeOAuthAuthorizationCodeHandler {
    pub async fn handle(
        &self,
        command: ConsumeOAuthAuthorizationCode,
        _metadata: ExecutionMetadata,
    ) -> Result<OAuthAuthorizationCodeGrant, OAuthAuthorizationCommandError> {
        let digest = Sha256Digest::digest(command.code.as_bytes());
        self.repository
            .in_unit_of_work(async move |context| {
                let repository = context.oauth_authorization_flows();
                let Some(mut flow) = repository.get_by_code_digest(digest).await? else {
                    return Ok(None);
                };
                if flow
                    .consume_code(
                        &command.client_id,
                        &command.redirect_uri,
                        command.consumed_at,
                    )
                    .is_err()
                {
                    return Ok(None);
                }
                let code = flow
                    .authorization_code()
                    .ok_or(RepositoryError::InvariantViolation(
                        "consumed OAuth flow must contain a code",
                    ))?;
                let grant = OAuthAuthorizationCodeGrant {
                    request_id: flow.id(),
                    agent_connection_id: code.agent_connection_id(),
                    workspace_id: code.workspace_id(),
                    client_id: flow.client_id().to_owned(),
                    redirect_uri: flow.redirect_uri().to_owned(),
                    code_challenge: flow.code_challenge().to_owned(),
                    resource: flow.resource().to_owned(),
                    scopes: flow.scopes().to_vec(),
                    auth0_subject: flow
                        .auth0_subject()
                        .ok_or(RepositoryError::InvariantViolation(
                            "OAuth code missing subject",
                        ))?
                        .to_owned(),
                    user_id: flow.user_id().ok_or(RepositoryError::InvariantViolation(
                        "OAuth code missing user",
                    ))?,
                    expires_at: code.expires_at(),
                };
                repository.save(&flow).await?;
                Ok(Some(grant))
            })
            .await?
            .ok_or(OAuthAuthorizationCommandError::Unavailable)
    }
}
