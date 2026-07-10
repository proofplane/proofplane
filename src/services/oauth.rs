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
    authentication::auth0::{TokenVerifier, VerifiedMcpClaims, VerifyError},
    authentication::paseto::{
        IssuedPasetoToken, McpOAuthDecryptor, McpOAuthEncryptor, RegisteredClaims,
    },
    domain::{
        canonical_permissions, AgentConnection, NewOAuthAuthorizationCode,
        NewOAuthAuthorizationRequest, NewOAuthClient, OAuthAuthorizationCode,
        OAuthAuthorizationRequest, OAuthAuthorizationRequestId, OAuthClient, WorkspacePermission,
    },
    repository::{Error as RepositoryError, Postgres},
};

use super::agent_connections::{
    AgentConnectionError, AgentConnectionService, AuthorizeMcpConnectionPayload,
    ConsumeContinuationOutcome, ConsumeContinuationPayload, CreatePendingConnectionPayload,
    FindReusableConnectionPayload,
};

const AUTHORIZATION_CODE_TTL: ChronoDuration = ChronoDuration::minutes(5);
const AUTHORIZATION_REQUEST_TTL: ChronoDuration = ChronoDuration::minutes(10);
const ACCESS_TOKEN_TTL: ChronoDuration = ChronoDuration::hours(24);

