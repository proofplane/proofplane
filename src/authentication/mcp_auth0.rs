use std::{
    collections::HashSet,
    time::{SystemTime, UNIX_EPOCH},
};

use async_trait::async_trait;
use jwtk::Claims;
use serde_json::{Map, Value};

use crate::{authentication::jwks::JwksVerifier, config::Auth0Config, domain::WorkspacePermission};

const ISSUED_AT_LEEWAY: std::time::Duration = std::time::Duration::from_secs(30);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedMcpClaims {
    pub subject: String,
    pub client_id: String,
    pub scopes: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum McpVerifyError {
    #[error("token is malformed or its signature is invalid")]
    InvalidToken,
    #[error("token is expired")]
    Expired,
    #[error("token is not yet valid")]
    NotYetValid,
    #[error("token issuer does not match")]
    IssuerMismatch,
    #[error("token audience does not match")]
    AudienceMismatch,
    #[error("token subject is missing")]
    MissingSubject,
    #[error("client-credentials identities are not accepted")]
    MachineIdentity,
    #[error("token authorized client is missing or not allowed")]
    InvalidClient,
    #[error("token lifetime claims are missing or invalid")]
    InvalidLifetime,
    #[error("token scopes are missing or unsupported")]
    InvalidScopes,
    #[error("the JWKS endpoint is unavailable")]
    JwksUnavailable,
}

impl McpVerifyError {
    pub fn is_token_rejection(&self) -> bool {
        !matches!(self, Self::JwksUnavailable)
    }
}

#[async_trait]
pub trait McpTokenVerifier: Send + Sync {
    async fn verify(&self, token: &str) -> Result<VerifiedMcpClaims, McpVerifyError>;
}

pub struct Auth0McpTokenVerifier {
    verifier: JwksVerifier,
    issuer: String,
    audience: String,
    allowed_client_ids: HashSet<String>,
}

impl Auth0McpTokenVerifier {
    pub fn new(config: &Auth0Config) -> Self {
        Self {
            verifier: JwksVerifier::remote(config.jwks_url.to_string()),
            issuer: config.issuer.to_string(),
            audience: config.mcp.resource.to_string(),
            allowed_client_ids: config.mcp.allowed_client_ids.iter().cloned().collect(),
        }
    }

    #[cfg(test)]
    fn local(verifier: jwtk::jwk::JwkSetVerifier) -> Self {
        Self {
            verifier: JwksVerifier::local(verifier),
            issuer: "https://tenant.auth0.com/".to_owned(),
            audience: "https://mcp.proofplane.com/mcp".to_owned(),
            allowed_client_ids: HashSet::from(["client-123".to_owned()]),
        }
    }
}

#[async_trait]
impl McpTokenVerifier for Auth0McpTokenVerifier {
    async fn verify(&self, token: &str) -> Result<VerifiedMcpClaims, McpVerifyError> {
        let verified = self
            .verifier
            .verify::<Map<String, Value>>(token)
            .await
            .map_err(classify_jwt_error)?;
        if verified.header().alg.as_ref() != "RS256" {
            return Err(McpVerifyError::InvalidToken);
        }

        validate_claims(
            verified.claims(),
            &self.issuer,
            &self.audience,
            &self.allowed_client_ids,
        )
    }
}

fn classify_jwt_error(error: jwtk::Error) -> McpVerifyError {
    match error {
        jwtk::Error::Expired => McpVerifyError::Expired,
        jwtk::Error::Before => McpVerifyError::NotYetValid,
        jwtk::Error::Reqwest(_) => McpVerifyError::JwksUnavailable,
        _ => McpVerifyError::InvalidToken,
    }
}

fn validate_claims(
    claims: &Claims<Map<String, Value>>,
    issuer: &str,
    audience: &str,
    allowed_client_ids: &HashSet<String>,
) -> Result<VerifiedMcpClaims, McpVerifyError> {
    if claims.iss.as_deref() != Some(issuer) {
        return Err(McpVerifyError::IssuerMismatch);
    }
    if !claims.aud.iter().any(|entry| entry == audience) {
        return Err(McpVerifyError::AudienceMismatch);
    }

    let subject = claims
        .sub
        .as_deref()
        .filter(|value| !value.is_empty())
        .ok_or(McpVerifyError::MissingSubject)?
        .to_owned();
    if subject.ends_with("@clients")
        || string_claim(&claims.extra, "gty") == Some("client-credentials")
    {
        return Err(McpVerifyError::MachineIdentity);
    }

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| McpVerifyError::InvalidLifetime)?;
    let issued_at = claims.iat.ok_or(McpVerifyError::InvalidLifetime)?;
    let expires_at = claims.exp.ok_or(McpVerifyError::InvalidLifetime)?;
    if issued_at > now + ISSUED_AT_LEEWAY || expires_at <= issued_at {
        return Err(McpVerifyError::InvalidLifetime);
    }

    let client_id = string_claim(&claims.extra, "azp").ok_or(McpVerifyError::InvalidClient)?;
    if !allowed_client_ids.contains(client_id) {
        return Err(McpVerifyError::InvalidClient);
    }

    let raw_scope = string_claim(&claims.extra, "scope").ok_or(McpVerifyError::InvalidScopes)?;
    let scopes = raw_scope
        .split_ascii_whitespace()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if scopes.is_empty()
        || scopes.iter().any(|scope| {
            !WorkspacePermission::ALL
                .iter()
                .any(|permission| permission.as_str() == scope)
        })
    {
        return Err(McpVerifyError::InvalidScopes);
    }

    Ok(VerifiedMcpClaims {
        subject,
        client_id: client_id.to_owned(),
        scopes,
    })
}

