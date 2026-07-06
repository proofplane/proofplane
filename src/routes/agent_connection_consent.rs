use std::{collections::HashSet, sync::Arc};

use axum::{
    extract::{Form, Query, State},
    http::{header, HeaderValue, StatusCode},
    response::{Html, IntoResponse, Response},
    routing::get,
    Router,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use url::Url;
use uuid::Uuid;

use crate::{
    authentication::auth0_redirect_token::{
        ConsentDecision, ConsentResultClaims, ConsentTransactionClaims, RedirectTokenCodec,
        RedirectTokenError, MAX_TOKEN_LIFETIME_SECONDS,
    },
    domain::{canonical_permissions, UserId, WorkspaceId, WorkspacePermission},
    services::agent_connections::{
        AgentConnectionError, AgentConnectionService, ConsentContextOutcome,
        CreatePendingConnectionPayload,
    },
    validate,
    validation::Validation,
};

const UNAVAILABLE_HTML: &str = "<!doctype html><html><head><meta charset=\"utf-8\"><title>Authorization unavailable</title></head><body><main><h1>Authorization unavailable</h1><p>This authorization request cannot be completed. Return to your client and try again.</p></main></body></html>";

pub trait ConsentResultSigner: Send + Sync {
    fn sign_result(&self, claims: ConsentResultClaims) -> Result<String, RedirectTokenError>;
}

impl ConsentResultSigner for RedirectTokenCodec {
    fn sign_result(&self, claims: ConsentResultClaims) -> Result<String, RedirectTokenError> {
        RedirectTokenCodec::sign_result(self, claims)
    }
}

#[derive(Clone)]
pub struct AgentConnectionConsentState {
    pub service: AgentConnectionService,
    pub token_codec: Arc<RedirectTokenCodec>,
    pub result_signer: Arc<dyn ConsentResultSigner>,
    pub resource: String,
    pub allowed_client_ids: HashSet<String>,
    pub auth0_continue_url: Url,
}

pub fn router(state: AgentConnectionConsentState) -> Router {
    Router::new()
        .route(
            "/agent-connections/consent",
            get(show_consent).post(submit_consent),
        )
        .with_state(state)
}

#[derive(Debug, Deserialize)]
struct ConsentQuery {
    session_token: String,
    state: String,
}

#[derive(Debug, Deserialize)]
struct ConsentForm {
    session_token: String,
    state: String,
    decision: String,
    workspace_id: Option<String>,
}

async fn show_consent(
    State(state): State<AgentConnectionConsentState>,
    query: Result<Query<ConsentQuery>, axum::extract::rejection::QueryRejection>,
) -> Response {
    let Ok(Query(query)) = query else {
        return unavailable();
    };
    if query.state.trim().is_empty() {
        return unavailable();
    }
    let claims = match verify_transaction(&state, &query.session_token) {
        Ok(claims) => claims,
        Err(()) => return unavailable(),
    };
    let context = match state.service.consent_context(&claims.sub).await {
        Ok(ConsentContextOutcome::Available(context)) => context,
        Ok(ConsentContextOutcome::Unavailable) | Err(_) => return unavailable(),
    };

    secure_response(
        StatusCode::OK,
        render_consent(&query, &claims, &context.workspaces),
    )
}

async fn submit_consent(
    State(state): State<AgentConnectionConsentState>,
    form: Result<Form<ConsentForm>, axum::extract::rejection::FormRejection>,
) -> Response {
    let Ok(Form(form)) = form else {
        return unavailable();
    };
    if form.state.trim().is_empty() {
        return unavailable();
    }
    let claims = match verify_transaction(&state, &form.session_token) {
        Ok(claims) => claims,
        Err(()) => return unavailable(),
    };
    let now = Utc::now();

    match form.decision.as_str() {
        "deny" => {
            let result = denied_result(&claims, &form.state, now);
            redirect_with_result(&state, &form.state, result)
        }
        "approve" => {
            let Some(workspace_id) = form.workspace_id else {
                return unavailable();
            };
            let context = match state.service.consent_context(&claims.sub).await {
                Ok(ConsentContextOutcome::Available(context)) => context,
                Ok(ConsentContextOutcome::Unavailable) => return unavailable(),
                Err(error) => {
                    log_service_error(error);
                    return unavailable();
                }
            };
            let (continuation_token, nonce) = match (random_secret(), random_secret()) {
                (Ok(continuation_token), Ok(nonce)) => (continuation_token, nonce),
                _ => return unavailable(),
            };
            let pending = PendingCreationRequest::from_claims(
                &claims,
                workspace_id,
                continuation_token.clone(),
                nonce.clone(),
            )
            .with_user_id(context.user.id)
            .into_payload()
            .into_result();
            let Ok(pending) = pending else {
                return unavailable();
            };
            if let Err(error) = state.service.create_pending(pending).await {
                log_service_error(error);
                return unavailable();
            }

            let result =
                approved_result(&claims, &form.state, continuation_token.clone(), nonce, now);
            let token = match state.result_signer.sign_result(result) {
                Ok(token) => token,
                Err(_) => {
                    if let Err(error) = state.service.deny_pending(&continuation_token).await {
                        tracing::error!(%error, "failed to clean up pending consent after signing failure");
                    }
                    return unavailable();
                }
            };
            redirect_to_auth0(&state, &form.state, &token)
        }
        _ => unavailable(),
    }
}

fn verify_transaction(
    state: &AgentConnectionConsentState,
    token: &str,
) -> Result<ConsentTransactionClaims, ()> {
    let claims = state
        .token_codec
        .verify_transaction(token, Utc::now().timestamp())
        .map_err(|_| ())?;
    if claims.resource != state.resource
        || !state.allowed_client_ids.contains(&claims.client_id)
        || required("subject", claims.sub.clone()).is_invalid()
        || required("transaction_id", claims.transaction_id.clone()).is_invalid()
        || required("oauth_state", claims.oauth_state.clone()).is_invalid()
        || required("client_name", claims.client_name.clone()).is_invalid()
        || parse_scopes(claims.scopes.clone()).is_invalid()
    {
        return Err(());
    }
    Ok(claims)
}

#[derive(Debug)]
struct PendingCreationRequest {
    user_id: String,
    workspace_id: String,
    auth0_subject: String,
    auth0_client_id: String,
    client_display_name: String,
    resource: String,
    scopes: Vec<String>,
    expires_at: i64,
    continuation_token: String,
    nonce: String,
}

impl PendingCreationRequest {
    fn from_claims(
        claims: &ConsentTransactionClaims,
        workspace_id: String,
        continuation_token: String,
        nonce: String,
    ) -> Self {
        Self {
            user_id: String::new(),
            workspace_id,
            auth0_subject: claims.sub.clone(),
            auth0_client_id: claims.client_id.clone(),
            client_display_name: claims.client_name.clone(),
            resource: claims.resource.clone(),
            scopes: claims.scopes.clone(),
            expires_at: claims.exp,
            continuation_token,
            nonce,
        }
    }

    fn with_user_id(mut self, user_id: UserId) -> Self {
        self.user_id = user_id.to_string();
        self
    }

    fn into_payload(self) -> Validation<CreatePendingConnectionPayload, String> {
        validate! {
            user_id <- parse_user_id(self.user_id),
            workspace_id <- parse_workspace_id(self.workspace_id),
            auth0_subject <- required("subject", self.auth0_subject),
            auth0_client_id <- required("client_id", self.auth0_client_id),
            client_display_name <- required("client_name", self.client_display_name),
            resource <- canonical_resource(self.resource),
            permissions <- parse_scopes(self.scopes),
            expires_at <- timestamp("expires_at", self.expires_at),
            continuation_token <- required("continuation_token", self.continuation_token),
            nonce <- required("nonce", self.nonce),
            => CreatePendingConnectionPayload {
                user_id,
                workspace_id,
                auth0_subject,
                auth0_client_id,
                client_display_name,
                resource,
                permissions,
                expires_at,
                continuation_token,
                nonce,
            },
        }
    }
}

fn denied_result(
    claims: &ConsentTransactionClaims,
    state: &str,
    now: DateTime<Utc>,
) -> ConsentResultClaims {
    result_claims(claims, state, ConsentDecision::Denied, None, None, now)
}

fn approved_result(
    claims: &ConsentTransactionClaims,
    state: &str,
    continuation_token: String,
    nonce: String,
    now: DateTime<Utc>,
) -> ConsentResultClaims {
    result_claims(
        claims,
        state,
        ConsentDecision::Approved,
        Some(continuation_token),
        Some(nonce),
        now,
    )
}

fn result_claims(
    claims: &ConsentTransactionClaims,
    state: &str,
    decision: ConsentDecision,
    continuation_token: Option<String>,
    nonce: Option<String>,
    now: DateTime<Utc>,
) -> ConsentResultClaims {
    let issued_at = now.timestamp();
    ConsentResultClaims {
        purpose: String::new(),
        version: 0,
        decision,
        sub: claims.sub.clone(),
        transaction_id: claims.transaction_id.clone(),
        oauth_state: claims.oauth_state.clone(),
        state: state.to_owned(),
        continuation_token,
        nonce,
        iss: String::new(),
        aud: String::new(),
        iat: issued_at,
        exp: claims.exp.min(issued_at + MAX_TOKEN_LIFETIME_SECONDS),
    }
}

fn redirect_with_result(
    state: &AgentConnectionConsentState,
    auth0_state: &str,
    result: ConsentResultClaims,
) -> Response {
    match state.result_signer.sign_result(result) {
        Ok(token) => redirect_to_auth0(state, auth0_state, &token),
        Err(_) => unavailable(),
    }
}

fn redirect_to_auth0(
    state: &AgentConnectionConsentState,
    auth0_state: &str,
    result_token: &str,
) -> Response {
    let mut url = state.auth0_continue_url.clone();
    url.query_pairs_mut()
        .append_pair("state", auth0_state)
        .append_pair("session_token", result_token);
    let mut response = secure_response(StatusCode::SEE_OTHER, String::new());
    match HeaderValue::from_str(url.as_str()) {
        Ok(location) => {
            response.headers_mut().insert(header::LOCATION, location);
            response
        }
        Err(_) => unavailable(),
    }
}

fn render_consent(
    query: &ConsentQuery,
    claims: &ConsentTransactionClaims,
    workspaces: &[crate::domain::WorkspaceWithRole],
) -> String {
    let mut options = String::new();
    for workspace in workspaces {
        options.push_str(&format!(
            "<option value=\"{}\">{}</option>",
            workspace.workspace.id,
            escape_html(&workspace.workspace.name)
        ));
    }
    let scopes = claims
        .scopes
        .iter()
        .map(|scope| format!("<li>{}</li>", escape_html(scope)))
        .collect::<String>();
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>Authorize agent connection</title></head><body><main><h1>Authorize {client}</h1><p>Select one workspace for this connection.</p><ul>{scopes}</ul><form method=\"post\"><input type=\"hidden\" name=\"session_token\" value=\"{token}\"><input type=\"hidden\" name=\"state\" value=\"{state}\"><label>Workspace<select name=\"workspace_id\" required>{options}</select></label><button type=\"submit\" name=\"decision\" value=\"approve\">Approve</button><button type=\"submit\" name=\"decision\" value=\"deny\">Deny</button></form></main></body></html>",
        client = escape_html(&claims.client_name),
        token = escape_html(&query.session_token),
        state = escape_html(&query.state),
    )
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn random_secret() -> Result<String, getrandom::Error> {
    let mut bytes = [0_u8; 32];
    getrandom::fill(&mut bytes)?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

fn parse_user_id(value: String) -> Validation<UserId, String> {
    value
        .parse::<Uuid>()
        .map(UserId::from)
        .map(Validation::valid)
        .unwrap_or_else(|_| Validation::invalid("user_id must be a UUID".to_owned()))
}

fn parse_workspace_id(value: String) -> Validation<WorkspaceId, String> {
    value
        .parse::<Uuid>()
        .map(WorkspaceId::from)
        .map(Validation::valid)
        .unwrap_or_else(|_| Validation::invalid("workspace_id must be a UUID".to_owned()))
}

fn timestamp(field: &str, value: i64) -> Validation<DateTime<Utc>, String> {
    DateTime::from_timestamp(value, 0)
        .map(Validation::valid)
        .unwrap_or_else(|| Validation::invalid(format!("{field} must be a valid timestamp")))
}

fn canonical_resource(value: String) -> Validation<String, String> {
    let Ok(url) = Url::parse(&value) else {
        return Validation::invalid("resource must be an absolute URL".to_owned());
    };
    if url.query().is_some() || url.fragment().is_some() || url.as_str() != value {
        return Validation::invalid(
            "resource must be a canonical URL without query or fragment".to_owned(),
        );
    }
    Validation::valid(value)
}

fn parse_scopes(values: Vec<String>) -> Validation<Vec<WorkspacePermission>, String> {
    if values.is_empty() {
        return Validation::invalid("scopes must not be empty".to_owned());
    }
    let mut permissions = Vec::with_capacity(values.len());
    let mut errors = Vec::new();
    for value in values {
        match value.parse::<WorkspacePermission>() {
            Ok(permission) if permissions.contains(&permission) => {
                errors.push(format!("scopes contains duplicate value {value}"));
            }
            Ok(permission) => permissions.push(permission),
            Err(_) => errors.push(format!("unknown scope: {value}")),
        }
    }
    if !errors.is_empty() {
        return Validation::invalid_many(errors);
    }
    Validation::valid(canonical_permissions(permissions).expect("scopes are unique"))
}

fn required(field: &str, value: String) -> Validation<String, String> {
    if value.trim().is_empty() {
        Validation::invalid(format!("{field} must not be blank"))
    } else {
        Validation::valid(value)
    }
}

fn unavailable() -> Response {
    secure_response(StatusCode::BAD_REQUEST, UNAVAILABLE_HTML.to_owned())
}

fn secure_response(status: StatusCode, body: String) -> Response {
    let mut response = (status, Html(body)).into_response();
    let headers = response.headers_mut();
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("no-store, max-age=0"),
    );
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(
            "default-src 'none'; form-action 'self'; base-uri 'none'; frame-ancestors 'none'",
        ),
    );
    headers.insert(
        header::HeaderName::from_static("x-frame-options"),
        HeaderValue::from_static("DENY"),
    );
    response
}

