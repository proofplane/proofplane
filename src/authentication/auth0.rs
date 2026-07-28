use std::{
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use async_trait::async_trait;
use jwtk::Claims;
use secrecy::{ExposeSecret, SecretString};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use url::Url;

use uuid::Uuid;

use crate::{
    authentication::jwks::JwksVerifier,
    config::Auth0Config,
    domain::{
        canonical_permissions, AgentConnectionId, DomainError, Sha256Digest, WorkspaceId,
        WorkspacePermission,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedAuditorIdentity {
    pub subject: String,
    pub email: String,
    pub email_verified: bool,
}

#[derive(Debug, Clone)]
pub struct AuditorIdentityExchange {
    pub authorization_code: SecretString,
    pub redirect_uri: Url,
    pub pkce_verifier: SecretString,
    pub expected_nonce_digest: Sha256Digest,
}

#[derive(Debug, thiserror::Error)]
pub enum AuditorIdentityProviderError {
    #[error("auditor identity was rejected")]
    Rejected,
    #[error("auditor identity provider is unavailable")]
    Unavailable,
}

impl From<VerifyError> for AuditorIdentityProviderError {
    fn from(error: VerifyError) -> Self {
        if error.is_token_rejection() {
            Self::Rejected
        } else {
            Self::Unavailable
        }
    }
}

#[async_trait]
pub trait AuditorIdentityProvider: Send + Sync {
    async fn exchange_and_verify(
        &self,
        exchange: AuditorIdentityExchange,
    ) -> Result<VerifiedAuditorIdentity, AuditorIdentityProviderError>;
}

pub type SharedAuditorIdentityProvider = Arc<dyn AuditorIdentityProvider>;

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
    #[error("token email is missing")]
    MissingEmail,
    #[error("token email is not verified")]
    EmailNotVerified,
    #[error("token nonce does not match the authentication transaction")]
    NonceMismatch,
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

        validate_lifetime(claims)?;

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

fn validate_lifetime<T>(claims: &Claims<T>) -> Result<(), VerifyError> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| VerifyError::InvalidLifetime)?;
    let issued_at = claims.iat.ok_or(VerifyError::InvalidLifetime)?;
    let expires_at = claims.exp.ok_or(VerifyError::InvalidLifetime)?;
    if issued_at > now + ISSUED_AT_LEEWAY || expires_at <= issued_at || expires_at <= now {
        return Err(VerifyError::InvalidLifetime);
    }

    Ok(())
}

fn optional_uuid_claim(value: &Option<String>) -> Result<Option<Uuid>, VerifyError> {
    value
        .as_deref()
        .map(Uuid::parse_str)
        .transpose()
        .map_err(|_| VerifyError::InvalidConnectionClaims)
}

#[derive(Default, Serialize, Deserialize)]
struct AuditorExtraClaims {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    email: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    email_verified: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    nonce: Option<String>,
}

struct VerifiedAuditorClaims {
    identity: VerifiedAuditorIdentity,
    nonce: String,
}

struct AuditorPolicy;

impl ClaimsPolicy for AuditorPolicy {
    type ExtraClaims = AuditorExtraClaims;
    type Output = VerifiedAuditorClaims;

    fn validate(
        &self,
        claims: &Claims<Self::ExtraClaims>,
        subject: String,
    ) -> Result<Self::Output, VerifyError> {
        validate_lifetime(claims)?;

        if subject.trim().is_empty() {
            return Err(VerifyError::MissingSubject);
        }
        let email = claims
            .extra
            .email
            .as_deref()
            .filter(|email| !email.trim().is_empty())
            .ok_or(VerifyError::MissingEmail)?
            .to_owned();
        if claims.extra.email_verified != Some(true) {
            return Err(VerifyError::EmailNotVerified);
        }
        let nonce = claims
            .extra
            .nonce
            .as_deref()
            .filter(|nonce| !nonce.trim().is_empty())
            .ok_or(VerifyError::NonceMismatch)?
            .to_owned();

        Ok(VerifiedAuditorClaims {
            identity: VerifiedAuditorIdentity {
                subject,
                email,
                email_verified: true,
            },
            nonce,
        })
    }
}

pub struct Auth0AuditorTokenVerifier {
    verifier: Auth0Verifier<AuditorPolicy>,
}

pub struct Auth0AuditorIdentityProvider {
    client: reqwest::Client,
    client_id: String,
    client_secret: SecretString,
    token_endpoint: Url,
    verifier: Auth0AuditorTokenVerifier,
}

impl Auth0AuditorIdentityProvider {
    pub fn new(config: &Auth0Config) -> Self {
        Self {
            client: reqwest::Client::new(),
            client_id: config.auditor_portal.client_id.clone(),
            client_secret: config.auditor_portal.client_secret.clone(),
            token_endpoint: config.auditor_portal.token_endpoint.clone(),
            verifier: Auth0AuditorTokenVerifier::new(config),
        }
    }
}

#[derive(Deserialize)]
struct AuditorTokenResponse {
    id_token: String,
}

#[async_trait]
impl AuditorIdentityProvider for Auth0AuditorIdentityProvider {
    async fn exchange_and_verify(
        &self,
        exchange: AuditorIdentityExchange,
    ) -> Result<VerifiedAuditorIdentity, AuditorIdentityProviderError> {
        let response = self
            .client
            .post(self.token_endpoint.clone())
            .form(&[
                ("grant_type", "authorization_code"),
                ("client_id", self.client_id.as_str()),
                ("client_secret", self.client_secret.expose_secret()),
                ("code", exchange.authorization_code.expose_secret()),
                ("redirect_uri", exchange.redirect_uri.as_str()),
                ("code_verifier", exchange.pkce_verifier.expose_secret()),
            ])
            .send()
            .await
            .map_err(|_| AuditorIdentityProviderError::Unavailable)?;

        if response.status().is_client_error() {
            return Err(AuditorIdentityProviderError::Rejected);
        }
        if !response.status().is_success() {
            return Err(AuditorIdentityProviderError::Unavailable);
        }

        let token = response
            .json::<AuditorTokenResponse>()
            .await
            .map_err(|_| AuditorIdentityProviderError::Unavailable)?;
        self.verifier
            .verify(&token.id_token, exchange.expected_nonce_digest)
            .await
            .map_err(AuditorIdentityProviderError::from)
    }
}

impl Auth0AuditorTokenVerifier {
    pub fn new(config: &Auth0Config) -> Self {
        Self {
            verifier: Auth0Verifier {
                verifier: JwksVerifier::remote(config.jwks_url.to_string()),
                issuer: config.issuer.to_string(),
                audience: config.auditor_portal.client_id.clone(),
                policy: AuditorPolicy,
            },
        }
    }

    pub async fn verify(
        &self,
        token: &str,
        expected_nonce_digest: Sha256Digest,
    ) -> Result<VerifiedAuditorIdentity, VerifyError> {
        let claims = self.verifier.verify(token).await?;
        if Sha256Digest::digest(claims.nonce.as_bytes()) != expected_nonce_digest {
            return Err(VerifyError::NonceMismatch);
        }

        Ok(claims.identity)
    }
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
    use axum::{
        extract::{Form, State},
        http::StatusCode,
        response::{IntoResponse, Response},
        routing::{get, post},
        Json, Router,
    };
    use jwtk::{
        hmac::{HmacAlgorithm, HmacKey},
        jwk::{JwkSet, WithKid},
        rsa::{RsaAlgorithm, RsaPrivateKey},
        sign, HeaderAndClaims, PublicKeyToJwk,
    };
    use std::{
        collections::HashMap,
        sync::{
            atomic::{AtomicU16, Ordering},
            Mutex,
        },
    };

    const ISSUER: &str = "https://tenant.auth0.com/";
    const API_AUDIENCE: &str = "https://api.proofplane.com";
    const MCP_AUDIENCE: &str = "https://mcp.proofplane.com/mcp";
    const CLIENT: &str = "client-123";
    const AUDITOR_CLIENT: &str = "auditor-client-123";
    const AUDITOR_NONCE: &str = "auditor-nonce-123";

    #[derive(Clone)]
    struct AuditorProviderServerState {
        jwks: serde_json::Value,
        token: String,
        token_status: Arc<AtomicU16>,
        forms: Arc<Mutex<Vec<HashMap<String, String>>>>,
    }

    async fn provider_jwks(
        State(state): State<AuditorProviderServerState>,
    ) -> Json<serde_json::Value> {
        Json(state.jwks)
    }

    async fn provider_token(
        State(state): State<AuditorProviderServerState>,
        Form(form): Form<HashMap<String, String>>,
    ) -> Response {
        state.forms.lock().expect("provider form lock").push(form);
        let status = StatusCode::from_u16(state.token_status.load(Ordering::SeqCst))
            .expect("configured status is valid");
        if status.is_success() {
            Json(serde_json::json!({ "id_token": state.token })).into_response()
        } else {
            status.into_response()
        }
    }

    async fn auditor_provider_fixture() -> (
        Auth0AuditorIdentityProvider,
        Arc<AtomicU16>,
        Arc<Mutex<Vec<HashMap<String, String>>>>,
        tokio::task::JoinHandle<()>,
    ) {
        let private_key =
            RsaPrivateKey::generate(2048, RsaAlgorithm::RS256).expect("rsa key generates");
        let signing_key =
            WithKid::new_with_thumbprint_id(private_key).expect("signing key derives kid");
        let jwk = signing_key.public_key_to_jwk().expect("public jwk derives");
        let token = sign_with(&signing_key, auditor_claims());
        let token_status = Arc::new(AtomicU16::new(StatusCode::OK.as_u16()));
        let forms = Arc::new(Mutex::new(Vec::new()));
        let state = AuditorProviderServerState {
            jwks: serde_json::to_value(JwkSet { keys: vec![jwk] }).expect("JWKS serializes"),
            token,
            token_status: token_status.clone(),
            forms: forms.clone(),
        };
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("provider listener binds");
        let address = listener.local_addr().expect("provider address");
        let server = tokio::spawn(async move {
            axum::serve(
                listener,
                Router::new()
                    .route("/jwks", get(provider_jwks))
                    .route("/oauth/token", post(provider_token))
                    .with_state(state),
            )
            .await
            .expect("provider server runs");
        });
        let config = Auth0Config {
            issuer: Url::parse(ISSUER).expect("issuer parses"),
            audience: API_AUDIENCE.to_owned(),
            jwks_url: Url::parse(&format!("http://{address}/jwks")).expect("JWKS URL parses"),
            upstream_oauth: crate::config::Auth0UpstreamOAuthConfig {
                client_id: CLIENT.to_owned(),
                client_secret: SecretString::from("management-secret"),
                callback_path: "/oauth/callback".to_owned(),
            },
            auditor_portal: crate::config::Auth0AuditorPortalConfig {
                client_id: AUDITOR_CLIENT.to_owned(),
                client_secret: SecretString::from("auditor-client-secret"),
                callback_path: "/auditor-access/auth0/callback".to_owned(),
                callback_url: Url::parse(
                    "https://api.proofplane.com/auditor-access/auth0/callback",
                )
                .expect("callback parses"),
                connection: "email".to_owned(),
                authorization_endpoint: Url::parse(&format!("http://{address}/authorize"))
                    .expect("authorization URL parses"),
                token_endpoint: Url::parse(&format!("http://{address}/oauth/token"))
                    .expect("token URL parses"),
            },
        };

        (
            Auth0AuditorIdentityProvider::new(&config),
            token_status,
            forms,
            server,
        )
    }

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

    #[tokio::test]
    async fn auditor_provider_sends_exact_exchange_and_classifies_endpoint_failures() {
        let (provider, token_status, forms, server) = auditor_provider_fixture().await;
        let exchange = || AuditorIdentityExchange {
            authorization_code: SecretString::from("authorization-code"),
            redirect_uri: Url::parse("https://api.proofplane.com/auditor-access/auth0/callback")
                .expect("callback parses"),
            pkce_verifier: SecretString::from("pkce-verifier"),
            expected_nonce_digest: Sha256Digest::digest(AUDITOR_NONCE.as_bytes()),
        };

        let identity = provider
            .exchange_and_verify(exchange())
            .await
            .expect("exchange verifies");
        assert_eq!(identity.subject, "email|auditor");
        assert_eq!(identity.email, "auditor@example.com");

        {
            let captured = forms.lock().expect("provider form lock");
            assert_eq!(captured.len(), 1);
            for (name, value) in [
                ("grant_type", "authorization_code"),
                ("client_id", AUDITOR_CLIENT),
                ("client_secret", "auditor-client-secret"),
                ("code", "authorization-code"),
                (
                    "redirect_uri",
                    "https://api.proofplane.com/auditor-access/auth0/callback",
                ),
                ("code_verifier", "pkce-verifier"),
            ] {
                assert_eq!(
                    captured[0].get(name).map(String::as_str),
                    Some(value),
                    "captured {name}"
                );
            }
        }

        token_status.store(StatusCode::BAD_REQUEST.as_u16(), Ordering::SeqCst);
        assert!(matches!(
            provider.exchange_and_verify(exchange()).await,
            Err(AuditorIdentityProviderError::Rejected)
        ));
        token_status.store(StatusCode::SERVICE_UNAVAILABLE.as_u16(), Ordering::SeqCst);
        assert!(matches!(
            provider.exchange_and_verify(exchange()).await,
            Err(AuditorIdentityProviderError::Unavailable)
        ));

        server.abort();
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

    fn auditor_fixture() -> (Auth0AuditorTokenVerifier, WithKid<RsaPrivateKey>) {
        let (jwks, signing_key) = signing_material();
        (
            Auth0AuditorTokenVerifier {
                verifier: Auth0Verifier {
                    verifier: jwks,
                    issuer: ISSUER.to_owned(),
                    audience: AUDITOR_CLIENT.to_owned(),
                    policy: AuditorPolicy,
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

    fn auditor_claims() -> HeaderAndClaims<AuditorExtraClaims> {
        let mut claims = HeaderAndClaims::with_claims(AuditorExtraClaims {
            email: Some("auditor@example.com".to_owned()),
            email_verified: Some(true),
            nonce: Some(AUDITOR_NONCE.to_owned()),
        });
        claims
            .set_iss(ISSUER)
            .set_sub("email|auditor")
            .add_aud(AUDITOR_CLIENT)
            .set_iat_now()
            .set_exp_from_now(Duration::from_secs(180));
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
    async fn auditor_policy_returns_verified_identity_for_expected_nonce() {
        let (verifier, signing_key) = auditor_fixture();
        let token = sign_with(&signing_key, auditor_claims());

        let identity = verifier
            .verify(&token, Sha256Digest::digest(AUDITOR_NONCE.as_bytes()))
            .await
            .expect("auditor token verifies");

        assert_eq!(
            identity,
            VerifiedAuditorIdentity {
                subject: "email|auditor".to_owned(),
                email: "auditor@example.com".to_owned(),
                email_verified: true,
            }
        );
    }

    #[test]
    fn auditor_verifier_uses_the_dedicated_client_audience() {
        let config =
            crate::config::load_from_path("config/local.yaml").expect("local config loads");
        let verifier = Auth0AuditorTokenVerifier::new(&config.auth0);

        assert_eq!(
            verifier.verifier.audience,
            config.auth0.auditor_portal.client_id
        );
        assert_eq!(verifier.verifier.issuer, config.auth0.issuer.as_str());
    }

    #[test]
    fn auditor_exchange_debug_output_redacts_protocol_secrets() {
        let exchange = AuditorIdentityExchange {
            authorization_code: SecretString::from("unique-authorization-code"),
            redirect_uri: Url::parse("https://api.proofplane.com/auditor-access/auth0/callback")
                .expect("redirect URI parses"),
            pkce_verifier: SecretString::from("unique-pkce-verifier"),
            expected_nonce_digest: Sha256Digest::digest(b"unique-expected-nonce"),
        };
        let debug = format!("{exchange:?}");

        assert!(!debug.contains("unique-authorization-code"));
        assert!(!debug.contains("unique-pkce-verifier"));
        assert!(!debug.contains("unique-expected-nonce"));
        assert!(debug.contains("[redacted]"));
    }

    #[tokio::test]
    async fn auditor_policy_rejects_invalid_algorithm_signature_issuer_and_audience() {
        let (verifier, signing_key) = auditor_fixture();
        let expected_nonce = Sha256Digest::digest(AUDITOR_NONCE.as_bytes());

        let hmac_key = HmacKey::generate(HmacAlgorithm::HS256).expect("hmac key generates");
        let hmac_with_kid = WithKid::new(signing_key.kid().to_owned(), hmac_key);
        let mut claims = auditor_claims();
        let token = sign(&mut claims, &hmac_with_kid).expect("HS256 token signs");
        assert!(matches!(
            verifier.verify(&token, expected_nonce).await,
            Err(VerifyError::InvalidToken)
        ));

        let valid = sign_with(&signing_key, auditor_claims());
        assert!(matches!(
            verifier
                .verify(&tamper_signature(&valid), expected_nonce)
                .await,
            Err(VerifyError::InvalidToken)
        ));
        assert!(matches!(
            verifier.verify("not-a-token", expected_nonce).await,
            Err(VerifyError::InvalidToken)
        ));

        let mut missing_issuer = auditor_claims();
        missing_issuer.claims_mut().iss = None;
        assert!(matches!(
            verifier
                .verify(&sign_with(&signing_key, missing_issuer), expected_nonce)
                .await,
            Err(VerifyError::MissingIssuer)
        ));

        let mut wrong_issuer = auditor_claims();
        wrong_issuer.set_iss("https://other.example/");
        assert!(matches!(
            verifier
                .verify(&sign_with(&signing_key, wrong_issuer), expected_nonce)
                .await,
            Err(VerifyError::IssuerMismatch)
        ));

        let mut wrong_audience = auditor_claims();
        wrong_audience.claims_mut().aud = Default::default();
        wrong_audience.add_aud("other-client");
        assert!(matches!(
            verifier
                .verify(&sign_with(&signing_key, wrong_audience), expected_nonce)
                .await,
            Err(VerifyError::AudienceMismatch)
        ));
    }

    #[tokio::test]
    async fn auditor_policy_rejects_invalid_lifetime_and_identity_claims() {
        let (verifier, signing_key) = auditor_fixture();
        let expected_nonce = Sha256Digest::digest(AUDITOR_NONCE.as_bytes());
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();

        let mut missing_iat = auditor_claims();
        missing_iat.claims_mut().iat = None;
        let mut missing_exp = auditor_claims();
        missing_exp.claims_mut().exp = None;
        let mut future_iat = auditor_claims();
        future_iat.claims_mut().iat = Some(now + Duration::from_secs(60));
        let mut invalid_order = auditor_claims();
        invalid_order.claims_mut().exp = invalid_order.claims().iat;

        for claims in [missing_iat, missing_exp, future_iat, invalid_order] {
            assert!(matches!(
                verifier
                    .verify(&sign_with(&signing_key, claims), expected_nonce)
                    .await,
                Err(VerifyError::InvalidLifetime | VerifyError::Expired)
            ));
        }

        for subject in [None, Some(""), Some("   ")] {
            let mut claims = auditor_claims();
            claims.claims_mut().sub = subject.map(str::to_owned);
            assert!(matches!(
                verifier
                    .verify(&sign_with(&signing_key, claims), expected_nonce)
                    .await,
                Err(VerifyError::MissingSubject)
            ));
        }

        for email in [None, Some(""), Some("   ")] {
            let mut claims = auditor_claims();
            claims.claims_mut().extra.email = email.map(str::to_owned);
            assert!(matches!(
                verifier
                    .verify(&sign_with(&signing_key, claims), expected_nonce)
                    .await,
                Err(VerifyError::MissingEmail)
            ));
        }

        for email_verified in [None, Some(false)] {
            let mut claims = auditor_claims();
            claims.claims_mut().extra.email_verified = email_verified;
            assert!(matches!(
                verifier
                    .verify(&sign_with(&signing_key, claims), expected_nonce)
                    .await,
                Err(VerifyError::EmailNotVerified)
            ));
        }
    }

    #[tokio::test]
    async fn auditor_policy_rejects_missing_or_mismatched_nonce() {
        let (verifier, signing_key) = auditor_fixture();
        let expected_nonce = Sha256Digest::digest(AUDITOR_NONCE.as_bytes());

        for nonce in [None, Some(""), Some("other-nonce")] {
            let mut claims = auditor_claims();
            claims.claims_mut().extra.nonce = nonce.map(str::to_owned);
            assert!(matches!(
                verifier
                    .verify(&sign_with(&signing_key, claims), expected_nonce)
                    .await,
                Err(VerifyError::NonceMismatch)
            ));
        }
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
    async fn all_policies_classify_unavailable_jwks_as_dependency_failure() {
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

        let (_, auditor_key) = signing_material();
        let auditor_verifier = Auth0AuditorTokenVerifier {
            verifier: Auth0Verifier {
                verifier: JwksVerifier::remote(format!("http://{address}/jwks")),
                issuer: ISSUER.to_owned(),
                audience: AUDITOR_CLIENT.to_owned(),
                policy: AuditorPolicy,
            },
        };
        let token = sign_with(&auditor_key, auditor_claims());
        assert!(matches!(
            auditor_verifier
                .verify(&token, Sha256Digest::digest(AUDITOR_NONCE.as_bytes()))
                .await,
            Err(VerifyError::JwksUnavailable)
        ));
    }

    #[test]
    fn only_jwks_failures_are_classified_as_unavailable() {
        assert!(!VerifyError::JwksUnavailable.is_token_rejection());
        assert!(VerifyError::InvalidToken.is_token_rejection());
        assert!(VerifyError::EmailNotVerified.is_token_rejection());
        assert!(VerifyError::NonceMismatch.is_token_rejection());
        assert!(matches!(
            AuditorIdentityProviderError::from(VerifyError::JwksUnavailable),
            AuditorIdentityProviderError::Unavailable
        ));
        assert!(matches!(
            AuditorIdentityProviderError::from(VerifyError::InvalidToken),
            AuditorIdentityProviderError::Rejected
        ));
    }
}
