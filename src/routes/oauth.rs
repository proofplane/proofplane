use axum::{
    extract::{Form, Query, State},
    http::{header, HeaderValue, StatusCode},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use reqwest::Client;
use secrecy::ExposeSecret;
use serde::{Deserialize, Serialize};
use url::Url;
use uuid::Uuid;

use crate::{
    authentication::{
        auth0::{TokenVerifier, VerifiedClaims},
        UserAuthenticator,
    },
    config::Auth0Config,
    domain::{OAuthAuthorizationRequestId, WorkspacePermission},
    services::oauth::{
        parse_scope, valid_redirect_uri, ApproveConsentPayload, AuthorizePayload, CallbackOutcome,
        OAuthConsentContext, OAuthError, OAuthService, RegisterClientPayload, TokenPayload,
    },
};

pub struct OAuthState<V: TokenVerifier<Claims = VerifiedClaims>> {
    pub service: OAuthService,
    pub user_authenticator: UserAuthenticator<V>,
    pub auth0: Auth0Config,
    pub issuer: Url,
    pub resource: Url,
    pub http: Client,
}

impl<V: TokenVerifier<Claims = VerifiedClaims>> Clone for OAuthState<V> {
    fn clone(&self) -> Self {
        Self {
            service: self.service.clone(),
            user_authenticator: self.user_authenticator.clone(),
            auth0: self.auth0.clone(),
            issuer: self.issuer.clone(),
            resource: self.resource.clone(),
            http: self.http.clone(),
        }
    }
}

pub fn router<V>(state: OAuthState<V>) -> Router
where
    V: TokenVerifier<Claims = VerifiedClaims> + 'static,
{
    Router::new()
        .route(
            "/.well-known/oauth-authorization-server",
            get(authorization_server_metadata::<V>),
        )
        .route("/oauth/register", post(register_client::<V>))
        .route("/oauth/authorize", get(authorize::<V>))
        .route("/oauth/auth0/callback", get(auth0_callback::<V>))
        .route("/oauth/consent", post(submit_consent::<V>))
        .route("/oauth/token", post(token::<V>))
        .with_state(state)
}

#[derive(Serialize)]
struct AuthorizationServerMetadata {
    issuer: String,
    authorization_endpoint: String,
    token_endpoint: String,
    registration_endpoint: String,
    response_types_supported: Vec<&'static str>,
    grant_types_supported: Vec<&'static str>,
    code_challenge_methods_supported: Vec<&'static str>,
    token_endpoint_auth_methods_supported: Vec<&'static str>,
    scopes_supported: Vec<&'static str>,
}

async fn authorization_server_metadata<V>(
    State(state): State<OAuthState<V>>,
) -> Json<AuthorizationServerMetadata>
where
    V: TokenVerifier<Claims = VerifiedClaims>,
{
    Json(AuthorizationServerMetadata {
        issuer: state.issuer.to_string(),
        authorization_endpoint: endpoint(&state.issuer, "oauth/authorize"),
        token_endpoint: endpoint(&state.issuer, "oauth/token"),
        registration_endpoint: endpoint(&state.issuer, "oauth/register"),
        response_types_supported: vec!["code"],
        grant_types_supported: vec!["authorization_code"],
        code_challenge_methods_supported: vec!["S256"],
        token_endpoint_auth_methods_supported: vec!["none"],
        scopes_supported: WorkspacePermission::ALL
            .iter()
            .map(|permission| permission.as_str())
            .collect(),
    })
}

#[derive(Deserialize)]
struct RegisterRequest {
    #[serde(default)]
    client_name: Option<String>,
    redirect_uris: Vec<String>,
    #[serde(default)]
    token_endpoint_auth_method: Option<String>,
    #[serde(default)]
    grant_types: Vec<String>,
}

#[derive(Serialize)]
struct RegisterResponse {
    client_id: String,
    client_name: String,
    redirect_uris: Vec<String>,
    token_endpoint_auth_method: &'static str,
    grant_types: Vec<&'static str>,
    response_types: Vec<&'static str>,
    client_id_issued_at: i64,
}

async fn register_client<V>(
    State(state): State<OAuthState<V>>,
    Json(body): Json<RegisterRequest>,
) -> Response
where
    V: TokenVerifier<Claims = VerifiedClaims>,
{
    if !body.grant_types.is_empty()
        && !body
            .grant_types
            .iter()
            .any(|grant_type| grant_type == "authorization_code")
    {
        return oauth_json_error(StatusCode::BAD_REQUEST, "invalid_client_metadata");
    }
    if body
        .token_endpoint_auth_method
        .as_deref()
        .is_some_and(|method| method != "none")
    {
        return oauth_json_error(StatusCode::BAD_REQUEST, "invalid_client_metadata");
    }
    if body.redirect_uris.is_empty()
        || body
            .redirect_uris
            .iter()
            .any(|uri| !valid_redirect_uri(uri))
    {
        return oauth_json_error(StatusCode::BAD_REQUEST, "invalid_client_metadata");
    }
    let client_name = body
        .client_name
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| "MCP client".to_owned());
    match state
        .service
        .register_client(RegisterClientPayload {
            client_name,
            redirect_uris: body.redirect_uris,
        })
        .await
    {
        Ok(client) => (
            StatusCode::CREATED,
            Json(RegisterResponse {
                client_id: client.id,
                client_name: client.client_name,
                redirect_uris: client.redirect_uris,
                token_endpoint_auth_method: "none",
                grant_types: vec!["authorization_code"],
                response_types: vec!["code"],
                client_id_issued_at: client.created_at.timestamp(),
            }),
        )
            .into_response(),
        Err(_) => oauth_json_error(StatusCode::BAD_REQUEST, "invalid_client_metadata"),
    }
}

