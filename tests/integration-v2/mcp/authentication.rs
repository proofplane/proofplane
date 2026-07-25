use axum_test::TestResponse;
use http::{header, Method, StatusCode};
use proofplane::{
    domain::WorkspacePermission,
    mcp::ENDPOINT,
    routes::{
        authentication::AUTHORIZATION_HEADER,
        protected_resource_metadata::PROTECTED_RESOURCE_METADATA_ENDPOINT,
    },
};
use serde_json::{json, Value};

use crate::support::{
    harness::{self, TestApp},
    mcp::McpClient,
    oauth::authorize_agent_connection,
    scenario::ScenarioBuilder,
};

use super::initialize_body;

const CHALLENGE: &str = concat!(
    r#"Bearer realm="proofplane-mcp", "#,
    r#"resource_metadata="https://mcp.proofplane.test/.well-known/oauth-protected-resource/mcp", "#,
    r#"scope="read_evidence write_evidence read_evidence_submissions "#,
    r#"write_evidence_submissions read_controls write_controls manage_auditor_access""#,
);

#[tokio::test]
async fn missing_malformed_and_tampered_bearer_tokens_are_unauthorized() {
    let app = harness::app().await;
    let subject = "auth0|mcp-malformed-token";

    ScenarioBuilder::new(&app)
        .with_user(subject)
        .with_workspace(subject, "Malformed Token")
        .build()
        .await;

    let token =
        authorize_agent_connection(&app, subject, "Claude", &WorkspacePermission::ALL).await;

    // A real token with its last character replaced still parses as a header but
    // fails to decrypt, so it must be rejected exactly like nonsense.
    let mut tampered = token[..token.len() - 1].to_owned();
    tampered.push(if token.ends_with('a') { 'b' } else { 'a' });

    // A `Bearer ` value padded with trailing whitespace is rejected too, but the
    // HTTP parser strips it before the middleware ever sees it, so that case
    // only means something as a unit test over `bearer_token`.
    for (case, authorization) in [
        ("no credentials", None),
        ("wrong scheme", Some("Basic abc".to_owned())),
        ("empty token", Some("Bearer ".to_owned())),
        ("nonsense token", Some("Bearer not-a-token".to_owned())),
        ("tampered token", Some(format!("Bearer {tampered}"))),
    ] {
        let response = initialize(&app, authorization.as_deref()).await;

        assert_eq!(
            response.status_code(),
            StatusCode::UNAUTHORIZED,
            "{case} is rejected"
        );
        assert_eq!(
            response.header(header::WWW_AUTHENTICATE),
            CHALLENGE,
            "{case} is challenged"
        );
    }
}

#[tokio::test]
async fn revoked_connection_token_is_unauthorized() {
    let app = harness::app().await;
    let subject = "auth0|mcp-revoked-connection";

    ScenarioBuilder::new(&app)
        .with_user(subject)
        .with_workspace(subject, "Revoked Connection")
        .build()
        .await;

    let token =
        authorize_agent_connection(&app, subject, "Claude", &WorkspacePermission::ALL).await;

    // The token has to work first, or the assertion below proves nothing.
    McpClient::connect(app.mcp_server(), &token).await;

    let connections = app
        .app_server()
        .get("/agent-connections")
        .add_header(AUTHORIZATION_HEADER, format!("Bearer {subject}"))
        .await;
    connections.assert_status_ok();
    let connection_id = connections.json::<Value>()["connections"][0]["id"]
        .as_str()
        .expect("connection id is a string")
        .to_owned();

    app.app_server()
        .delete(&format!("/agent-connections/{connection_id}"))
        .add_header(AUTHORIZATION_HEADER, format!("Bearer {subject}"))
        .await
        .assert_status(StatusCode::NO_CONTENT);

    let response = initialize(&app, Some(&format!("Bearer {token}"))).await;
    response.assert_status(StatusCode::UNAUTHORIZED);
    assert_eq!(response.header(header::WWW_AUTHENTICATE), CHALLENGE);
}

#[tokio::test]
async fn protected_resource_metadata_and_cors_are_public() {
    let app = harness::app().await;

    let metadata = app
        .mcp_server()
        .get(PROTECTED_RESOURCE_METADATA_ENDPOINT)
        .await;
    metadata.assert_status_ok();
    metadata.assert_json(&json!({
        "resource": "https://mcp.proofplane.test/mcp",
        "authorization_servers": ["https://api.proofplane.test/"],
        "bearer_methods_supported": ["header"],
        "scopes_supported": [
            "read_evidence",
            "write_evidence",
            "read_evidence_submissions",
            "write_evidence_submissions",
            "read_controls",
            "write_controls",
            "manage_auditor_access"
        ]
    }));

    // Browser-based clients such as the MCP inspector discover the server
    // cross-origin, so both the read and its preflight have to be reachable.
    let cross_origin = app
        .mcp_server()
        .get(PROTECTED_RESOURCE_METADATA_ENDPOINT)
        .add_header(header::ORIGIN, "http://localhost:6274")
        .await;
    cross_origin.assert_status_ok();
    cross_origin.assert_header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*");

    let preflight = app
        .mcp_server()
        .method(Method::OPTIONS, PROTECTED_RESOURCE_METADATA_ENDPOINT)
        .add_header(header::ORIGIN, "http://localhost:6274")
        .add_header(header::ACCESS_CONTROL_REQUEST_METHOD, "GET")
        .await;
    preflight.assert_status_ok();
    preflight.assert_header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*");
}

#[tokio::test]
async fn operational_routes_are_public() {
    let app = harness::app().await;

    app.mcp_server().get("/livez").await.assert_status_ok();
    app.mcp_server().get("/readyz").await.assert_status_ok();

    let metrics = app.mcp_server().get("/metrics").await;
    metrics.assert_status_ok();
    assert!(metrics
        .header(header::CONTENT_TYPE)
        .to_str()
        .expect("content type is text")
        .starts_with("text/plain"));
}

async fn initialize(app: &TestApp, authorization: Option<&str>) -> TestResponse {
    let mut request = app
        .mcp_server()
        .post(ENDPOINT)
        .add_header(header::ACCEPT, "application/json, text/event-stream")
        .json(&initialize_body());

    if let Some(authorization) = authorization {
        request = request.add_header(header::AUTHORIZATION, authorization);
    }

    request.await
}
