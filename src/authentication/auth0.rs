use std::time::{Duration, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use jwtk::Claims;
use serde::{de::DeserializeOwned, Deserialize, Serialize};

use uuid::Uuid;

use crate::{
    authentication::jwks::JwksVerifier,
    config::Auth0Config,
    domain::{
        canonical_permissions, AgentConnectionId, DomainError, WorkspaceId, WorkspacePermission,
    },
};

const ISSUED_AT_LEEWAY: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedClaims {
    pub sub: String,
    pub email: Option<String>,
    pub name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedMcpClaims {
    pub subject: String,
    pub client_id: String,
    pub scopes: Vec<WorkspacePermission>,
    pub connection_id: Option<AgentConnectionId>,
    pub workspace_id: Option<WorkspaceId>,
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
    #[error("client-credentials identities are not accepted")]
    MachineIdentity,
    #[error("token authorized client is missing")]
    InvalidClient,
    #[error("token lifetime claims are missing or invalid")]
    InvalidLifetime,
    #[error("token scopes are missing or unsupported")]
    InvalidScopes,
    #[error("token connection claims are malformed")]
    InvalidConnectionClaims,
    #[error("the JWKS endpoint is unavailable")]
    JwksUnavailable,
}

impl VerifyError {
    pub fn is_token_rejection(&self) -> bool {
        !matches!(self, Self::JwksUnavailable)
    }
}

#[async_trait]
pub trait TokenVerifier: Send + Sync {
    type Claims;

    async fn verify(&self, token: &str) -> Result<Self::Claims, VerifyError>;
}

trait ClaimsPolicy: Send + Sync {
    type ExtraClaims: DeserializeOwned;
    type Output;

    fn validate(
        &self,
        claims: &Claims<Self::ExtraClaims>,
        subject: String,
    ) -> Result<Self::Output, VerifyError>;
}

struct Auth0Verifier<P> {
    verifier: JwksVerifier,
    issuer: String,
    audience: String,
    policy: P,
}

impl<P: ClaimsPolicy> Auth0Verifier<P> {
    async fn verify(&self, token: &str) -> Result<P::Output, VerifyError> {
        let verified = self
            .verifier
            .verify::<P::ExtraClaims>(token)
            .await
            .map_err(classify_jwtk_error)?;

        if verified.header().alg.as_ref() != "RS256" {
            return Err(VerifyError::InvalidToken);
        }

        let claims = verified.claims();
        let token_issuer = claims.iss.as_deref().ok_or(VerifyError::MissingIssuer)?;
        if token_issuer != self.issuer {
            return Err(VerifyError::IssuerMismatch);
        }
        if !claims.aud.iter().any(|entry| entry == &self.audience) {
            return Err(VerifyError::AudienceMismatch);
        }
        let subject = claims
            .sub
            .as_deref()
            .filter(|subject| !subject.is_empty())
            .ok_or(VerifyError::MissingSubject)?
            .to_owned();

        self.policy.validate(claims, subject)
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

#[derive(Default, Serialize, Deserialize)]
struct UserExtraClaims {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    email: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    name: Option<String>,
}

struct UserPolicy;

impl ClaimsPolicy for UserPolicy {
    type ExtraClaims = UserExtraClaims;
    type Output = VerifiedClaims;

    fn validate(
        &self,
        claims: &Claims<Self::ExtraClaims>,
        sub: String,
    ) -> Result<Self::Output, VerifyError> {
        Ok(VerifiedClaims {
            sub,
            email: claims.extra.email.clone(),
            name: claims.extra.name.clone(),
        })
    }
}

#[derive(Default, Serialize, Deserialize)]
struct McpExtraClaims {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    azp: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    scope: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    gty: Option<String>,
    #[serde(
        default,
        rename = "https://proofplane.com/connection_id",
        skip_serializing_if = "Option::is_none"
    )]
    connection_id: Option<String>,
    #[serde(
        default,
        rename = "https://proofplane.com/workspace_id",
        skip_serializing_if = "Option::is_none"
    )]
    workspace_id: Option<String>,
}

struct McpPolicy;

impl ClaimsPolicy for McpPolicy {
    type ExtraClaims = McpExtraClaims;
    type Output = VerifiedMcpClaims;

