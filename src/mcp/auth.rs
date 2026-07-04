use std::sync::Arc;

use axum::{
    extract::{Request, State},
    http::{header, HeaderValue, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use tracing::Span;

use crate::{
    authentication::{
        mcp_auth0::{McpTokenVerifier, VerifiedMcpClaims},
        ApiTokenAuthenticator, ApiTokenContext,
    },
    config::Auth0McpConfig,
    domain::WorkspacePermission,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum McpPrincipal {
    ApiToken(ApiTokenContext),
    Auth0(VerifiedMcpClaims),
}

#[derive(Clone)]
pub(crate) struct AuthenticationState {
    pub api_tokens: Arc<ApiTokenAuthenticator>,
    pub auth0: Arc<dyn McpTokenVerifier>,
    pub auth0_config: Auth0McpConfig,
}

pub(crate) async fn authenticate_request(
    State(state): State<AuthenticationState>,
    mut request: Request,
    next: Next,
) -> Response {
    let Some(raw_token) = bearer_token(&request) else {
        return unauthorized(&state.auth0_config);
    };

    let principal = if raw_token.starts_with("ppat_") {
        match state.api_tokens.authenticate(raw_token).await {
            Ok(Some(context)) => McpPrincipal::ApiToken(context),
            Ok(None) => return unauthorized(&state.auth0_config),
            Err(error) => {
                tracing::error!(%error, "MCP API token authentication failed");
                return (StatusCode::INTERNAL_SERVER_ERROR, "internal server error")
                    .into_response();
            }
        }
    } else {
        match state.auth0.verify(raw_token).await {
            Ok(claims) => McpPrincipal::Auth0(claims),
            Err(error) if error.is_token_rejection() => {
                return unauthorized(&state.auth0_config);
            }
            Err(error) => {
                tracing::error!(%error, "MCP Auth0 token verification failed");
                return (StatusCode::INTERNAL_SERVER_ERROR, "internal server error")
                    .into_response();
            }
        }
    };

    if let McpPrincipal::ApiToken(context) = principal {
        Span::current().record("user_id", context.user_id.to_string());
        Span::current().record("api_token_id", context.api_token_id.to_string());
    }
    request.extensions_mut().insert(principal);
    next.run(request).await
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

fn unauthorized(config: &Auth0McpConfig) -> Response {
    let mut response = (StatusCode::UNAUTHORIZED, "authentication required").into_response();
    let scopes = WorkspacePermission::ALL
        .iter()
        .map(|permission| permission.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    let metadata = config
        .resource
        .join("/.well-known/oauth-protected-resource/mcp")
        .expect("validated resource URL joins");
    let challenge = format!(
        "Bearer realm=\"proofplane-mcp\", resource_metadata=\"{metadata}\", scope=\"{scopes}\""
    );
    response.headers_mut().insert(
        header::WWW_AUTHENTICATE,
        HeaderValue::from_str(&challenge).expect("validated URLs form a header value"),
    );
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::Request};
    #[test]
    fn bearer_token_accepts_api_tokens_and_jwts() {
        for raw_token in ["ppat_example", "eyJhbGciOiJSUzI1NiJ9.payload.signature"] {
            let request = Request::builder()
                .header(header::AUTHORIZATION, format!("Bearer {raw_token}"))
                .body(Body::empty())
                .expect("request builds");

            assert_eq!(bearer_token(&request), Some(raw_token));
        }
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