#[derive(Deserialize)]
struct AuthorizeQuery {
    response_type: String,
    client_id: String,
    redirect_uri: String,
    code_challenge: String,
    code_challenge_method: String,
    resource: String,
    scope: String,
    state: Option<String>,
}

async fn authorize<V>(
    State(state): State<OAuthState<V>>,
    Query(query): Query<AuthorizeQuery>,
) -> Response
where
    V: TokenVerifier<Claims = VerifiedClaims>,
{
    if query.response_type != "code"
        || query.code_challenge_method != "S256"
        || query.resource != state.resource.as_str()
    {
        return oauth_json_error(StatusCode::BAD_REQUEST, "invalid_request");
    }
    if query.code_challenge.trim().is_empty() {
        return oauth_json_error(StatusCode::BAD_REQUEST, "invalid_request");
    }
    let Ok(scopes) = parse_scope(&query.scope) else {
        return oauth_json_error(StatusCode::BAD_REQUEST, "invalid_request");
    };
    let prepared = match state
        .service
        .prepare_authorization(AuthorizePayload {
            client_id: query.client_id,
            redirect_uri: query.redirect_uri,
            code_challenge: query.code_challenge,
            scopes,
            state: query.state,
        })
        .await
    {
        Ok(prepared) => prepared,
        Err(_) => return oauth_json_error(StatusCode::BAD_REQUEST, "invalid_request"),
    };
    let Ok(mut url) = state.auth0.issuer.join("authorize") else {
        return oauth_json_error(StatusCode::BAD_REQUEST, "server_error");
    };
    let Ok(callback) = state.issuer.join(
        state
            .auth0
            .upstream_oauth
            .callback_path
            .trim_start_matches('/'),
    ) else {
        return oauth_json_error(StatusCode::BAD_REQUEST, "server_error");
    };
    url.query_pairs_mut()
        .append_pair("response_type", "code")
        .append_pair("client_id", &state.auth0.upstream_oauth.client_id)
        .append_pair("redirect_uri", callback.as_str())
        .append_pair("scope", "openid profile email")
        .append_pair("audience", &state.auth0.audience)
        .append_pair("state", &prepared.csrf_token);
    redirect(url)
}

#[derive(Deserialize)]
struct Auth0CallbackQuery {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
}

#[derive(Deserialize)]
struct Auth0TokenResponse {
    access_token: String,
}

