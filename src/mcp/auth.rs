use std::sync::Arc;

use axum::{
    extract::{Request, State},
    http::{header, HeaderValue, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use tracing::Span;

use crate::authentication::{
    auth0::{TokenVerifier, VerifiedMcpClaims},
    ApiTokenAuthenticator, ApiTokenContext,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum McpPrincipal {
    ApiToken(ApiTokenContext),
    Auth0(VerifiedMcpClaims),
}

pub(crate) struct AuthenticationState<V> {
    pub api_tokens: Arc<ApiTokenAuthenticator>,
    pub auth0: Arc<V>,
    pub challenge: HeaderValue,
}

impl<V> Clone for AuthenticationState<V> {
    fn clone(&self) -> Self {
        Self {
            api_tokens: self.api_tokens.clone(),
            auth0: self.auth0.clone(),
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

    let principal = if raw_token.starts_with("ppat_") {
        match state.api_tokens.authenticate(raw_token).await {
            Ok(Some(context)) => McpPrincipal::ApiToken(context),
            Ok(None) => return unauthorized(&state.challenge),
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
                return unauthorized(&state.challenge);
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