    fn validate(
        &self,
        claims: &Claims<Self::ExtraClaims>,
        subject: String,
    ) -> Result<Self::Output, VerifyError> {
        if subject.ends_with("@clients")
            || claims.extra.gty.as_deref() == Some("client-credentials")
        {
            return Err(VerifyError::MachineIdentity);
        }

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| VerifyError::InvalidLifetime)?;
        let issued_at = claims.iat.ok_or(VerifyError::InvalidLifetime)?;
        let expires_at = claims.exp.ok_or(VerifyError::InvalidLifetime)?;
        if issued_at > now + ISSUED_AT_LEEWAY || expires_at <= issued_at {
            return Err(VerifyError::InvalidLifetime);
        }

        let client_id = claims
            .extra
            .azp
            .as_deref()
            .filter(|client_id| !client_id.trim().is_empty())
            .ok_or(VerifyError::InvalidClient)?;

        let raw_scope = claims
            .extra
            .scope
            .as_deref()
            .filter(|scope| !scope.is_empty())
            .ok_or(VerifyError::InvalidScopes)?;
        let scopes = raw_scope
            .split_ascii_whitespace()
            .map(str::parse::<WorkspacePermission>)
            .collect::<Result<Vec<_>, DomainError>>()
            .map_err(|_| VerifyError::InvalidScopes)
            .and_then(|scopes| {
                canonical_permissions(scopes).map_err(|_| VerifyError::InvalidScopes)
            })?;
        if scopes.is_empty() {
            return Err(VerifyError::InvalidScopes);
        }

        let connection_id =
            optional_uuid_claim(&claims.extra.connection_id)?.map(AgentConnectionId::from);
        let workspace_id = optional_uuid_claim(&claims.extra.workspace_id)?.map(WorkspaceId::from);

        Ok(VerifiedMcpClaims {
            subject,
            client_id: client_id.to_owned(),
            scopes,
            connection_id,
            workspace_id,
        })
    }
}

fn optional_uuid_claim(value: &Option<String>) -> Result<Option<Uuid>, VerifyError> {
    value
        .as_deref()
        .map(Uuid::parse_str)
        .transpose()
        .map_err(|_| VerifyError::InvalidConnectionClaims)
}

pub struct Auth0TokenVerifier {
    verifier: Auth0Verifier<UserPolicy>,
}

impl Auth0TokenVerifier {
    pub fn new(config: &Auth0Config) -> Self {
        Self {
            verifier: Auth0Verifier {
                verifier: JwksVerifier::remote(config.jwks_url.to_string()),
                issuer: config.issuer.to_string(),
                audience: config.audience.clone(),
                policy: UserPolicy,
            },
        }
    }
}

#[async_trait]
impl TokenVerifier for Auth0TokenVerifier {
    type Claims = VerifiedClaims;

    async fn verify(&self, token: &str) -> Result<VerifiedClaims, VerifyError> {
        self.verifier.verify(token).await
    }
}

pub struct Auth0McpTokenVerifier {
    verifier: Auth0Verifier<McpPolicy>,
}

impl Auth0McpTokenVerifier {
    pub fn new(config: &Auth0Config, audience: impl Into<String>) -> Self {
        Self {
            verifier: Auth0Verifier {
                verifier: JwksVerifier::remote(config.jwks_url.to_string()),
                issuer: config.issuer.to_string(),
                audience: audience.into(),
                policy: McpPolicy,
            },
        }
    }
}

#[async_trait]
impl TokenVerifier for Auth0McpTokenVerifier {
    type Claims = VerifiedMcpClaims;

    async fn verify(&self, token: &str) -> Result<VerifiedMcpClaims, VerifyError> {
        self.verifier.verify(token).await
    }
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

    const ISSUER: &str = "https://tenant.auth0.com/";
    const API_AUDIENCE: &str = "https://api.proofplane.com";
    const MCP_AUDIENCE: &str = "https://mcp.proofplane.com/mcp";
    const CLIENT: &str = "client-123";

    fn signing_material() -> (JwksVerifier, WithKid<RsaPrivateKey>) {
        let private_key =
            RsaPrivateKey::generate(2048, RsaAlgorithm::RS256).expect("rsa key generates");
        let signing_key =
            WithKid::new_with_thumbprint_id(private_key).expect("signing key derives kid");
        let jwk = signing_key.public_key_to_jwk().expect("public jwk derives");
        let mut verifier = JwkSet { keys: vec![jwk] }.verifier();
        verifier.set_require_kid(false);
        (JwksVerifier::local(verifier), signing_key)
    }