async fn auth0_callback<V>(
    State(state): State<OAuthState<V>>,
    Query(query): Query<Auth0CallbackQuery>,
) -> Response
where
    V: TokenVerifier<Claims = VerifiedClaims>,
{
    if query.error.is_some() {
        return oauth_html_error();
    }
    let (Some(code), Some(csrf)) = (query.code, query.state) else {
        return oauth_html_error();
    };
    let Ok(callback) = state.issuer.join(
        state
            .auth0
            .upstream_oauth
            .callback_path
            .trim_start_matches('/'),
    ) else {
        return oauth_html_error();
    };
    let Ok(token_endpoint) = state.auth0.issuer.join("oauth/token") else {
        return oauth_html_error();
    };
    let token_response = match state
        .http
        .post(token_endpoint)
        .form(&[
            ("grant_type", "authorization_code"),
            ("client_id", state.auth0.upstream_oauth.client_id.as_str()),
            (
                "client_secret",
                state.auth0.upstream_oauth.client_secret.expose_secret(),
            ),
            ("code", code.as_str()),
            ("redirect_uri", callback.as_str()),
        ])
        .send()
        .await
        .and_then(|response| response.error_for_status())
    {
        Ok(response) => response,
        Err(error) => {
            tracing::error!(%error, "Auth0 OAuth token exchange failed");
            return oauth_html_error();
        }
    };
    let token_response = match token_response.json::<Auth0TokenResponse>().await {
        Ok(token_response) => token_response,
        Err(error) => {
            tracing::error!(%error, "Auth0 OAuth token response was invalid");
            return oauth_html_error();
        }
    };
    let user = match state
        .user_authenticator
        .authenticate(&token_response.access_token)
        .await
    {
        Ok(user) => user,
        Err(error) => {
            tracing::error!(%error, "Auth0 OAuth identity verification failed");
            return oauth_html_error();
        }
    };
    match state
        .service
        .complete_upstream_login(&csrf, user.auth0_sub, user.user_id)
        .await
    {
        Ok(CallbackOutcome::Reusable { redirect_uri }) => redirect(redirect_uri),
        Ok(CallbackOutcome::ConsentRequired { context }) => consent_page(*context, None),
        Err(error) => {
            tracing::error!(%error, "OAuth callback failed");
            oauth_html_error()
        }
    }
}

#[derive(Deserialize)]
struct ConsentForm {
    request_id: String,
    decision: String,
}

async fn submit_consent<V>(
    State(state): State<OAuthState<V>>,
    Form(form): Form<ConsentForm>,
) -> Response
where
    V: TokenVerifier<Claims = VerifiedClaims>,
{
    if form.decision != "approve" {
        return oauth_html_error();
    }
    let Ok(request_id) = Uuid::parse_str(&form.request_id).map(OAuthAuthorizationRequestId::from)
    else {
        return oauth_html_error();
    };
    match state
        .service
        .approve_consent(ApproveConsentPayload { request_id })
        .await
    {
        Ok(redirect_uri) => redirect(redirect_uri),
        Err(error @ OAuthError::InvalidGrant) => {
            tracing::error!(%error, "OAuth consent failed");
            oauth_html_error()
        }
        Err(error) => {
            tracing::error!(%error, "OAuth consent failed");
            oauth_html_error()
        }
    }
}

#[derive(Deserialize)]
struct TokenForm {
    grant_type: String,
    client_id: String,
    redirect_uri: String,
    code: String,
    code_verifier: String,
}

#[derive(Serialize)]
struct TokenResponse {
    access_token: String,
    token_type: &'static str,
    expires_in: i64,
    scope: String,
}

