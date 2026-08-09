use axum_test::TestResponse;
use bytes::Bytes;
use http::{header, StatusCode};
use proofplane::{domain::WorkspacePermission, routes::authentication::AUTHORIZATION_HEADER};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::support::{
    harness::{self, TestApp},
    oauth::{
        begin_authorization, complete_upstream_login, consent_agent_connection, register_client,
    },
    scenario::ScenarioBuilder,
};

/// Every dead or replayed consent request renders this one page. Pinning the
/// whole body is what proves nothing about the request leaks into it.
const RECOVERY_PAGE_BODY: &str = concat!(
    r#"<body><header>PROOFPLANE <span>/ CONNECTION APPROVAL</span></header>"#,
    r#"<main><p class="eyebrow">REQUEST ENDED</p>"#,
    r#"<h1>Connection could not be completed</h1>"#,
    r#"<p>Return to your client and start the Proofplane connection again.</p></main></body>"#,
);

#[tokio::test]
async fn consent_cancellation_consumes_request_and_preserves_state() {
    let app = harness::app().await;
    let sub = "auth0|oauth-cancel";

    ScenarioBuilder::new(&app)
        .with_user(sub)
        .with_workspace(sub, "Test Workspace")
        .build()
        .await;

    let client_id = register_client(&app, "Cancel Client").await;
    let csrf = begin_authorization(
        &app,
        &client_id,
        "original-state",
        &WorkspacePermission::ALL,
    )
    .await;
    let request_id = complete_upstream_login(&app, sub, &csrf).await;

    let response = submit_connection_consent_form(&app, &request_id, "cancel").await;
    response.assert_status(StatusCode::SEE_OTHER);
    let location = response.header(header::LOCATION);
    let location = url::Url::parse(location.to_str().expect("location is text"))
        .expect("redirect location is a URL");
    assert_eq!(location.path(), "/callback");
    assert_eq!(
        location
            .query_pairs()
            .collect::<std::collections::HashMap<_, _>>(),
        std::collections::HashMap::from([
            ("error".into(), "access_denied".into()),
            ("state".into(), "original-state".into()),
        ])
    );

    let replay = submit_connection_consent_form(&app, &request_id, "cancel").await;
    replay.assert_status_ok();
    assert_recovery_page(&replay.text(), "cancel replay");
}

#[tokio::test]
async fn consent_approval_authorizes_once_and_replay_is_generic() {
    let app = harness::app().await;
    let sub = "auth0|oauth-approve";

    ScenarioBuilder::new(&app)
        .with_user(sub)
        .with_workspace(sub, "Test Workspace")
        .build()
        .await;

    let client_id = register_client(&app, "Approval Client").await;
    let csrf = begin_authorization(
        &app,
        &client_id,
        "original-state",
        &WorkspacePermission::ALL,
    )
    .await;
    let request_id = complete_upstream_login(&app, sub, &csrf).await;

    let response = submit_connection_consent_form(&app, &request_id, "approve").await;
    response.assert_status(StatusCode::SEE_OTHER);
    let location_header = response.header(header::LOCATION);
    let location = url::Url::parse(location_header.to_str().expect("location is text"))
        .expect("approval redirect is a URL");
    assert!(location.query_pairs().any(|(key, _)| key == "code"));
    assert!(location
        .query_pairs()
        .any(|(key, value)| key == "state" && value == "original-state"));

    let response = app
        .app_server()
        .get("/agent-connections")
        .add_header(AUTHORIZATION_HEADER, format!("Bearer {sub}"))
        .await;

    let body = response.json::<Value>();
    let connections = body["connections"]
        .as_array()
        .expect("connections is an array");

    assert_eq!(connections.len(), 1);
    assert_eq!(connections[0]["client_name"], "Approval Client");

    let replay = submit_connection_consent_form(&app, &request_id, "approve").await;
    replay.assert_status_ok();
    assert_recovery_page(&replay.text(), "approve replay");
}

#[tokio::test]
async fn unrecognized_consent_submissions_reveal_only_recovery_guidance() {
    let app = harness::app().await;

    // No request was ever created for this one, so the decision sent against it
    // should make no difference to what comes back.
    let unknown = Uuid::new_v4().to_string();

    for (request_id, decision) in [
        ("not-a-uuid", "approve"),
        (unknown.as_str(), "approve"),
        (unknown.as_str(), "cancel"),
        (unknown.as_str(), "bogus"),
    ] {
        let response = submit_connection_consent_form(&app, request_id, decision).await;

        response.assert_status_ok();
        assert_recovery_page(&response.text(), &format!("{decision} on {request_id}"));
    }
}

#[tokio::test]
async fn authorization_server_metadata_advertises_dcr_alongside_cimd() {
    let app = harness::app().await;

    let response = app
        .app_server()
        .get("/.well-known/oauth-authorization-server")
        .await;
    response.assert_status_ok();

    assert_eq!(
        response.json::<Value>(),
        json!({
            "issuer": "https://api.proofplane.test/",
            "authorization_endpoint": "https://api.proofplane.test/oauth/authorize",
            "token_endpoint": "https://api.proofplane.test/oauth/token",
            "registration_endpoint": "https://api.proofplane.test/oauth/register",
            "response_types_supported": ["code"],
            "grant_types_supported": ["authorization_code"],
            "code_challenge_methods_supported": ["S256"],
            "token_endpoint_auth_methods_supported": ["none"],
            "client_id_metadata_document_supported": true,
            "scopes_supported": [
                "read_evidence",
                "write_evidence",
                "read_evidence_submissions",
                "write_evidence_submissions",
                "read_controls",
                "write_controls",
                "manage_auditor_access",
            ],
        })
    );

    // The advertised registration endpoint has to actually mint a signed,
    // self-describing client id, or the advertisement is a lie.
    let client_id = register_client(&app, "Codex CLI").await;
    assert!(
        client_id.starts_with("ppcli.v1."),
        "registration minted {client_id}, which is not a signed client id"
    );
}

