use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use axum::http::{header, StatusCode};
use axum_test::TestServer;
use chrono::Utc;
use proofplane::{
    authentication::auth0_redirect_token::{
        ConsentResultClaims, ConsentTransactionClaims, RedirectTokenCodec, RedirectTokenError,
    },
    routes::agent_connection_consent::{self, AgentConnectionConsentState, ConsentResultSigner},
    services::agent_connections::{
        AgentConnectionService, ConsumeContinuationOutcome, ConsumeContinuationPayload,
    },
};
use secrecy::SecretString;
use url::Url;
use uuid::Uuid;

use super::support::TestApp;

const SECRET: &str = "integration-action-shared-secret-001";
const SUBJECT: &str = "auth0|consent-user";
const CLIENT_ID: &str = "consent-client";
const RESOURCE: &str = "https://mcp.proofplane.test/mcp";
const ISSUER: &str = "https://tenant.auth0.com/";
const CONSENT_URL: &str = "https://api.proofplane.test/agent-connections/consent";

#[tokio::test]
async fn page_escapes_content_and_approval_creates_single_use_continuation() {
    let (app, workspace_id) = fixture("Workspace <unsafe>").await;
    let codec = Arc::new(make_codec());
    let server = consent_server(&app, codec.clone(), codec.clone());
    let token = transaction_token(&codec, "Client <unsafe>");

    let page = server
        .get("/agent-connections/consent")
        .add_query_param("session_token", &token)
        .add_query_param("state", "auth0-opaque-state")
        .await;
    page.assert_status_ok();
    page.assert_header(header::CACHE_CONTROL, "no-store, max-age=0");
    let html = page.text();
    assert!(html.contains("Client &lt;unsafe&gt;"));
    assert!(html.contains("Workspace &lt;unsafe&gt;"));
    assert!(!html.contains("<unsafe>"));

    let approved = post_form(
        &server,
        &token,
        "auth0-opaque-state",
        "approve",
        Some(&workspace_id.to_string()),
    )
    .await;
    approved.assert_status(StatusCode::SEE_OTHER);
    let location_header = approved.header(header::LOCATION);
    let location = location_header.to_str().unwrap();
    let location = Url::parse(location).unwrap();
    assert_eq!(location.path(), "/continue");
    assert_eq!(
        location
            .query_pairs()
            .find(|(key, _)| key == "state")
            .unwrap()
            .1,
        "auth0-opaque-state"
    );
    let result_token = location
        .query_pairs()
        .find(|(key, _)| key == "session_token")
        .unwrap()
        .1
        .into_owned();
    let result = codec
        .verify_result(&result_token, Utc::now().timestamp())
        .expect("result verifies");
    assert_eq!(result.state, "auth0-opaque-state");
    let service = AgentConnectionService::new(app.postgres_arc());
    let consumed = service
        .consume_continuation(ConsumeContinuationPayload {
            continuation_token: result.continuation_token.unwrap(),
            nonce: result.nonce.unwrap(),
        })
        .await
        .expect("continuation request succeeds");
    assert!(matches!(consumed, ConsumeContinuationOutcome::Approved(_)));

    let replay = post_form(
        &server,
        &token,
        "auth0-opaque-state",
        "approve",
        Some(&workspace_id.to_string()),
    )
    .await;
    replay.assert_status_bad_request();
}

#[tokio::test]
async fn denial_and_invalid_browser_requests_create_no_pending_record() {
    let (app, workspace_id) = fixture("Workspace").await;
    let codec = Arc::new(make_codec());
    let server = consent_server(&app, codec.clone(), codec.clone());
    let token = transaction_token(&codec, "Client");

    let denied = post_form(
        &server,
        &token,
        "auth0-state",
        "deny",
        Some(&workspace_id.to_string()),
    )
    .await;
    denied.assert_status(StatusCode::SEE_OTHER);
    let denied_location = Url::parse(denied.header(header::LOCATION).to_str().unwrap()).unwrap();
    let denied_token = denied_location
        .query_pairs()
        .find(|(key, _)| key == "session_token")
        .unwrap()
        .1
        .into_owned();
    let denied_result = codec
        .verify_result(&denied_token, Utc::now().timestamp())
        .expect("denied result verifies");
    assert_eq!(
        denied_result.decision,
        proofplane::authentication::auth0_redirect_token::ConsentDecision::Denied
    );
    assert!(denied_result.continuation_token.is_none());
    assert_eq!(pending_count(&app).await, 0);

    server
        .get("/agent-connections/consent")
        .await
        .assert_status_bad_request();
    let mut tampered = token.clone();
    let replacement = if tampered.ends_with('A') { "B" } else { "A" };
    tampered.replace_range(tampered.len() - 1.., replacement);
    server
        .get("/agent-connections/consent")
        .add_query_param("session_token", tampered)
        .add_query_param("state", "auth0-state")
        .await
        .assert_status_bad_request();
    assert_eq!(pending_count(&app).await, 0);
}