async fn token<V>(State(state): State<OAuthState<V>>, Form(form): Form<TokenForm>) -> Response
where
    V: TokenVerifier<Claims = VerifiedClaims>,
{
    if form.grant_type != "authorization_code" {
        return oauth_json_error(StatusCode::BAD_REQUEST, "unsupported_grant_type");
    }
    match state
        .service
        .issue_access_token(TokenPayload {
            client_id: form.client_id,
            redirect_uri: form.redirect_uri,
            code: form.code,
            code_verifier: form.code_verifier,
        })
        .await
    {
        Ok(issued) => (
            StatusCode::OK,
            Json(TokenResponse {
                access_token: issued.token,
                token_type: "Bearer",
                expires_in: 86_400,
                scope: WorkspacePermission::ALL
                    .iter()
                    .map(|permission| permission.as_str())
                    .collect::<Vec<_>>()
                    .join(" "),
            }),
        )
            .into_response(),
        Err(OAuthError::InvalidGrant) => oauth_json_error(StatusCode::BAD_REQUEST, "invalid_grant"),
        Err(error) => {
            tracing::error!(%error, "OAuth token exchange failed");
            oauth_json_error(StatusCode::BAD_REQUEST, "invalid_request")
        }
    }
}

fn consent_page(context: OAuthConsentContext, message: Option<&str>) -> Response {
    secure_html(render_consent_page(context, message))
}

fn render_consent_page(context: OAuthConsentContext, message: Option<&str>) -> String {
    let request = &context.request;
    let scopes = request
        .scopes
        .iter()
        .map(|scope| format!("<li>{}</li>", escape_html(scope.as_str())))
        .collect::<String>();
    let workspace_name = escape_html(&context.workspace.workspace.name);
    let workspace_role = escape_html(context.workspace.role.as_str());
    let notice = message
        .map(|message| {
            format!(
                r#"<section class="notice"><strong>{}</strong></section>"#,
                escape_html(message)
            )
        })
        .unwrap_or_default();
    format!(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Authorize MCP client</title>
<style>
:root {{
  color-scheme: dark;
  --canvas: oklch(17% 0.012 170);
  --surface: oklch(24% 0.014 170);
  --surface-raised: oklch(30% 0.018 170);
  --line: oklch(39% 0.018 170);
  --ink: oklch(94% 0.01 150);
  --muted: oklch(76% 0.015 155);
  --accent: oklch(78% 0.09 174);
  --signal: oklch(78% 0.08 48);
  --danger: oklch(66% 0.18 28);
  --danger-hover: oklch(60% 0.18 28);
}}
* {{ box-sizing: border-box; }}
body {{
  margin: 0;
  min-height: 100vh;
  background: var(--canvas);
  color: var(--ink);
  font-family: Inter, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
}}
main {{
  width: min(760px, calc(100% - 32px));
  margin: 0 auto;
  padding: 56px 0;
}}
h1 {{ margin: 0 0 10px; font-size: 1.75rem; line-height: 1.15; letter-spacing: 0; }}
h2 {{ margin: 0 0 14px; font-size: 1.05rem; letter-spacing: 0; }}
p {{ color: var(--muted); line-height: 1.55; max-width: 68ch; }}
.panel {{
  border: 1px solid var(--line);
  background: var(--surface);
  border-radius: 8px;
  padding: 24px;
  margin-top: 24px;
}}
.notice {{
  border: 1px solid color-mix(in oklch, var(--accent) 50%, var(--line));
  background: oklch(27% 0.03 170);
  border-radius: 8px;
  padding: 16px;
  margin-top: 24px;
}}
ul {{ margin: 12px 0 0; padding-left: 20px; color: var(--muted); line-height: 1.6; }}
form {{ display: grid; gap: 14px; margin-top: 18px; max-width: 560px; }}
label {{ font-size: 0.8125rem; font-weight: 620; color: var(--muted); }}
.workspace-fixed {{
  width: 100%;
  min-height: 42px;
  border: 1px solid var(--line);
  background: var(--surface-raised);
  color: var(--ink);
  border-radius: 6px;
  padding: 10px 12px;
  margin-top: 8px;
}}
button:focus-visible {{
  outline: 2px solid var(--signal);
  outline-offset: 2px;
}}
.actions {{ display: flex; flex-wrap: wrap; gap: 10px; align-items: center; }}
button {{
  display: inline-block;
  border: 0;
  border-radius: 6px;
  background: var(--accent);
  color: var(--canvas);
  padding: 10px 16px;
  font-weight: 700;
  cursor: pointer;
}}
button:hover {{ background: oklch(72% 0.09 174); }}
.danger-button {{ background: var(--danger); color: var(--ink); }}
.danger-button:hover {{ background: var(--danger-hover); }}
</style>
</head>
<body>
<main>
<h1>Authorize {client_name}</h1>
<p>This MCP client will use your Proofplane workspace. The client receives only the approved workspace permissions below.</p>
{notice}
<section class="panel" aria-labelledby="requested-access">
<h2 id="requested-access">Requested access</h2>
<ul>{scopes}</ul>
</section>
<section class="panel" aria-labelledby="workspace-context">
<h2 id="workspace-context">Workspace</h2>
<form method="post" action="/oauth/consent">
<input type="hidden" name="request_id" value="{request_id}">
<div class="workspace-fixed">{workspace_name} - {workspace_role}</div>
<div class="actions">
<button type="submit" name="decision" value="approve">Approve</button>
<button class="danger-button" type="submit" name="decision" value="deny">Deny</button>
</div>
</form>
</section>
</main>
</body>
</html>"#,
        client_name = escape_html(&context.client_name),
        request_id = Uuid::from(request.id),
        workspace_name = workspace_name,
        workspace_role = workspace_role,
    )
}