fn string_claim<'a>(claims: &'a Map<String, Value>, name: &str) -> Option<&'a str> {
    claims.get(name)?.as_str().filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use jwtk::{
        hmac::{HmacAlgorithm, HmacKey},
        jwk::{JwkSet, WithKid},
        rsa::{RsaAlgorithm, RsaPrivateKey},
        sign, HeaderAndClaims, PublicKeyToJwk,
    };
    use std::time::Duration;

    const ISSUER: &str = "https://tenant.auth0.com/";
    const AUDIENCE: &str = "https://mcp.proofplane.com/mcp";
    const CLIENT: &str = "client-123";

    fn claims() -> HeaderAndClaims<Map<String, Value>> {
        let mut claims = HeaderAndClaims::with_claims(Map::new());
        claims
            .set_iss(ISSUER)
            .set_sub("auth0|user")
            .add_aud(AUDIENCE)
            .set_iat_now()
            .set_exp_from_now(Duration::from_secs(3600))
            .insert("azp", CLIENT)
            .insert("scope", "read_controls write_controls");
        claims
    }

    fn validate(
        claims: &HeaderAndClaims<Map<String, Value>>,
    ) -> Result<VerifiedMcpClaims, McpVerifyError> {
        validate_claims(
            claims.claims(),
            ISSUER,
            AUDIENCE,
            &HashSet::from([CLIENT.to_owned()]),
        )
    }

    fn verifier_and_signing_key() -> (Auth0McpTokenVerifier, WithKid<RsaPrivateKey>) {
        let private_key = RsaPrivateKey::generate(2048, RsaAlgorithm::RS256).unwrap();
        let signing_key = WithKid::new_with_thumbprint_id(private_key).unwrap();
        let jwk = signing_key.public_key_to_jwk().unwrap();
        let mut verifier = JwkSet { keys: vec![jwk] }.verifier();
        verifier.set_require_kid(false);
        (Auth0McpTokenVerifier::local(verifier), signing_key)
    }

    #[test]
    fn valid_claims_are_projected() {
        let verified = validate(&claims()).expect("claims verify");
        assert_eq!(verified.subject, "auth0|user");
        assert_eq!(verified.client_id, CLIENT);
        assert_eq!(verified.scopes, ["read_controls", "write_controls"]);
    }

    #[test]
    fn rejects_wrong_issuer_audience_and_subject() {
        let mut wrong_issuer = claims();
        wrong_issuer.set_iss("https://other.example/");
        assert!(matches!(
            validate(&wrong_issuer),
            Err(McpVerifyError::IssuerMismatch)
        ));

        let mut wrong_audience = claims();
        wrong_audience.claims_mut().aud = Default::default();
        wrong_audience.add_aud("https://other.example/mcp");
        assert!(matches!(
            validate(&wrong_audience),
            Err(McpVerifyError::AudienceMismatch)
        ));

        let mut missing_subject = claims();
        missing_subject.claims_mut().sub = None;
        assert!(matches!(
            validate(&missing_subject),
            Err(McpVerifyError::MissingSubject)
        ));
    }

    #[test]
    fn rejects_missing_invalid_or_future_lifetime() {
        let mut missing_iat = claims();
        missing_iat.claims_mut().iat = None;
        assert!(matches!(
            validate(&missing_iat),
            Err(McpVerifyError::InvalidLifetime)
        ));

        let mut future_iat = claims();
        future_iat.claims_mut().iat =
            Some(SystemTime::now().duration_since(UNIX_EPOCH).unwrap() + Duration::from_secs(60));
        assert!(matches!(
            validate(&future_iat),
            Err(McpVerifyError::InvalidLifetime)
        ));

        let mut missing_exp = claims();
        missing_exp.claims_mut().exp = None;
        assert!(matches!(
            validate(&missing_exp),
            Err(McpVerifyError::InvalidLifetime)
        ));

        let mut invalid_order = claims();
        invalid_order.claims_mut().exp = invalid_order.claims().iat;
        assert!(matches!(
            validate(&invalid_order),
            Err(McpVerifyError::InvalidLifetime)
        ));
    }

    #[test]
    fn rejects_missing_or_unallowlisted_clients() {
        let mut missing_client = claims();
        missing_client.claims_mut().extra.remove("azp");
        assert!(matches!(
            validate(&missing_client),
            Err(McpVerifyError::InvalidClient)
        ));

        let mut wrong_client = claims();
        wrong_client.insert("azp", "unregistered");
        assert!(matches!(
            validate(&wrong_client),
            Err(McpVerifyError::InvalidClient)
        ));
    }

    #[test]
    fn rejects_missing_unknown_and_offline_scopes() {
        for scope in ["", "offline_access", "read_controls unknown_scope"] {
            let mut bad_scope = claims();
            bad_scope.insert("scope", scope);
            assert!(matches!(
                validate(&bad_scope),
                Err(McpVerifyError::InvalidScopes)
            ));
        }

        let mut missing_scope = claims();
        missing_scope.claims_mut().extra.remove("scope");
        assert!(matches!(
            validate(&missing_scope),
            Err(McpVerifyError::InvalidScopes)
        ));
    }

    #[test]
    fn rejects_client_credentials_identities() {
        let mut machine_subject = claims();
        machine_subject.set_sub("client-123@clients");
        assert!(matches!(
            validate(&machine_subject),
            Err(McpVerifyError::MachineIdentity)
        ));

        let mut machine_grant = claims();
        machine_grant.insert("gty", "client-credentials");
        assert!(matches!(
            validate(&machine_grant),
            Err(McpVerifyError::MachineIdentity)
        ));
    }

    #[tokio::test]
    async fn verifies_rs256_signature_and_rejects_tampering_or_other_algorithms() {
        let (verifier, signing_key) = verifier_and_signing_key();
        let mut valid_claims = claims();
        let valid = sign(&mut valid_claims, &signing_key).unwrap();
        verifier.verify(&valid).await.expect("RS256 token verifies");

        let (head, signature) = valid.rsplit_once('.').unwrap();
        let mut chars = signature.chars();
        let first = chars.next().unwrap();
        let tampered = format!(
            "{head}.{}{}",
            if first == 'A' { 'B' } else { 'A' },
            chars.as_str()
        );
        assert!(matches!(
            verifier.verify(&tampered).await,
            Err(McpVerifyError::InvalidToken)
        ));

        let hmac_key =
            HmacKey::from_bytes(b"at-least-32-byte-test-secret-value", HmacAlgorithm::HS256);
        let hmac_with_kid = WithKid::new(signing_key.kid().to_owned(), hmac_key);
        let mut hmac_claims = claims();
        let hmac = sign(&mut hmac_claims, &hmac_with_kid).unwrap();
        assert!(matches!(
            verifier.verify(&hmac).await,
            Err(McpVerifyError::InvalidToken)
        ));
    }

    #[tokio::test]
    async fn rejects_expired_tokens() {
        let (verifier, signing_key) = verifier_and_signing_key();
        let mut expired_claims = claims();
        expired_claims.claims_mut().exp =
            Some(SystemTime::now().duration_since(UNIX_EPOCH).unwrap() - Duration::from_secs(1));
        let expired = sign(&mut expired_claims, &signing_key).unwrap();

        assert!(matches!(
            verifier.verify(&expired).await,
            Err(McpVerifyError::Expired)
        ));
    }

    #[tokio::test]
    async fn classifies_unavailable_jwks_as_dependency_failure() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);
        let (mut verifier, signing_key) = verifier_and_signing_key();
        verifier.verifier = JwksVerifier::remote(format!("http://{address}/jwks"));
        let mut valid_claims = claims();
        let token = sign(&mut valid_claims, &signing_key).unwrap();

        assert!(matches!(
            verifier.verify(&token).await,
            Err(McpVerifyError::JwksUnavailable)
        ));
    }
}