    fn user_fixture() -> (Auth0TokenVerifier, WithKid<RsaPrivateKey>) {
        let (jwks, signing_key) = signing_material();
        (
            Auth0TokenVerifier {
                verifier: Auth0Verifier {
                    verifier: jwks,
                    issuer: ISSUER.to_owned(),
                    audience: API_AUDIENCE.to_owned(),
                    policy: UserPolicy,
                },
            },
            signing_key,
        )
    }

    fn mcp_fixture() -> (Auth0McpTokenVerifier, WithKid<RsaPrivateKey>) {
        let (jwks, signing_key) = signing_material();
        (
            Auth0McpTokenVerifier {
                verifier: Auth0Verifier {
                    verifier: jwks,
                    issuer: ISSUER.to_owned(),
                    audience: MCP_AUDIENCE.to_owned(),
                    policy: McpPolicy,
                },
            },
            signing_key,
        )
    }

    fn user_claims(
        issuer: &str,
        audience: &str,
        subject: &str,
    ) -> HeaderAndClaims<UserExtraClaims> {
        let mut claims = HeaderAndClaims::with_claims(UserExtraClaims::default());
        claims
            .set_iss(issuer)
            .set_sub(subject)
            .add_aud(audience)
            .set_exp_from_now(Duration::from_secs(3600));
        claims
    }

    fn mcp_claims() -> HeaderAndClaims<McpExtraClaims> {
        let mut claims = HeaderAndClaims::with_claims(McpExtraClaims {
            azp: Some(CLIENT.to_owned()),
            scope: Some("read_controls write_controls".to_owned()),
            gty: None,
            connection_id: None,
            workspace_id: None,
        });
        claims
            .set_iss(ISSUER)
            .set_sub("auth0|user")
            .add_aud(MCP_AUDIENCE)
            .set_iat_now()
            .set_exp_from_now(Duration::from_secs(3600));
        claims
    }

    fn sign_with<T: Serialize>(
        signing_key: &WithKid<RsaPrivateKey>,
        mut claims: HeaderAndClaims<T>,
    ) -> String {
        sign(&mut claims, signing_key).expect("token signs")
    }

    fn tamper_signature(token: &str) -> String {
        let (head, signature) = token
            .rsplit_once('.')
            .expect("token has a signature segment");
        let mut chars = signature.chars();
        let first = chars.next().expect("signature is non-empty");
        format!(
            "{head}.{}{}",
            if first == 'A' { 'B' } else { 'A' },
            chars.as_str()
        )
    }

    #[tokio::test]
    async fn user_policy_projects_optional_profile_claims() {
        let (verifier, signing_key) = user_fixture();
        let mut with_profile = user_claims(ISSUER, API_AUDIENCE, "auth0|profile");
        with_profile.claims_mut().extra = UserExtraClaims {
            email: Some("human@example.com".to_owned()),
            name: Some("Human Example".to_owned()),
        };
        let verified = verifier
            .verify(&sign_with(&signing_key, with_profile))
            .await
            .expect("token verifies");
        assert_eq!(
            verified,
            VerifiedClaims {
                sub: "auth0|profile".to_owned(),
                email: Some("human@example.com".to_owned()),
                name: Some("Human Example".to_owned()),
            }
        );

        let without_profile = user_claims(ISSUER, API_AUDIENCE, "auth0|no-profile");
        let verified = verifier
            .verify(&sign_with(&signing_key, without_profile))
            .await
            .expect("token verifies");
        assert_eq!(verified.email, None);
        assert_eq!(verified.name, None);
    }

    #[tokio::test]
    async fn both_policies_reject_non_rs256_tokens() {
        let (user_verifier, user_key) = user_fixture();
        let hmac_key = HmacKey::generate(HmacAlgorithm::HS256).expect("hmac key generates");
        let hmac_with_kid = WithKid::new(user_key.kid().to_owned(), hmac_key);
        let mut claims = user_claims(ISSUER, API_AUDIENCE, "auth0|user");
        let token = sign(&mut claims, &hmac_with_kid).expect("HS256 token signs");
        assert!(matches!(
            user_verifier.verify(&token).await,
            Err(VerifyError::InvalidToken)
        ));

        let (mcp_verifier, mcp_key) = mcp_fixture();
        let hmac_key = HmacKey::generate(HmacAlgorithm::HS256).expect("hmac key generates");
        let hmac_with_kid = WithKid::new(mcp_key.kid().to_owned(), hmac_key);
        let mut claims = mcp_claims();
        let token = sign(&mut claims, &hmac_with_kid).expect("HS256 token signs");
        assert!(matches!(
            mcp_verifier.verify(&token).await,
            Err(VerifyError::InvalidToken)
        ));
    }

