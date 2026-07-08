use axum::extract::Request;
use tracing::Span;

use crate::{
    authentication::{
        auth0::{TokenVerifier, VerifiedClaims},
        AuthError, UserAuthenticator, UserContext,
    },
    routes::error::ApiError,
};

pub const AUTHORIZATION_HEADER: &str = "authorization";

pub(in crate::routes) async fn authenticate_user<V: TokenVerifier<Claims = VerifiedClaims>>(
    authenticator: &UserAuthenticator<V>,
    request: &mut Request,
) -> Result<(), ApiError> {
    let token = bearer_token_from_request(request).ok_or(ApiError::Unauthorized)?;
    let user = authenticator.authenticate(&token).await.map_err(|error| {
        if let AuthError::Unauthorized(_) = error {
            return ApiError::Unauthorized;
        }

        tracing::error!(%error, "user authentication failed");
        ApiError::Internal
    })?;

    attach_user_context(request, user);

    Ok(())
}

fn bearer_token_from_request(request: &Request) -> Option<String> {
    header_value(request, AUTHORIZATION_HEADER)?
        .strip_prefix("Bearer ")
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .map(str::to_owned)
}

fn attach_user_context(request: &mut Request, user: UserContext) {
    Span::current().record("user_id", user.user_id.to_string());
    request.extensions_mut().insert(user);
}

fn header_value(request: &Request, header: &'static str) -> Option<String> {
    request
        .headers()
        .get(header)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
}