fn log_service_error(error: AgentConnectionError) {
    tracing::error!(%error, "agent connection consent failed");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_escapes_client_workspace_state_and_token_content() {
        let query = ConsentQuery {
            session_token: "\"><script>token</script>".to_owned(),
            state: "\" onfocus=\"alert(1)".to_owned(),
        };
        let claims = ConsentTransactionClaims {
            purpose: String::new(),
            version: 1,
            transaction_id: "transaction".to_owned(),
            oauth_state: "oauth".to_owned(),
            client_id: "client".to_owned(),
            client_name: "<Client & Co>".to_owned(),
            resource: "https://mcp.example/mcp".to_owned(),
            scopes: vec!["read_controls".to_owned()],
            sub: "auth0|user".to_owned(),
            iss: String::new(),
            aud: String::new(),
            iat: 1,
            exp: 2,
        };
        let html = render_consent(&query, &claims, &[]);
        assert!(html.contains("&lt;Client &amp; Co&gt;"));
        assert!(!html.contains("<script>"));
        assert!(!html.contains("onfocus=\"alert"));
    }

    #[test]
    fn pending_request_accumulates_all_field_errors() {
        let errors = PendingCreationRequest {
            user_id: String::new(),
            workspace_id: "no".to_owned(),
            auth0_subject: " ".to_owned(),
            auth0_client_id: " ".to_owned(),
            client_display_name: " ".to_owned(),
            resource: "no".to_owned(),
            scopes: Vec::new(),
            expires_at: i64::MAX,
            continuation_token: " ".to_owned(),
            nonce: " ".to_owned(),
        }
        .into_payload()
        .into_result()
        .expect_err("request is invalid");
        assert_eq!(errors.len(), 10);
    }

    #[test]
    fn generated_secrets_are_independent_256_bit_values() {
        let left = random_secret().unwrap();
        let right = random_secret().unwrap();
        assert_ne!(left, right);
        assert_eq!(URL_SAFE_NO_PAD.decode(left).unwrap().len(), 32);
        assert_eq!(URL_SAFE_NO_PAD.decode(right).unwrap().len(), 32);
    }
}
