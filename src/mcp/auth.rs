use axum::{
    extract::{Request, State},
    http::{header, HeaderValue, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use tracing::Span;

use crate::{
    application::{
        commands::agent_connections::{
            AgentConnectionCommandError, AuthorizeAgentConnection, AuthorizeAgentConnectionHandler,
        },
        ExecutionMetadata,
    },
    authentication::{
        auth0::{TokenVerifier, VerifiedMcpClaims},
        AgentConnectionContext,
    },
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum McpPrincipal {
    AgentConnection(AgentConnectionContext),
}

pub(crate) struct AuthenticationState<V> {
    pub auth0: std::sync::Arc<V>,
    pub authorize_agent_connection: AuthorizeAgentConnectionHandler,
    pub resource: String,
    pub challenge: HeaderValue,
}

impl<V> Clone for AuthenticationState<V> {
    fn clone(&self) -> Self {
        Self {
            auth0: self.auth0.clone(),
            authorize_agent_connection: self.authorize_agent_connection.clone(),
            resource: self.resource.clone(),
            challenge: self.challenge.clone(),
        }
    }
}

pub(crate) async fn authenticate_request<V: TokenVerifier<Claims = VerifiedMcpClaims>>(
    State(state): State<AuthenticationState<V>>,
    mut request: Request,
    next: Next,
) -> Response {
    let Some(raw_token) = bearer_token(&request) else {
        return unauthorized(&state.challenge);
    };

    let principal = match state.auth0.verify(raw_token).await {
        Ok(claims) => match agent_connection_principal(&state, claims).await {
            Ok(principal) => principal,
            Err(AgentConnectionAuthError::Rejected) => return unauthorized(&state.challenge),
            Err(AgentConnectionAuthError::Unavailable(error)) => {
                tracing::error!(%error, "MCP agent connection authorization failed");
                return (StatusCode::INTERNAL_SERVER_ERROR, "internal server error")
                    .into_response();
            }
        },
        Err(error) if error.is_token_rejection() => {
            return unauthorized(&state.challenge);
        }
        Err(error) => {
            tracing::error!(%error, "MCP OAuth token verification failed");
            return (StatusCode::INTERNAL_SERVER_ERROR, "internal server error").into_response();
        }
    };

    match &principal {
        McpPrincipal::AgentConnection(context) => {
            Span::current().record("user_id", context.user_id.to_string());
        }
    }
    request.extensions_mut().insert(principal);
    next.run(request).await
}

enum AgentConnectionAuthError {
    Rejected,
    Unavailable(AgentConnectionCommandError),
}

async fn agent_connection_principal<V: TokenVerifier<Claims = VerifiedMcpClaims>>(
    state: &AuthenticationState<V>,
    claims: VerifiedMcpClaims,
) -> Result<McpPrincipal, AgentConnectionAuthError> {
    match (claims.connection_id, claims.workspace_id) {
        (Some(connection_id), Some(workspace_id)) => {
            let context = state
                .authorize_agent_connection
                .handle(
                    AuthorizeAgentConnection {
                        connection_id,
                        workspace_id,
                        auth0_subject: claims.subject.clone(),
                        auth0_client_id: claims.client_id.clone(),
                        resource: state.resource.clone(),
                        permissions: claims.scopes.clone(),
                    },
                    ExecutionMetadata::background(),
                )
                .await
                .map_err(AgentConnectionAuthError::Unavailable)?
                .ok_or(AgentConnectionAuthError::Rejected)?;
            Ok(McpPrincipal::AgentConnection(context))
        }
        _ => Err(AgentConnectionAuthError::Rejected),
    }
}

fn bearer_token(request: &Request) -> Option<&str> {
    let value = request
        .headers()
        .get(header::AUTHORIZATION)?
        .to_str()
        .ok()?;
    let token = value.strip_prefix("Bearer ")?;
    if token.is_empty() || token.trim() != token {
        return None;
    }
    Some(token)
}

fn unauthorized(challenge: &HeaderValue) -> Response {
    let mut response = (StatusCode::UNAUTHORIZED, "authentication required").into_response();
    response
        .headers_mut()
        .insert(header::WWW_AUTHENTICATE, challenge.clone());
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::Request};
    #[test]
    fn bearer_token_accepts_jwts() {
        let raw_token = "eyJhbGciOiJSUzI1NiJ9.payload.signature";
        let request = Request::builder()
            .header(header::AUTHORIZATION, format!("Bearer {raw_token}"))
            .body(Body::empty())
            .expect("request builds");

        assert_eq!(bearer_token(&request), Some(raw_token));
    }

    #[test]
    fn bearer_token_rejects_missing_wrong_or_malformed_credentials() {
        for authorization in [
            None,
            Some("Basic abc"),
            Some("Bearer "),
            Some("Bearer padded "),
        ] {
            let mut builder = Request::builder();
            if let Some(authorization) = authorization {
                builder = builder.header(header::AUTHORIZATION, authorization);
            }
            let request = builder.body(Body::empty()).expect("request builds");

            assert_eq!(bearer_token(&request), None);
        }
    }
}
