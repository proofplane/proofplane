use std::sync::Arc;

use axum::{
    extract::{Request, State},
    http::{header, HeaderValue, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use tracing::Span;

use crate::authentication::{opaque_token, ApiTokenAuthenticator, OAuthAccessAuthenticator};
use url::Url;

#[derive(Clone)]
pub struct McpAuthenticator {
    pub api_tokens: Arc<ApiTokenAuthenticator>,
    pub oauth: OAuthAccessAuthenticator,
    pub metadata_url: Url,
}

pub(crate) async fn authenticate_request(
    State(authenticator): State<McpAuthenticator>,
    mut request: Request,
    next: Next,
) -> Response {
    let Some(raw_token) = bearer_token(&request) else {
        return unauthorized(&authenticator.metadata_url);
    };

    let authenticated = if opaque_token::parse(raw_token).is_ok() {
        authenticator.api_tokens.authenticate(raw_token).await
    } else {
        authenticator.oauth.authenticate(raw_token).await
    };
    let context = match authenticated {
        Ok(Some(context)) => context,
        Ok(None) => return unauthorized(&authenticator.metadata_url),
        Err(error) => {
            tracing::error!(%error, "MCP API token authentication failed");
            return (StatusCode::INTERNAL_SERVER_ERROR, "internal server error").into_response();
        }
    };

    Span::current().record("user_id", context.user_id.to_string());
    Span::current().record("api_token_id", context.api_token_id.to_string());
    request.extensions_mut().insert(context);
    next.run(request).await
}

fn bearer_token(request: &Request) -> Option<&str> {
    let value = request
        .headers()
        .get(header::AUTHORIZATION)?
        .to_str()
        .ok()?;
    let token = value.strip_prefix("Bearer ")?;
    if token.trim() != token || token.is_empty() {
        return None;
    }
    Some(token)
}

fn unauthorized(metadata_url: &Url) -> Response {
    let mut response = (StatusCode::UNAUTHORIZED, "authentication required").into_response();
    response.headers_mut().insert(
        header::WWW_AUTHENTICATE,
        HeaderValue::from_str(&format!(
            "Bearer realm=\"proofplane-mcp\", resource_metadata=\"{metadata_url}\""
        ))
        .expect("validated metadata URL is a header value"),
    );
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::Request};
    use secrecy::ExposeSecret;

    #[test]
    fn bearer_token_accepts_valid_api_tokens() {
        let raw_token = opaque_token::generate_opaque_token()
            .expect("token is generated")
            .raw_token
            .expose_secret()
            .to_owned();
        let request = Request::builder()
            .header(header::AUTHORIZATION, format!("Bearer {raw_token}"))
            .body(Body::empty())
            .expect("request builds");

        assert_eq!(bearer_token(&request), Some(raw_token.as_str()));
    }

    #[test]
    fn bearer_token_rejects_missing_wrong_or_malformed_credentials() {
        for authorization in [None, Some("Basic abc"), Some("Bearer not-a-token")] {
            let mut builder = Request::builder();
            if let Some(authorization) = authorization {
                builder = builder.header(header::AUTHORIZATION, authorization);
            }
            let request = builder.body(Body::empty()).expect("request builds");

            assert_eq!(
                bearer_token(&request),
                authorization.and_then(|value| value.strip_prefix("Bearer "))
            );
        }
    }
}
