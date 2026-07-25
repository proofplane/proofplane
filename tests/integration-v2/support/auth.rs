use async_trait::async_trait;
use http::StatusCode;
use proofplane::authentication::auth0::{TokenVerifier, VerifiedClaims, VerifyError};
use serde_json::Value;

/// Test double for the Auth0 token verifier. The bearer token IS the `auth0_sub`,
/// except for the reserved values below. Tokens prefixed with `noprofile:` omit the
/// `email`/`name` claims so JIT provisioning can be exercised without a profile.
pub struct FakeTokenVerifier;

#[async_trait]
impl TokenVerifier for FakeTokenVerifier {
    type Claims = VerifiedClaims;

    async fn verify(&self, token: &str) -> Result<VerifiedClaims, VerifyError> {
        if token.is_empty() || token == "invalid" {
            return Err(VerifyError::InvalidToken);
        }

        if let Some(sub) = token.strip_prefix("noprofile:") {
            return Ok(VerifiedClaims {
                sub: sub.to_owned(),
                email: None,
                name: None,
            });
        }

        Ok(VerifiedClaims {
            sub: token.to_owned(),
            email: Some(format!("{token}@example.com")),
            name: Some("Integration Human".to_owned()),
        })
    }
}

#[track_caller]
pub fn assert_unauthorized(body: &Value, status: StatusCode) {
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["error"]["code"], "unauthorized");
    assert_eq!(body["error"]["details"], serde_json::json!([]));
}
