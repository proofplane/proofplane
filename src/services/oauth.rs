use std::sync::Arc;

use async_trait::async_trait;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::{Duration as ChronoDuration, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;
use url::Url;
use uuid::Uuid;

use crate::{
    application::{
        commands::agent_connections::{
            AgentConnectionCommandError, AuthorizeAgentConnection, AuthorizeAgentConnectionHandler,
            ConsumeAgentConnectionContinuation, ConsumeAgentConnectionContinuationHandler,
            ConsumeAgentConnectionOutcome, RequestAgentConnection, RequestAgentConnectionHandler,
        },
        commands::oauth_authorization_flows::{
            ApproveOAuthAuthorization, ApproveOAuthAuthorizationHandler,
            AttachOAuthAuthorizationSubject, AttachOAuthAuthorizationSubjectHandler,
            CancelOAuthAuthorization, CancelOAuthAuthorizationHandler,
            ConsumeOAuthAuthorizationCode, ConsumeOAuthAuthorizationCodeHandler,
            OAuthAuthorizationCodeGrant, OAuthAuthorizationCommandError, RequestOAuthAuthorization,
            RequestOAuthAuthorizationHandler,
        },
        queries::agent_connections::{
            FindReusableAgentConnection, FindReusableAgentConnectionHandler,
        },
        queries::oauth_authorization_flows::{
            OAuthConsentContext as OAuthConsentContextProjection, ReadOAuthConsentContext,
            ReadOAuthConsentContextHandler,
        },
        ExecutionMetadata,
    },
    authentication::auth0::{TokenVerifier, VerifiedMcpClaims, VerifyError},
    authentication::paseto::{
        IssuedPasetoToken, McpOAuthDecryptor, McpOAuthEncryptor, RegisteredClaims,
    },
    domain::{
        canonical_permissions, OAuthAuthorizationFlow, OAuthAuthorizationRequest,
        OAuthAuthorizationRequestId, WorkspacePermission,
    },
    repository::{Error as RepositoryError, Postgres},
};

use super::client_resolver::{ClientResolutionError, ClientResolver};
use crate::authentication::client_registration::{RegisterClientPayload, RegisteredClient};

const AUTHORIZATION_CODE_TTL: ChronoDuration = ChronoDuration::minutes(5);
const AUTHORIZATION_REQUEST_TTL: ChronoDuration = ChronoDuration::minutes(10);
const ACCESS_TOKEN_TTL: ChronoDuration = ChronoDuration::hours(24);

#[derive(Clone)]
pub struct OAuthService {
    request_authorization: RequestOAuthAuthorizationHandler,
    attach_authorization_subject: AttachOAuthAuthorizationSubjectHandler,
    cancel_authorization: CancelOAuthAuthorizationHandler,
    approve_authorization: ApproveOAuthAuthorizationHandler,
    consume_authorization_code: ConsumeOAuthAuthorizationCodeHandler,
    read_consent_context: ReadOAuthConsentContextHandler,
    find_reusable_agent_connection: FindReusableAgentConnectionHandler,
    request_agent_connection: RequestAgentConnectionHandler,
    consume_agent_connection: ConsumeAgentConnectionContinuationHandler,
    authorize_agent_connection: AuthorizeAgentConnectionHandler,
    clients: ClientResolver,
    resource: Url,
    token_encryptor: McpOAuthEncryptor,
    token_decryptor: McpOAuthDecryptor,
}

#[derive(Debug, Error)]
pub enum OAuthError {
    #[error("OAuth request was invalid")]
    InvalidRequest,
    #[error("OAuth client was not found")]
    InvalidClient,
    #[error("OAuth grant was invalid")]
    InvalidGrant,
    #[error("OAuth dependency failed")]
    Repository(#[from] RepositoryError),
    #[error("agent connection authorization failed")]
    AgentConnection(#[from] AgentConnectionCommandError),
    #[error("MCP OAuth token operation failed")]
    Token(#[from] crate::authentication::paseto::Error),
    #[error("client id could not be resolved")]
    ClientResolution(#[from] ClientResolutionError),
    #[error("random generation failed")]
    Random(#[from] getrandom::Error),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorizePayload {
    pub client_id: String,
    pub redirect_uri: String,
    pub code_challenge: String,
    pub scopes: Vec<WorkspacePermission>,
    pub state: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedAuthorization {
    pub request: OAuthAuthorizationRequest,
    pub csrf_token: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallbackOutcome {
    Reusable { redirect_uri: Url },
    ConsentRequired { context: Box<OAuthConsentContext> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OAuthConsentContext {
    pub request_id: OAuthAuthorizationRequestId,
    pub client_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApproveConsentPayload {
    pub request_id: OAuthAuthorizationRequestId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenPayload {
    pub client_id: String,
    pub redirect_uri: String,
    pub code: String,
    pub code_verifier: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpOAuthClaims {
    pub auth0_subject: String,
    pub client_id: String,
    pub connection_id: Uuid,
    pub workspace_id: Uuid,
    pub resource: String,
    pub scopes: Vec<String>,
}

impl OAuthService {
    pub fn new(
        repository: Arc<Postgres>,
        _issuer: Url,
        resource: Url,
        clients: ClientResolver,
        token_encryptor: McpOAuthEncryptor,
        token_decryptor: McpOAuthDecryptor,
    ) -> Self {
        Self {
            request_authorization: RequestOAuthAuthorizationHandler::new(repository.clone()),
            attach_authorization_subject: AttachOAuthAuthorizationSubjectHandler::new(
                repository.clone(),
            ),
            cancel_authorization: CancelOAuthAuthorizationHandler::new(repository.clone()),
            approve_authorization: ApproveOAuthAuthorizationHandler::new(repository.clone()),
            consume_authorization_code: ConsumeOAuthAuthorizationCodeHandler::new(
                repository.clone(),
            ),
            read_consent_context: ReadOAuthConsentContextHandler::new(repository.clone()),
            find_reusable_agent_connection: FindReusableAgentConnectionHandler::new(
                repository.clone(),
            ),
            request_agent_connection: RequestAgentConnectionHandler::new(repository.clone()),
            consume_agent_connection: ConsumeAgentConnectionContinuationHandler::new(
                repository.clone(),
            ),
            authorize_agent_connection: AuthorizeAgentConnectionHandler::new(repository.clone()),
            clients,
            resource,
            token_encryptor,
            token_decryptor,
        }
    }

    pub fn register_client(&self, payload: RegisterClientPayload) -> RegisteredClient {
        self.clients.register(payload)
    }

    pub async fn prepare_authorization(
        &self,
        payload: AuthorizePayload,
    ) -> Result<PreparedAuthorization, OAuthError> {
        let client = self.clients.resolve(&payload.client_id).await?;
        if !client
            .redirect_uris
            .iter()
            .any(|uri| redirect_uri_matches(uri, &payload.redirect_uri))
        {
            return Err(OAuthError::InvalidRequest);
        }
        let csrf_token = random_secret(32)?;
        let now = Utc::now();
        let flow = self
            .request_authorization
            .handle(
                RequestOAuthAuthorization {
                    request_id: OAuthAuthorizationRequestId::from(Uuid::new_v4()),
                    client_id: payload.client_id,
                    client_name: client.client_name,
                    redirect_uri: payload.redirect_uri,
                    code_challenge: payload.code_challenge,
                    state: payload.state.unwrap_or_default(),
                    resource: self.resource.to_string(),
                    scopes: payload.scopes,
                    csrf_token: csrf_token.clone(),
                    created_at: now,
                    expires_at: now + AUTHORIZATION_REQUEST_TTL,
                },
                ExecutionMetadata::background(),
            )
            .await
            .map_err(request_authorization_error)?;
        Ok(PreparedAuthorization {
            request: request_from_flow(&flow)?,
            csrf_token,
        })
    }

    pub async fn complete_upstream_login(
        &self,
        csrf_token: &str,
        auth0_subject: String,
        user_id: crate::domain::UserId,
    ) -> Result<CallbackOutcome, OAuthError> {
        let flow = self
            .attach_authorization_subject
            .handle(
                AttachOAuthAuthorizationSubject {
                    csrf_token: csrf_token.to_owned(),
                    auth0_subject: auth0_subject.clone(),
                    user_id,
                    attached_at: Utc::now(),
                },
                ExecutionMetadata::background(),
            )
            .await
            .map_err(authorization_grant_error)?;
        if let Some(connection) = self
            .find_reusable_agent_connection
            .handle(FindReusableAgentConnection {
                auth0_subject,
                auth0_client_id: flow.client_id().to_owned(),
                resource: flow.resource().to_owned(),
                permissions: flow.scopes().to_vec(),
            })
            .await?
        {
            return Ok(CallbackOutcome::Reusable {
                redirect_uri: self
                    .issue_code_redirect(&flow, connection.id, connection.workspace_id)
                    .await?,
            });
        }
        Ok(CallbackOutcome::ConsentRequired {
            context: Box::new(self.consent_context(flow.id()).await?),
        })
    }

    pub async fn consent_context_by_request_id(
        &self,
        request_id: OAuthAuthorizationRequestId,
    ) -> Result<OAuthConsentContext, OAuthError> {
        self.consent_context(request_id).await
    }

    pub async fn approve_consent(&self, payload: ApproveConsentPayload) -> Result<Url, OAuthError> {
        let context = self.read_consent_context(payload.request_id).await?;
        let continuation_token = random_secret(32)?;
        let nonce = random_secret(32)?;
        self.request_agent_connection
            .handle(
                RequestAgentConnection {
                    user_id: context.user_id,
                    workspace_id: context.workspace_id,
                    auth0_subject: context.auth0_subject.clone(),
                    auth0_client_id: context.client_id.clone(),
                    client_display_name: context.client_name.clone(),
                    resource: context.resource.clone(),
                    permissions: context.scopes.clone(),
                    expires_at: context.expires_at,
                    continuation_token: continuation_token.clone(),
                    nonce: nonce.clone(),
                },
                ExecutionMetadata::background(),
            )
            .await?;
        let connection = match self
            .consume_agent_connection
            .handle(
                ConsumeAgentConnectionContinuation {
                    continuation_token,
                    nonce,
                },
                ExecutionMetadata::background(),
            )
            .await?
        {
            ConsumeAgentConnectionOutcome::Approved(connection) => connection,
            ConsumeAgentConnectionOutcome::Invalid => return Err(OAuthError::InvalidGrant),
        };
        self.issue_code_redirect_from_context(&context, connection.id, connection.workspace_id)
            .await
    }

    pub async fn cancel_consent(
        &self,
        request_id: OAuthAuthorizationRequestId,
    ) -> Result<Url, OAuthError> {
        let flow = self
            .cancel_authorization
            .handle(
                CancelOAuthAuthorization {
                    request_id,
                    cancelled_at: Utc::now(),
                },
                ExecutionMetadata::background(),
            )
            .await
            .map_err(authorization_grant_error)?;
        redirect_with_error(flow.redirect_uri(), "access_denied", flow.state())
    }

    pub async fn issue_access_token(
        &self,
        payload: TokenPayload,
    ) -> Result<IssuedPasetoToken, OAuthError> {
        let code = self
            .consume_authorization_code
            .handle(
                ConsumeOAuthAuthorizationCode {
                    code: payload.code,
                    client_id: payload.client_id,
                    redirect_uri: payload.redirect_uri,
                    consumed_at: Utc::now(),
                },
                ExecutionMetadata::background(),
            )
            .await
            .map_err(authorization_grant_error)?;
        if pkce_challenge(&payload.code_verifier) != code.code_challenge {
            return Err(OAuthError::InvalidGrant);
        }
        self.issue_token_for_code(code).await
    }

    pub fn verify_access_token(&self, token: &str) -> Result<VerifiedMcpClaims, VerifyError> {
        let verified = self
            .token_decryptor
            .decrypt::<McpOAuthClaims>(token)
            .map_err(|_| VerifyError::InvalidToken)?;
        let scopes = parse_scope(&verified.claims.scopes.join(" "))
            .map_err(|_| VerifyError::InvalidScopes)?;
        Ok(VerifiedMcpClaims {
            subject: verified.claims.auth0_subject,
            client_id: verified.claims.client_id,
            scopes,
            connection_id: Some(verified.claims.connection_id.into()),
            workspace_id: Some(verified.claims.workspace_id.into()),
        })
    }

    async fn issue_code_redirect(
        &self,
        flow: &OAuthAuthorizationFlow,
        connection_id: crate::domain::AgentConnectionId,
        workspace_id: crate::domain::WorkspaceId,
    ) -> Result<Url, OAuthError> {
        let code = random_secret(32)?;
        let now = Utc::now();
        self.approve_authorization
            .handle(
                ApproveOAuthAuthorization {
                    request_id: flow.id(),
                    code: code.clone(),
                    agent_connection_id: connection_id,
                    workspace_id,
                    approved_at: now,
                    code_expires_at: now + AUTHORIZATION_CODE_TTL,
                },
                ExecutionMetadata::background(),
            )
            .await
            .map_err(authorization_grant_error)?;
        redirect_with_code(flow.redirect_uri(), &code, flow.state())
    }

    async fn issue_code_redirect_from_context(
        &self,
        context: &OAuthConsentContextProjection,
        connection_id: crate::domain::AgentConnectionId,
        workspace_id: crate::domain::WorkspaceId,
    ) -> Result<Url, OAuthError> {
        let code = random_secret(32)?;
        let now = Utc::now();
        self.approve_authorization
            .handle(
                ApproveOAuthAuthorization {
                    request_id: context.request_id,
                    code: code.clone(),
                    agent_connection_id: connection_id,
                    workspace_id,
                    approved_at: now,
                    code_expires_at: now + AUTHORIZATION_CODE_TTL,
                },
                ExecutionMetadata::background(),
            )
            .await
            .map_err(authorization_grant_error)?;
        redirect_with_code(&context.redirect_uri, &code, &context.state)
    }

    async fn read_consent_context(
        &self,
        request_id: OAuthAuthorizationRequestId,
    ) -> Result<OAuthConsentContextProjection, OAuthError> {
        self.read_consent_context
            .handle(ReadOAuthConsentContext {
                request_id,
                now: Utc::now(),
            })
            .await?
            .ok_or(OAuthError::InvalidGrant)
    }

    async fn consent_context(
        &self,
        request_id: OAuthAuthorizationRequestId,
    ) -> Result<OAuthConsentContext, OAuthError> {
        let context = self.read_consent_context(request_id).await?;
        Ok(OAuthConsentContext {
            request_id: context.request_id,
            client_name: context.client_name,
        })
    }

    async fn issue_token_for_code(
        &self,
        code: OAuthAuthorizationCodeGrant,
    ) -> Result<IssuedPasetoToken, OAuthError> {
        let Some(context) = self
            .authorize_agent_connection
            .handle(
                AuthorizeAgentConnection {
                    connection_id: code.agent_connection_id,
                    workspace_id: code.workspace_id,
                    auth0_subject: code.auth0_subject.clone(),
                    auth0_client_id: code.client_id.clone(),
                    resource: code.resource.clone(),
                    permissions: code.scopes.clone(),
                },
                ExecutionMetadata::background(),
            )
            .await?
        else {
            return Err(OAuthError::InvalidGrant);
        };
        let scopes = code
            .scopes
            .iter()
            .map(|scope| scope.as_str().to_owned())
            .collect::<Vec<_>>();
        Ok(self.token_encryptor.encrypt(
            RegisteredClaims {
                subject: context.user_id.into(),
                token_id: Uuid::new_v4(),
                expires_at: Utc::now() + ACCESS_TOKEN_TTL,
            },
            &McpOAuthClaims {
                auth0_subject: code.auth0_subject,
                client_id: code.client_id,
                connection_id: context.connection_id.into(),
                workspace_id: context.workspace_id.into(),
                resource: code.resource,
                scopes,
            },
        )?)
    }
}

fn request_from_flow(
    flow: &OAuthAuthorizationFlow,
) -> Result<OAuthAuthorizationRequest, OAuthError> {
    Ok(OAuthAuthorizationRequest {
        id: flow.id(),
        client_id: flow.client_id().to_owned(),
        client_name: flow.client_name().to_owned(),
        redirect_uri: flow.redirect_uri().to_owned(),
        code_challenge: flow.code_challenge().to_owned(),
        state: flow.state().to_owned(),
        resource: flow.resource().to_owned(),
        scopes: flow.scopes().to_vec(),
        auth0_subject: flow.auth0_subject().map(str::to_owned),
        user_id: flow.user_id(),
        expires_at: flow.expires_at(),
    })
}

fn request_authorization_error(error: OAuthAuthorizationCommandError) -> OAuthError {
    match error {
        OAuthAuthorizationCommandError::Unavailable | OAuthAuthorizationCommandError::Invalid => {
            OAuthError::InvalidRequest
        }
        OAuthAuthorizationCommandError::Repository(error) => OAuthError::Repository(error),
    }
}

fn authorization_grant_error(error: OAuthAuthorizationCommandError) -> OAuthError {
    match error {
        OAuthAuthorizationCommandError::Unavailable | OAuthAuthorizationCommandError::Invalid => {
            OAuthError::InvalidGrant
        }
        OAuthAuthorizationCommandError::Repository(error) => OAuthError::Repository(error),
    }
}

#[async_trait]
impl TokenVerifier for OAuthService {
    type Claims = VerifiedMcpClaims;

    async fn verify(&self, token: &str) -> Result<VerifiedMcpClaims, VerifyError> {
        self.verify_access_token(token)
    }
}

pub fn parse_scope(scope: &str) -> Result<Vec<WorkspacePermission>, OAuthError> {
    let values = scope.split_ascii_whitespace().collect::<Vec<_>>();
    if values.is_empty() {
        return Err(OAuthError::InvalidRequest);
    }
    let permissions = values
        .into_iter()
        .map(str::parse)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| OAuthError::InvalidRequest)?;
    canonical_permissions(permissions).map_err(|_| OAuthError::InvalidRequest)
}

pub fn pkce_challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

fn redirect_with_code(redirect_uri: &str, code: &str, state: &str) -> Result<Url, OAuthError> {
    let mut url = Url::parse(redirect_uri).map_err(|_| OAuthError::InvalidRequest)?;
    url.query_pairs_mut().append_pair("code", code);
    if !state.is_empty() {
        url.query_pairs_mut().append_pair("state", state);
    }
    Ok(url)
}

fn redirect_with_error(redirect_uri: &str, error: &str, state: &str) -> Result<Url, OAuthError> {
    let mut url = Url::parse(redirect_uri).map_err(|_| OAuthError::InvalidRequest)?;
    url.query_pairs_mut().append_pair("error", error);
    if !state.is_empty() {
        url.query_pairs_mut().append_pair("state", state);
    }
    Ok(url)
}

/// Whether a redirect_uri requested at authorize time matches one the client
/// declared in its metadata document. HTTPS redirects must match exactly;
/// loopback redirects match ignoring the port because native
/// clients bind an ephemeral OS-assigned port they cannot know when publishing.
fn redirect_uri_matches(declared: &str, requested: &str) -> bool {
    if declared == requested {
        return true;
    }
    let (Ok(declared_url), Ok(requested_url)) = (Url::parse(declared), Url::parse(requested))
    else {
        return false;
    };
    if is_loopback_redirect(&declared_url) && is_loopback_redirect(&requested_url) {
        return declared_url.scheme() == requested_url.scheme()
            && declared_url.host_str() == requested_url.host_str()
            && declared_url.path() == requested_url.path()
            && declared_url.query() == requested_url.query();
    }
    false
}

fn is_loopback_redirect(url: &Url) -> bool {
    url.scheme() == "http"
        && url
            .host_str()
            .is_some_and(|host| matches!(host, "127.0.0.1" | "localhost" | "::1" | "[::1]"))
}

pub fn valid_redirect_uri(uri: &str) -> bool {
    let Ok(url) = Url::parse(uri) else {
        return false;
    };
    match url.scheme() {
        "https" => true,
        "http" => url
            .host_str()
            .is_some_and(|host| matches!(host, "127.0.0.1" | "localhost" | "::1")),
        _ => false,
    }
}

fn random_secret(bytes_len: usize) -> Result<String, getrandom::Error> {
    let mut bytes = vec![0_u8; bytes_len];
    getrandom::fill(&mut bytes)?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

#[cfg(test)]
mod tests {
    use super::redirect_uri_matches;

    #[test]
    fn https_redirects_must_match_exactly() {
        assert!(redirect_uri_matches(
            "https://chatgpt.com/connector/oauth/abc",
            "https://chatgpt.com/connector/oauth/abc",
        ));
        assert!(!redirect_uri_matches(
            "https://chatgpt.com/connector/oauth/abc",
            "https://chatgpt.com/connector/oauth/xyz",
        ));
        // A different port on an HTTPS redirect is not a loopback allowance.
        assert!(!redirect_uri_matches(
            "https://client.example/cb",
            "https://client.example:8443/cb",
        ));
    }

    #[test]
    fn loopback_redirects_ignore_the_port() {
        assert!(redirect_uri_matches(
            "http://127.0.0.1/callback",
            "http://127.0.0.1:52731/callback",
        ));
        assert!(redirect_uri_matches(
            "http://localhost:1/callback",
            "http://localhost:65535/callback",
        ));
        // Path and query still have to line up.
        assert!(!redirect_uri_matches(
            "http://127.0.0.1/callback",
            "http://127.0.0.1:52731/other",
        ));
        // The loopback allowance does not bridge different loopback hosts.
        assert!(!redirect_uri_matches(
            "http://127.0.0.1/callback",
            "http://localhost:52731/callback",
        ));
    }
}