    #[tokio::test]
    async fn common_claims_and_signature_failures_are_rejected() {
        let (verifier, signing_key) = user_fixture();

        let mut missing_issuer = user_claims(ISSUER, API_AUDIENCE, "auth0|user");
        missing_issuer.claims_mut().iss = None;
        assert!(matches!(
            verifier
                .verify(&sign_with(&signing_key, missing_issuer))
                .await,
            Err(VerifyError::MissingIssuer)
        ));

        for (claims, expected) in [
            (
                user_claims("https://other.example/", API_AUDIENCE, "auth0|user"),
                VerifyError::IssuerMismatch,
            ),
            (
                user_claims(ISSUER, "https://other.example/api", "auth0|user"),
                VerifyError::AudienceMismatch,
            ),
            (
                user_claims(ISSUER, API_AUDIENCE, ""),
                VerifyError::MissingSubject,
            ),
        ] {
            let error = verifier
                .verify(&sign_with(&signing_key, claims))
                .await
                .expect_err("token is rejected");
            assert_eq!(
                std::mem::discriminant(&error),
                std::mem::discriminant(&expected)
            );
        }

        let valid = sign_with(
            &signing_key,
            user_claims(ISSUER, API_AUDIENCE, "auth0|user"),
        );
        assert!(matches!(
            verifier.verify(&tamper_signature(&valid)).await,
            Err(VerifyError::InvalidToken)
        ));
    }

    #[tokio::test]
    async fn jwt_library_lifetime_failures_are_classified() {
        let (verifier, signing_key) = user_fixture();
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();

        let mut expired = user_claims(ISSUER, API_AUDIENCE, "auth0|user");
        expired.claims_mut().exp = Some(now - Duration::from_secs(1));
        assert!(matches!(
            verifier.verify(&sign_with(&signing_key, expired)).await,
            Err(VerifyError::Expired)
        ));

        let mut future = user_claims(ISSUER, API_AUDIENCE, "auth0|user");
        future.claims_mut().nbf = Some(now + Duration::from_secs(3600));
        assert!(matches!(
            verifier.verify(&sign_with(&signing_key, future)).await,
            Err(VerifyError::NotYetValid)
        ));
    }

    #[test]
    fn mcp_policy_projects_valid_claims() {
        let claims = mcp_claims();
        let policy = McpPolicy;
        let verified = policy
            .validate(claims.claims(), "auth0|user".to_owned())
            .expect("claims validate");
        assert_eq!(verified.subject, "auth0|user");
        assert_eq!(verified.client_id, CLIENT);
        assert_eq!(
            verified.scopes,
            [
                WorkspacePermission::ReadControls,
                WorkspacePermission::WriteControls
            ]
        );
    }

    #[test]
    fn mcp_policy_accepts_any_non_blank_dynamic_client_id() {
        let mut claims = mcp_claims();
        claims.claims_mut().extra.azp = Some("dynamic-client-456".to_owned());
        let policy = McpPolicy;

        let verified = policy
            .validate(claims.claims(), "auth0|user".to_owned())
            .expect("claims validate");

        assert_eq!(verified.client_id, "dynamic-client-456");
    }

    #[test]
    fn mcp_policy_rejects_missing_invalid_or_future_lifetime() {
        let policy = McpPolicy;
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();

        let mut cases = Vec::new();
        let mut missing_iat = mcp_claims();
        missing_iat.claims_mut().iat = None;
        cases.push(missing_iat);
        let mut future_iat = mcp_claims();
        future_iat.claims_mut().iat = Some(now + Duration::from_secs(60));
        cases.push(future_iat);
        let mut missing_exp = mcp_claims();
        missing_exp.claims_mut().exp = None;
        cases.push(missing_exp);
        let mut invalid_order = mcp_claims();
        invalid_order.claims_mut().exp = invalid_order.claims().iat;
        cases.push(invalid_order);

        for claims in cases {
            assert!(matches!(
                policy.validate(claims.claims(), "auth0|user".to_owned()),
                Err(VerifyError::InvalidLifetime)
            ));
        }
    }

