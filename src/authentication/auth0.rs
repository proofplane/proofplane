use jwtk::jwk::RemoteJwksVerifier;
use jwtk::Claims;
use serde::{Deserialize, Serialize};
use std::time::Duration;

use crate::config::Auth0Config;

const JWKS_CACHE_DURATION: Duration = Duration::from_secs(3600);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedClaims {
    pub sub: String,
    pub email: Option<String>,
    pub name: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum VerifyError {
    #[error("token is malformed or its signature is invalid")]
    InvalidToken,
    #[error("token is expired")]
    Expired,
    #[error("token is not yet valid")]
    NotYetValid,
    #[error("token issuer is missing")]
    MissingIssuer,
    #[error("token issuer does not match the configured issuer")]
    IssuerMismatch,
    #[error("token audience does not match the configured audience")]
    AudienceMismatch,
    #[error("token subject is missing")]
    MissingSubject,
    #[error("the JWKS endpoint is unavailable")]
    JwksUnavailable,
}

impl VerifyError {
    pub fn is_token_rejection(&self) -> bool {
        !matches!(self, VerifyError::JwksUnavailable)
    }
}

#[async_trait::async_trait]
pub trait TokenVerifier: Send + Sync {
    async fn verify(&self, token: &str) -> Result<VerifiedClaims, VerifyError>;
}

#[derive(Default, Serialize, Deserialize)]
struct Auth0ExtraClaims {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    email: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    name: Option<String>,
}

enum Backend {
    Remote(RemoteJwksVerifier),
    #[cfg(test)]
    Local(jwtk::jwk::JwkSetVerifier),
}

pub struct Auth0TokenVerifier {
    backend: Backend,
    issuer: String,
    audience: String,
}

impl Auth0TokenVerifier {
    pub fn new(config: &Auth0Config) -> Self {
        let backend = Backend::Remote(
            RemoteJwksVerifier::builder(config.jwks_url.to_string())
                .with_cache_duration(JWKS_CACHE_DURATION)
                .build(),
        );

        Self {
            backend,
            issuer: config.issuer.to_string(),
            audience: config.audience.clone(),
        }
    }
}

#[async_trait::async_trait]
impl TokenVerifier for Auth0TokenVerifier {
    async fn verify(&self, token: &str) -> Result<VerifiedClaims, VerifyError> {
        let verified = match &self.backend {
            Backend::Remote(verifier) => verifier.verify::<Auth0ExtraClaims>(token).await,
            #[cfg(test)]
            Backend::Local(verifier) => verifier.verify::<Auth0ExtraClaims>(token),
        }
        .map_err(classify_jwtk_error)?;

        validate_claims(verified.claims(), &self.issuer, &self.audience)
    }
}

fn classify_jwtk_error(error: jwtk::Error) -> VerifyError {
    match error {
        jwtk::Error::Expired => VerifyError::Expired,
        jwtk::Error::Before => VerifyError::NotYetValid,
        jwtk::Error::Reqwest(_) => VerifyError::JwksUnavailable,
        _ => VerifyError::InvalidToken,
    }
}

fn validate_claims(
    claims: &Claims<Auth0ExtraClaims>,
    issuer: &str,
    audience: &str,
) -> Result<VerifiedClaims, VerifyError> {
    let token_issuer = claims.iss.as_deref().ok_or(VerifyError::MissingIssuer)?;
    if token_issuer != issuer {
        return Err(VerifyError::IssuerMismatch);
    }

    if !claims.aud.iter().any(|entry| entry == audience) {
        return Err(VerifyError::AudienceMismatch);
    }

    let sub = claims
        .sub
        .as_deref()
        .filter(|sub| !sub.is_empty())
        .ok_or(VerifyError::MissingSubject)?
        .to_owned();

    Ok(VerifiedClaims {
        sub,
        email: claims.extra.email.clone(),
        name: claims.extra.name.clone(),
    })
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use jwtk::hmac::{HmacAlgorithm, HmacKey};
    use jwtk::jwk::{JwkSet, WithKid};
    use jwtk::rsa::{RsaAlgorithm, RsaPrivateKey};
    use jwtk::{sign, HeaderAndClaims, PublicKeyToJwk, SigningKey};

    use super::{Auth0ExtraClaims, Auth0TokenVerifier, Backend, TokenVerifier, VerifyError};

    const ISSUER: &str = "https://proofplane.us.auth0.com/";
    const AUDIENCE: &str = "https://api.proofplane.com";

    struct Fixture {
        signing_key: WithKid<RsaPrivateKey>,
        verifier: Auth0TokenVerifier,
    }

    fn fixture() -> Fixture {
        let private_key =
            RsaPrivateKey::generate(2048, RsaAlgorithm::RS256).expect("rsa key generates");
        let signing_key =
            WithKid::new_with_thumbprint_id(private_key).expect("signing key derives kid");
        let jwk = signing_key.public_key_to_jwk().expect("public jwk derives");
        let mut verifier = JwkSet { keys: vec![jwk] }.verifier();
        verifier.set_require_kid(false);

        Fixture {
            signing_key,
            verifier: Auth0TokenVerifier {
                backend: Backend::Local(verifier),
                issuer: ISSUER.to_owned(),
                audience: AUDIENCE.to_owned(),
            },
        }
    }

    fn claims(iss: &str, aud: &str, sub: &str) -> HeaderAndClaims<Auth0ExtraClaims> {
        let mut claims = HeaderAndClaims::with_claims(Auth0ExtraClaims::default());
        claims
            .set_iss(iss)
            .set_sub(sub)
            .add_aud(aud)
            .set_exp_from_now(Duration::from_secs(3600));
        claims
    }

    fn sign_with(fixture: &Fixture, mut claims: HeaderAndClaims<Auth0ExtraClaims>) -> String {
        sign(&mut claims, &fixture.signing_key as &dyn SigningKey).expect("token signs")
    }

    #[tokio::test]
    async fn valid_token_returns_claims() {
        let fixture = fixture();
        let mut claims = claims(ISSUER, AUDIENCE, "auth0|abc123");
        claims.claims_mut().extra = Auth0ExtraClaims {
            email: Some("human@example.com".to_owned()),
            name: Some("Human Example".to_owned()),
        };
        let token = sign_with(&fixture, claims);

        let verified = fixture
            .verifier
            .verify(&token)
            .await
            .expect("token verifies");

        assert_eq!(verified.sub, "auth0|abc123");
        assert_eq!(verified.email.as_deref(), Some("human@example.com"));
        assert_eq!(verified.name.as_deref(), Some("Human Example"));
    }

    #[tokio::test]
    async fn token_without_profile_claims_still_verifies() {
        let fixture = fixture();
        let token = sign_with(&fixture, claims(ISSUER, AUDIENCE, "auth0|no-profile"));

        let verified = fixture
            .verifier
            .verify(&token)
            .await
            .expect("token verifies");

        assert_eq!(verified.sub, "auth0|no-profile");
        assert_eq!(verified.email, None);
        assert_eq!(verified.name, None);
    }

    #[tokio::test]
    async fn expired_token_is_rejected() {
        let fixture = fixture();
        let mut claims = claims(ISSUER, AUDIENCE, "auth0|abc123");
        let expired = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .checked_sub(Duration::from_secs(3600))
            .unwrap();
        claims.claims_mut().exp = Some(expired);
        let token = sign_with(&fixture, claims);

        let error = fixture.verifier.verify(&token).await.expect_err("expired");
        assert!(matches!(error, VerifyError::Expired));
    }

    #[tokio::test]
    async fn wrong_audience_is_rejected() {
        let fixture = fixture();
        let token = sign_with(
            &fixture,
            claims(ISSUER, "https://wrong.audience", "auth0|abc"),
        );

        let error = fixture.verifier.verify(&token).await.expect_err("bad aud");
        assert!(matches!(error, VerifyError::AudienceMismatch));
    }

    #[tokio::test]
    async fn wrong_issuer_is_rejected() {
        let fixture = fixture();
        let token = sign_with(
            &fixture,
            claims("https://wrong.issuer/", AUDIENCE, "auth0|abc"),
        );

        let error = fixture.verifier.verify(&token).await.expect_err("bad iss");
        assert!(matches!(error, VerifyError::IssuerMismatch));
    }

    #[tokio::test]
    async fn tampered_signature_is_rejected() {
        let fixture = fixture();
        let token = sign_with(&fixture, claims(ISSUER, AUDIENCE, "auth0|abc"));
        let mut tampered = token.clone();
        let last = tampered.pop().unwrap();
        tampered.push(if last == 'A' { 'B' } else { 'A' });

        let error = fixture
            .verifier
            .verify(&tampered)
            .await
            .expect_err("tampered");
        assert!(matches!(error, VerifyError::InvalidToken));
    }

    #[tokio::test]
    async fn non_rs256_token_is_rejected() {
        let fixture = fixture();
        let hmac_key = HmacKey::generate(HmacAlgorithm::HS256).expect("hmac key generates");
        let hmac_with_kid = WithKid::new(fixture.signing_key.kid().to_owned(), hmac_key);
        let mut claims = claims(ISSUER, AUDIENCE, "auth0|abc");
        let token =
            sign(&mut claims, &hmac_with_kid as &dyn SigningKey).expect("hs256 token signs");

        let error = fixture
            .verifier
            .verify(&token)
            .await
            .expect_err("non rs256");
        assert!(matches!(error, VerifyError::InvalidToken));
    }
}