#[derive(Clone)]
pub struct OAuthService {
    repository: Arc<Postgres>,
    agent_connections: AgentConnectionService,
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
    AgentConnection(#[from] AgentConnectionError),
    #[error("MCP OAuth token operation failed")]
    Token(#[from] crate::authentication::paseto::Error),
    #[error("random generation failed")]
    Random(#[from] getrandom::Error),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisterClientPayload {
    pub client_name: String,
    pub redirect_uris: Vec<String>,
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
        token_encryptor: McpOAuthEncryptor,
        token_decryptor: McpOAuthDecryptor,
    ) -> Self {
        let agent_connections = AgentConnectionService::new(repository.clone());
        Self {
            repository,
            agent_connections,
            resource,
            token_encryptor,
            token_decryptor,
        }
    }

    pub async fn register_client(
        &self,
        payload: RegisterClientPayload,
    ) -> Result<OAuthClient, OAuthError> {
        self.repository
            .create_oauth_client(&NewOAuthClient {
                id: format!("ppoc_{}", random_secret(24)?),
                client_name: payload.client_name,
                redirect_uris: payload.redirect_uris,
            })
            .await
            .map_err(OAuthError::Repository)
    }

    pub async fn prepare_authorization(
        &self,
        payload: AuthorizePayload,
    ) -> Result<PreparedAuthorization, OAuthError> {
        let client = self
            .repository
            .get_oauth_client(&payload.client_id)
            .await?
            .ok_or(OAuthError::InvalidClient)?;
        if !client
            .redirect_uris
            .iter()
            .any(|uri| uri == &payload.redirect_uri)
        {
            return Err(OAuthError::InvalidRequest);
        }
        let csrf_token = random_secret(32)?;
        Ok(PreparedAuthorization {
            request: self
                .repository
                .create_oauth_authorization_request(&NewOAuthAuthorizationRequest {
                    id: OAuthAuthorizationRequestId::from(Uuid::new_v4()),
                    client_id: client.id,
                    redirect_uri: payload.redirect_uri,
                    code_challenge: payload.code_challenge,
                    state: payload.state.unwrap_or_default(),
                    resource: self.resource.to_string(),
                    scopes: payload.scopes,
                    csrf_token: csrf_token.clone(),
                    expires_at: Utc::now() + AUTHORIZATION_REQUEST_TTL,
                })
                .await?,
            csrf_token,
        })
    }

    pub async fn complete_upstream_login(
        &self,
        csrf_token: &str,
        auth0_subject: String,
        user_id: crate::domain::UserId,
    ) -> Result<CallbackOutcome, OAuthError> {
        let request = self
            .repository
            .get_oauth_authorization_request_by_csrf(csrf_token)
            .await?
            .ok_or(OAuthError::InvalidGrant)?;
        let request = self
            .repository
            .attach_oauth_authorization_subject(request.id, &auth0_subject, user_id)
            .await?
            .ok_or(OAuthError::InvalidGrant)?;
        if let Some(connection) = self
            .agent_connections
            .find_reusable(FindReusableConnectionPayload {
                auth0_subject,
                auth0_client_id: request.client_id.clone(),
                resource: request.resource.clone(),
                permissions: request.scopes.clone(),
            })
            .await?
        {
            return Ok(CallbackOutcome::Reusable {
                redirect_uri: self.issue_code_redirect(&request, connection).await?,
            });
        }
        Ok(CallbackOutcome::ConsentRequired {
            context: Box::new(self.consent_context(request).await?),
        })
    }

    pub async fn consent_context_by_request_id(
        &self,
        request_id: OAuthAuthorizationRequestId,
    ) -> Result<OAuthConsentContext, OAuthError> {
        let request = self
            .repository
            .get_oauth_authorization_request(request_id)
            .await?
            .ok_or(OAuthError::InvalidGrant)?;
        self.consent_context(request).await
    }

    pub async fn approve_consent(&self, payload: ApproveConsentPayload) -> Result<Url, OAuthError> {
        let request = self
            .repository
            .get_oauth_authorization_request(payload.request_id)
            .await
            .map_err(OAuthError::Repository)?
            .ok_or(OAuthError::InvalidGrant)?;
        let auth0_subject = request
            .auth0_subject
            .clone()
            .ok_or(OAuthError::InvalidGrant)?;
        let user_id = request.user_id.ok_or(OAuthError::InvalidGrant)?;
        let client = self
            .repository
            .get_oauth_client(&request.client_id)
            .await?
            .ok_or(OAuthError::InvalidClient)?;
        let workspace = self
            .repository
            .get_workspace_with_role_for_user(user_id)
            .await?
            .ok_or(OAuthError::InvalidGrant)?;
        let continuation_token = random_secret(32)?;
        let nonce = random_secret(32)?;
        self.agent_connections
            .create_pending(CreatePendingConnectionPayload {
                user_id,
                workspace_id: workspace.workspace.id,
                auth0_subject: auth0_subject.clone(),
                auth0_client_id: request.client_id.clone(),
                client_display_name: client.client_name,
                resource: request.resource.clone(),
                permissions: request.scopes.clone(),
                expires_at: request.expires_at,
                continuation_token: continuation_token.clone(),
                nonce: nonce.clone(),
            })
            .await?;
        let connection = match self
            .agent_connections
            .consume_continuation(ConsumeContinuationPayload {
                continuation_token,
                nonce,
            })
            .await?
        {
            ConsumeContinuationOutcome::Approved(connection) => connection,
            ConsumeContinuationOutcome::Invalid => return Err(OAuthError::InvalidGrant),
        };
        self.issue_code_redirect(&request, connection).await
    }

    pub async fn cancel_consent(
        &self,
        request_id: OAuthAuthorizationRequestId,
    ) -> Result<Url, OAuthError> {
        let request = self
            .repository
            .consume_oauth_authorization_request(request_id)
            .await?
            .ok_or(OAuthError::InvalidGrant)?;
        redirect_with_error(&request.redirect_uri, "access_denied", &request.state)
    }

    pub async fn issue_access_token(
        &self,
        payload: TokenPayload,
    ) -> Result<IssuedPasetoToken, OAuthError> {
        let code = self
            .repository
            .consume_oauth_authorization_code(
                &payload.code,
                &payload.client_id,
                &payload.redirect_uri,
            )
            .await?
            .ok_or(OAuthError::InvalidGrant)?;
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
        request: &OAuthAuthorizationRequest,
        connection: AgentConnection,
    ) -> Result<Url, OAuthError> {
        let code = random_secret(32)?;
        self.repository
            .create_oauth_authorization_code(&NewOAuthAuthorizationCode {
                code: code.clone(),
                request_id: request.id,
                agent_connection_id: connection.id,
                workspace_id: connection.workspace_id,
                client_id: request.client_id.clone(),
                redirect_uri: request.redirect_uri.clone(),
                code_challenge: request.code_challenge.clone(),
                resource: request.resource.clone(),
                scopes: request.scopes.clone(),
                expires_at: Utc::now() + AUTHORIZATION_CODE_TTL,
            })
            .await?;
        redirect_with_code(&request.redirect_uri, &code, &request.state)
    }

    async fn consent_context(
        &self,
        request: OAuthAuthorizationRequest,
    ) -> Result<OAuthConsentContext, OAuthError> {
        let user_id = request.user_id.ok_or(OAuthError::InvalidGrant)?;
        let client = self
            .repository
            .get_oauth_client(&request.client_id)
            .await?
            .ok_or(OAuthError::InvalidClient)?;
        self.repository
            .get_workspace_with_role_for_user(user_id)
            .await?
            .ok_or(OAuthError::InvalidGrant)?;
        Ok(OAuthConsentContext {
            request_id: request.id,
            client_name: client.client_name,
        })
    }

    async fn issue_token_for_code(
        &self,
        code: OAuthAuthorizationCode,
    ) -> Result<IssuedPasetoToken, OAuthError> {
        let Some(context) = self
            .agent_connections
            .authorize_mcp_connection(AuthorizeMcpConnectionPayload {
                connection_id: code.agent_connection_id,
                workspace_id: code.workspace_id,
                auth0_subject: code.auth0_subject.clone(),
                auth0_client_id: code.client_id.clone(),
                resource: code.resource.clone(),
                permissions: code.scopes.clone(),
            })
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