    #[test]
    fn mcp_policy_rejects_machine_clients_and_invalid_scopes() {
        let policy = McpPolicy;

        assert!(matches!(
            policy.validate(mcp_claims().claims(), "client-123@clients".to_owned()),
            Err(VerifyError::MachineIdentity)
        ));
        let mut machine_grant = mcp_claims();
        machine_grant.claims_mut().extra.gty = Some("client-credentials".to_owned());
        assert!(matches!(
            policy.validate(machine_grant.claims(), "auth0|user".to_owned()),
            Err(VerifyError::MachineIdentity)
        ));

        for client in [None, Some(""), Some(" \t ")] {
            let mut claims = mcp_claims();
            claims.claims_mut().extra.azp = client.map(str::to_owned);
            assert!(matches!(
                policy.validate(claims.claims(), "auth0|user".to_owned()),
                Err(VerifyError::InvalidClient)
            ));
        }

        for scope in [
            None,
            Some(""),
            Some("offline_access"),
            Some("read_controls unknown"),
        ] {
            let mut claims = mcp_claims();
            claims.claims_mut().extra.scope = scope.map(str::to_owned);
            assert!(matches!(
                policy.validate(claims.claims(), "auth0|user".to_owned()),
                Err(VerifyError::InvalidScopes)
            ));
        }
    }

    #[tokio::test]
    async fn mcp_verifier_rejects_common_claims_tampering_and_expiry() {
        let (verifier, signing_key) = mcp_fixture();
        verifier
            .verify(&sign_with(&signing_key, mcp_claims()))
            .await
            .expect("valid token verifies");

        let mut wrong_issuer = mcp_claims();
        wrong_issuer.set_iss("https://other.example/");
        assert!(matches!(
            verifier
                .verify(&sign_with(&signing_key, wrong_issuer))
                .await,
            Err(VerifyError::IssuerMismatch)
        ));

        let mut wrong_audience = mcp_claims();
        wrong_audience.claims_mut().aud = Default::default();
        wrong_audience.add_aud("https://other.example/mcp");
        assert!(matches!(
            verifier
                .verify(&sign_with(&signing_key, wrong_audience))
                .await,
            Err(VerifyError::AudienceMismatch)
        ));

        let mut missing_subject = mcp_claims();
        missing_subject.claims_mut().sub = None;
        assert!(matches!(
            verifier
                .verify(&sign_with(&signing_key, missing_subject))
                .await,
            Err(VerifyError::MissingSubject)
        ));

        let valid = sign_with(&signing_key, mcp_claims());
        assert!(matches!(
            verifier.verify(&tamper_signature(&valid)).await,
            Err(VerifyError::InvalidToken)
        ));

        let mut expired = mcp_claims();
        expired.claims_mut().exp =
            Some(SystemTime::now().duration_since(UNIX_EPOCH).unwrap() - Duration::from_secs(1));
        assert!(matches!(
            verifier.verify(&sign_with(&signing_key, expired)).await,
            Err(VerifyError::Expired)
        ));
    }

    #[tokio::test]
    async fn both_policies_classify_unavailable_jwks_as_dependency_failure() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);
        let remote_url = format!("http://{address}/jwks");

        let (_, user_key) = signing_material();
        let user_verifier = Auth0TokenVerifier {
            verifier: Auth0Verifier {
                verifier: JwksVerifier::remote(remote_url.clone()),
                issuer: ISSUER.to_owned(),
                audience: API_AUDIENCE.to_owned(),
                policy: UserPolicy,
            },
        };
        let token = sign_with(&user_key, user_claims(ISSUER, API_AUDIENCE, "auth0|user"));
        assert!(matches!(
            user_verifier.verify(&token).await,
            Err(VerifyError::JwksUnavailable)
        ));

        let (_, mcp_key) = signing_material();
        let mcp_verifier = Auth0McpTokenVerifier {
            verifier: Auth0Verifier {
                verifier: JwksVerifier::remote(remote_url),
                issuer: ISSUER.to_owned(),
                audience: MCP_AUDIENCE.to_owned(),
                policy: McpPolicy,
            },
        };
        let token = sign_with(&mcp_key, mcp_claims());
        assert!(matches!(
            mcp_verifier.verify(&token).await,
            Err(VerifyError::JwksUnavailable)
        ));
    }
}
