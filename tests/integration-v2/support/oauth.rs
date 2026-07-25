use axum::http::{header, StatusCode};
use axum_test::TestResponse;
use bytes::Bytes;
use proofplane::{domain::WorkspacePermission, services::oauth::pkce_challenge};
use serde_json::{json, Value};
use url::Url;

use crate::support::harness::TestApp;

const CODE_VERIFIER: &str = "integration-v2-code-verifier";
const REDIRECT_URI: &str = "http://127.0.0.1:1455/callback";

pub struct ConsentedConnection {
    pub client_id: String,
    pub code: String,
}

pub async fn authorize_agent_connection(
    app: &TestApp,
    sub: &str,
    client_name: &str,
    permissions: &[WorkspacePermission],
) -> String {
    let consented = consent_agent_connection(app, sub, client_name, permissions).await;

    redeem(app, &consented).await
}

/// Walks the flow up to consent.
///
/// `client_name` is what makes connections distinct, so give each one its own
/// name. Reusing a name for the same user resolves to the existing connection
/// and never reaches the consent page.
pub async fn consent_agent_connection(
    app: &TestApp,
    sub: &str,
    client_name: &str,
    permissions: &[WorkspacePermission],
) -> ConsentedConnection {
    let client_id = register_client(app, client_name).await;
    let csrf = begin_authorization(app, &client_id, "test-state", permissions).await;
    let request_id = complete_upstream_login(app, sub, &csrf).await;
    let code = approve_consent(app, &request_id).await;

    ConsentedConnection { client_id, code }
}

/// Redeems the authorization code for an access token.
pub async fn redeem(app: &TestApp, consented: &ConsentedConnection) -> String {
    exchange_code(app, &consented.client_id, &consented.code).await
}

pub async fn register_client(app: &TestApp, client_name: &str) -> String {
    let response = app
        .app_server()
        .post("/oauth/register")
        .json(&json!({
            "client_name": client_name,
            "redirect_uris": [REDIRECT_URI],
        }))
        .await;
    response.assert_status(StatusCode::CREATED);

    response.json::<Value>()["client_id"]
        .as_str()
        .expect("client_id is a string")
        .to_owned()
}

/// Starts the authorization request. The redirect to Auth0 carries the CSRF token
/// as its `state`, which is how the callback later finds this request.
pub async fn begin_authorization(
    app: &TestApp,
    client_id: &str,
    state: &str,
    permissions: &[WorkspacePermission],
) -> String {
    let scope = permissions
        .iter()
        .map(|permission| permission.as_str())
        .collect::<Vec<_>>()
        .join(" ");

    let response = app
        .app_server()
        .get("/oauth/authorize")
        .add_query_param("response_type", "code")
        .add_query_param("client_id", client_id)
        .add_query_param("redirect_uri", REDIRECT_URI)
        .add_query_param("code_challenge", pkce_challenge(CODE_VERIFIER))
        .add_query_param("code_challenge_method", "S256")
        .add_query_param("resource", "https://mcp.proofplane.test/mcp")
        .add_query_param("scope", scope)
        .add_query_param("state", state)
        .await;
    response.assert_status(StatusCode::SEE_OTHER);

    redirect_query_param(&response, "state").expect("authorize redirect carries the CSRF token")
}

/// Returns from the upstream tenant. The fake Auth0 echoes `code` back as the
/// access token, so passing the `auth0_sub` here is what picks the identity.
/// The response is the consent page.
pub async fn complete_upstream_login(app: &TestApp, sub: &str, csrf: &str) -> String {
    let response = app
        .app_server()
        .get("/oauth/auth0/callback")
        .add_query_param("code", sub)
        .add_query_param("state", csrf)
        .await;

    request_id(&response.text())
}

/// Approves consent, which creates the agent connection and redirects back to the
/// client with an authorization code.
async fn approve_consent(app: &TestApp, request_id: &str) -> String {
    let response = app
        .app_server()
        .post("/oauth/consent")
        .add_header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .bytes(Bytes::from(format!(
            "request_id={request_id}&decision=approve"
        )))
        .await;
    response.assert_status(StatusCode::SEE_OTHER);

    redirect_query_param(&response, "code")
        .expect("consent approval redirects with an authorization code")
}

async fn exchange_code(app: &TestApp, client_id: &str, code: &str) -> String {
    let response = app
        .app_server()
        .post("/oauth/token")
        .add_header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .bytes(Bytes::from(format!(
            "grant_type=authorization_code&client_id={client_id}&redirect_uri={REDIRECT_URI}\
             &code={code}&code_verifier={CODE_VERIFIER}"
        )))
        .await;
    response.assert_status_ok();

    response.json::<Value>()["access_token"]
        .as_str()
        .expect("access_token is a string")
        .to_owned()
}

/// The consent form posts its request back through a hidden input.
fn request_id(html: &str) -> String {
    const MARKER: &str = r#"name="request_id" value=""#;

    let start = html
        .find(MARKER)
        .map(|index| index + MARKER.len())
        .expect("consent page carries a request id");
    let rest = &html[start..];
    let end = rest.find('"').expect("request id is quoted");

    rest[..end].to_owned()
}

fn redirect_query_param(response: &TestResponse, name: &str) -> Option<String> {
    let location = response.header(header::LOCATION);
    let location = Url::parse(location.to_str().expect("redirect location is text"))
        .expect("redirect location is a URL");

    location
        .query_pairs()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.into_owned())
}