#[tokio::test]
async fn expired_wrong_client_resource_subject_and_scopes_are_unavailable() {
    let (app, _) = fixture("Workspace").await;
    let codec = Arc::new(make_codec());
    let server = consent_server(&app, codec.clone(), codec.clone());
    let now = Utc::now().timestamp();
    let mut cases = Vec::new();

    let mut expired = transaction_claims("Client");
    expired.iat = now - 301;
    expired.exp = now - 1;
    cases.push(expired);

    let mut wrong_client = transaction_claims("Client");
    wrong_client.client_id = "other-client".to_owned();
    cases.push(wrong_client);

    let mut wrong_resource = transaction_claims("Client");
    wrong_resource.resource = "https://other.example/mcp".to_owned();
    cases.push(wrong_resource);

    let mut wrong_subject = transaction_claims("Client");
    wrong_subject.sub = "auth0|unknown".to_owned();
    cases.push(wrong_subject);

    let mut invalid_scopes = transaction_claims("Client");
    invalid_scopes.scopes = vec!["admin".to_owned()];
    cases.push(invalid_scopes);

    for claims in cases {
        let token = codec.sign_transaction(claims).unwrap();
        server
            .get("/agent-connections/consent")
            .add_query_param("session_token", token)
            .add_query_param("state", "auth0-state")
            .await
            .assert_status_bad_request();
    }
    assert_eq!(pending_count(&app).await, 0);
}

#[tokio::test]
async fn removed_membership_and_result_signing_failure_cannot_leave_pending_records() {
    let (app, workspace_id) = fixture("Workspace").await;
    let codec = Arc::new(make_codec());
    let server = consent_server(&app, codec.clone(), codec.clone());
    let token = transaction_token(&codec, "Client");

    app.postgres()
        .get()
        .await
        .unwrap()
        .execute(
            "DELETE FROM workspace_memberships WHERE workspace_id = $1",
            &[&workspace_id],
        )
        .await
        .unwrap();
    post_form(
        &server,
        &token,
        "auth0-state",
        "approve",
        Some(&workspace_id.to_string()),
    )
    .await
    .assert_status_bad_request();
    assert_eq!(pending_count(&app).await, 0);

    let (app, workspace_id) = fixture("Workspace").await;
    let codec = Arc::new(make_codec());
    let failing = consent_server(&app, codec.clone(), Arc::new(FailingSigner));
    post_form(
        &failing,
        &transaction_token(&codec, "Client"),
        "auth0-state",
        "approve",
        Some(&workspace_id.to_string()),
    )
    .await
    .assert_status_bad_request();
    assert_eq!(pending_count(&app).await, 0);
}

struct FailingSigner;

impl ConsentResultSigner for FailingSigner {
    fn sign_result(&self, _claims: ConsentResultClaims) -> Result<String, RedirectTokenError> {
        Err(RedirectTokenError::Signing)
    }
}

async fn fixture(workspace_name: &str) -> (TestApp, Uuid) {
    let app = TestApp::start_without_default_auth().await;
    app.login(SUBJECT).await;
    let workspace = app.create_workspace_as(SUBJECT, workspace_name).await;
    let workspace_id = Uuid::parse_str(workspace["id"].as_str().unwrap()).unwrap();
    (app, workspace_id)
}

fn make_codec() -> RedirectTokenCodec {
    RedirectTokenCodec::new(SecretString::from(SECRET), ISSUER, CONSENT_URL)
}

fn consent_server(
    app: &TestApp,
    codec: Arc<RedirectTokenCodec>,
    signer: Arc<dyn ConsentResultSigner>,
) -> TestServer {
    TestServer::new(agent_connection_consent::router(
        AgentConnectionConsentState {
            service: AgentConnectionService::new(app.postgres_arc()),
            token_codec: codec,
            result_signer: signer,
            resource: RESOURCE.to_owned(),
            allowed_client_ids: HashSet::from([CLIENT_ID.to_owned()]),
            auth0_continue_url: Url::parse("https://tenant.auth0.com/continue").unwrap(),
        },
    ))
}

fn transaction_token(codec: &RedirectTokenCodec, client_name: &str) -> String {
    codec
        .sign_transaction(transaction_claims(client_name))
        .unwrap()
}

fn transaction_claims(client_name: &str) -> ConsentTransactionClaims {
    let now = Utc::now().timestamp();
    ConsentTransactionClaims {
        purpose: String::new(),
        version: 0,
        transaction_id: Uuid::new_v4().to_string(),
        oauth_state: "oauth-client-state".to_owned(),
        client_id: CLIENT_ID.to_owned(),
        client_name: client_name.to_owned(),
        resource: RESOURCE.to_owned(),
        scopes: vec![
            "write_controls".to_owned(),
            "read_evidence_requests".to_owned(),
        ],
        sub: SUBJECT.to_owned(),
        iss: String::new(),
        aud: String::new(),
        iat: now,
        exp: now + 300,
    }
}

async fn post_form(
    server: &TestServer,
    token: &str,
    state: &str,
    decision: &str,
    workspace_id: Option<&str>,
) -> axum_test::TestResponse {
    let mut form = HashMap::from([
        ("session_token", token),
        ("state", state),
        ("decision", decision),
    ]);
    if let Some(workspace_id) = workspace_id {
        form.insert("workspace_id", workspace_id);
    }
    server.post("/agent-connections/consent").form(&form).await
}

async fn pending_count(app: &TestApp) -> i64 {
    app.postgres()
        .get()
        .await
        .unwrap()
        .query_one(
            "SELECT count(*) FROM agent_connections WHERE status = 'pending'",
            &[],
        )
        .await
        .unwrap()
        .get(0)
}