#[tokio::test]
async fn dynamic_registration_is_deterministic_and_drives_authorize() {
    let app = harness::app().await;

    let response = app
        .app_server()
        .post("/oauth/register")
        .json(&json!({
            "client_name": "Codex CLI",
            "redirect_uris": ["http://127.0.0.1:1455/callback"],
        }))
        .await;
    response.assert_status(StatusCode::CREATED);

    let first = response.json::<Value>()["client_id"]
        .as_str()
        .expect("client_id is a string")
        .to_owned();

    let response = app
        .app_server()
        .post("/oauth/register")
        .json(&json!({
            "client_name": "Codex CLI",
            "redirect_uris": ["http://127.0.0.1:1456/callback"],
        }))
        .await;
    response.assert_status(StatusCode::CREATED);

    let second = response.json::<Value>()["client_id"]
        .as_str()
        .expect("client_id is a string")
        .to_owned();

    assert_eq!(
        first, second,
        "same client with a different ephemeral port must yield the same client_id"
    );

    // A third port, registered under neither of the above: the id resolves
    // offline and loopback matching ignores the port, so this still gets as far
    // as the redirect to Auth0.
    let authorize = app
        .app_server()
        .get("/oauth/authorize")
        .add_query_param("response_type", "code")
        .add_query_param("client_id", &first)
        .add_query_param("redirect_uri", "http://127.0.0.1:52731/callback")
        .add_query_param("code_challenge", "challenge")
        .add_query_param("code_challenge_method", "S256")
        .add_query_param("resource", "https://mcp.proofplane.test/mcp")
        .add_query_param("scope", "read_controls")
        .await;
    authorize.assert_status(StatusCode::SEE_OTHER);
}

#[tokio::test]
async fn authorization_codes_are_one_shot_for_pkce_mismatch_success_and_replay() {
    const REDIRECT_URI: &str = "http://127.0.0.1:1455/callback";
    const CODE_VERIFIER: &str = "integration-v2-code-verifier";

    let app = harness::app().await;
    let sub = "auth0|oauth-code-once";
    ScenarioBuilder::new(&app)
        .with_user(sub)
        .with_workspace(sub, "Test Workspace")
        .build()
        .await;

    let mismatched = consent_agent_connection(
        &app,
        sub,
        "PKCE Mismatch Client",
        &[WorkspacePermission::ReadControls],
    )
    .await;
    let mismatch = app
        .app_server()
        .post("/oauth/token")
        .add_header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .bytes(Bytes::from(format!(
            "grant_type=authorization_code&client_id={}&redirect_uri={REDIRECT_URI}\
             &code={}&code_verifier=wrong-verifier",
            mismatched.client_id, mismatched.code,
        )))
        .await;
    mismatch.assert_status(StatusCode::BAD_REQUEST);
    assert_eq!(mismatch.json::<Value>(), json!({"error": "invalid_grant"}));

    let consumed_after_mismatch = app
        .app_server()
        .post("/oauth/token")
        .add_header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .bytes(Bytes::from(format!(
            "grant_type=authorization_code&client_id={}&redirect_uri={REDIRECT_URI}\
             &code={}&code_verifier={CODE_VERIFIER}",
            mismatched.client_id, mismatched.code,
        )))
        .await;
    consumed_after_mismatch.assert_status(StatusCode::BAD_REQUEST);
    assert_eq!(
        consumed_after_mismatch.json::<Value>(),
        json!({"error": "invalid_grant"})
    );

    let valid = consent_agent_connection(
        &app,
        sub,
        "PKCE Replay Client",
        &[WorkspacePermission::ReadControls],
    )
    .await;
    let first = app
        .app_server()
        .post("/oauth/token")
        .add_header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .bytes(Bytes::from(format!(
            "grant_type=authorization_code&client_id={}&redirect_uri={REDIRECT_URI}\
             &code={}&code_verifier={CODE_VERIFIER}",
            valid.client_id, valid.code,
        )))
        .await;
    first.assert_status_ok();
    assert!(first.json::<Value>()["access_token"].is_string());

    let replay = app
        .app_server()
        .post("/oauth/token")
        .add_header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .bytes(Bytes::from(format!(
            "grant_type=authorization_code&client_id={}&redirect_uri={REDIRECT_URI}\
             &code={}&code_verifier={CODE_VERIFIER}",
            valid.client_id, valid.code,
        )))
        .await;
    replay.assert_status(StatusCode::BAD_REQUEST);
    assert_eq!(replay.json::<Value>(), json!({"error": "invalid_grant"}));
}

// This simulates granting the agent access in the web UI.
async fn submit_connection_consent_form(
    app: &TestApp,
    request_id: &str,
    decision: &str,
) -> TestResponse {
    app.app_server()
        .post("/oauth/consent")
        .add_header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .bytes(Bytes::from(format!(
            "request_id={request_id}&decision={decision}",
        )))
        .await
}

/// Asserts the page is exactly the recovery page, styling aside. Comparing the
/// whole body is deliberate: it says what the page *is*, so no request detail can
/// appear in it without failing here.
#[track_caller]
fn assert_recovery_page(html: &str, case: &str) {
    const CLOSING: &str = "</body>";

    let start = html.find("<body>").expect("page has a body element");
    let end = html.find(CLOSING).expect("page closes its body element") + CLOSING.len();

    assert_eq!(&html[start..end], RECOVERY_PAGE_BODY, "{case}");
}