fn oauth_json_error(status: StatusCode, error: &'static str) -> Response {
    (status, Json(serde_json::json!({ "error": error }))).into_response()
}

fn oauth_html_error() -> Response {
    secure_html("<!doctype html><html><head><meta charset=\"utf-8\"><title>Authorization failed</title></head><body><main><h1>Authorization failed</h1></main></body></html>".to_owned())
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn secure_html(body: String) -> Response {
    let mut response = (StatusCode::OK, Html(body)).into_response();
    let headers = response.headers_mut();
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    response
}

fn redirect(url: Url) -> Response {
    let mut response = StatusCode::SEE_OTHER.into_response();
    match HeaderValue::from_str(url.as_str()) {
        Ok(location) => {
            response.headers_mut().insert(header::LOCATION, location);
            response
        }
        Err(_) => oauth_html_error(),
    }
}

fn endpoint(issuer: &Url, path: &str) -> String {
    issuer
        .join(path)
        .map(|url| url.to_string())
        .unwrap_or_else(|_| issuer.to_string())
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use uuid::Uuid;

    use super::render_consent_page;
    use crate::{
        domain::{
            OAuthAuthorizationRequest, OAuthAuthorizationRequestId, Workspace, WorkspaceId,
            WorkspacePermission, WorkspaceRole, WorkspaceWithRole,
        },
        services::oauth::OAuthConsentContext,
    };

    #[test]
    fn consent_page_renders_fixed_workspace_with_escaped_content() {
        let workspace_id = WorkspaceId::from(Uuid::new_v4());
        let html = render_consent_page(
            OAuthConsentContext {
                request: OAuthAuthorizationRequest {
                    id: OAuthAuthorizationRequestId::from(Uuid::new_v4()),
                    client_id: "client".to_owned(),
                    redirect_uri: "http://127.0.0.1/callback".to_owned(),
                    code_challenge: "challenge".to_owned(),
                    state: "state".to_owned(),
                    resource: "http://127.0.0.1:3002/mcp".to_owned(),
                    scopes: vec![WorkspacePermission::ReadControls],
                    auth0_subject: Some("auth0|user".to_owned()),
                    user_id: None,
                    expires_at: Utc::now(),
                },
                client_name: "<Inspector & Codex>".to_owned(),
                workspace: WorkspaceWithRole {
                    workspace: Workspace {
                        id: workspace_id,
                        slug: None,
                        name: "SOC 2 <Prod>".to_owned(),
                        created_at: Utc::now(),
                    },
                    role: WorkspaceRole::Owner,
                },
            },
            None,
        );

        assert!(html.contains("&lt;Inspector &amp; Codex&gt;"));
        assert!(html.contains("SOC 2 &lt;Prod&gt; - owner"));
    }
}
