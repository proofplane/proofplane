use std::sync::Arc;

use axum::{
    extract::{Request, State},
    http::{header, HeaderValue, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use tracing::Span;

use crate::authentication::{opaque_token, ApiTokenAuthenticator};

pub(crate) const AUTHENTICATE_CHALLENGE: &str = "Bearer realm=\"proofplane-mcp\"";

pub(crate) async fn authenticate_request(
    State(authenticator): State<Arc<ApiTokenAuthenticator>>,
    mut request: Request,
    next: Next,
) -> Response {
    let Some(raw_token) = bearer_token(&request) else {
        return unauthorized();
    };

    let context = match authenticator.authenticate(raw_token).await {
        Ok(Some(context)) => context,
        Ok(None) => return unauthorized(),
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
    if token.trim() != token || opaque_token::parse(token).is_err() {
        return None;
    }
    Some(token)
}

fn unauthorized() -> Response {
    let mut response = (StatusCode::UNAUTHORIZED, "authentication required").into_response();
    response.headers_mut().insert(
        header::WWW_AUTHENTICATE,
        HeaderValue::from_static(AUTHENTICATE_CHALLENGE),
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

            assert_eq!(bearer_token(&request), None);
        }
    }
}
