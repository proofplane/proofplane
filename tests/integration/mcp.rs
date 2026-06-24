use axum::http::{header, StatusCode};
use proofplane::{domain::WorkspacePermission, mcp::SESSION_ID_HEADER};
use serde_json::json;
use uuid::Uuid;

use super::support::TestApp;

const MCP: &str = "/mcp";

#[tokio::test]
async fn mcp_reauthenticates_token_state_and_serves_public_operational_routes() {
    let app = TestApp::start_without_default_auth().await;
    let server = app.mcp_server();
    let client = app
        .postgres
        .get()
        .await
        .expect("fixture database connection opens");
    let workspace_id = app.home_workspace_id();

    let initialized = initialize(&server, app.api_token()).await;
    initialized.assert_status_ok();
    assert!(initialized.text().contains("proofplane"));
    let session_id = initialized.header(SESSION_ID_HEADER);

    client
        .execute(
            "UPDATE api_tokens SET revoked_at = now() WHERE id = $1",
            &[&app.api_token_id()],
        )
        .await
        .expect("token revokes");
    let revoked = server
        .delete(MCP)
        .add_header(header::AUTHORIZATION, app.bearer_token())
        .add_header(SESSION_ID_HEADER, session_id)
        .await;
    assert_unauthorized(&revoked);

    let expired = app
        .issue_api_token(workspace_id, WorkspacePermission::ALL.to_vec())
        .await;
    client
        .execute(
            "UPDATE api_tokens SET expires_at = now() - interval '1 second' WHERE id = $1",
            &[&Uuid::from(expired.token_id)],
        )
        .await
        .expect("token expires");
    assert_unauthorized(&initialize(&server, &expired.raw_token).await);

    let removed_member = app
        .issue_api_token(workspace_id, WorkspacePermission::ALL.to_vec())
        .await;
    client
        .execute(
            "DELETE FROM workspace_memberships WHERE workspace_id = $1 AND user_id = $2",
            &[&workspace_id, &Uuid::from(removed_member.user_id)],
        )
        .await
        .expect("membership removes");
    assert_unauthorized(&initialize(&server, &removed_member.raw_token).await);

    server.get("/livez").await.assert_status_ok();
    server.get("/readyz").await.assert_status_ok();
    let metrics = server.get("/metrics").await;
    metrics.assert_status_ok();
    assert!(metrics
        .header(header::CONTENT_TYPE)
        .to_str()
        .expect("content type is text")
        .starts_with("text/plain"));
}

async fn initialize(server: &axum_test::TestServer, raw_token: &str) -> axum_test::TestResponse {
    server
        .post(MCP)
        .add_header(header::AUTHORIZATION, format!("Bearer {raw_token}"))
        .add_header(header::CONTENT_TYPE, "application/json")
        .add_header(header::ACCEPT, "application/json, text/event-stream")
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-03-26",
                "capabilities": {},
                "clientInfo": {"name": "integration-test", "version": "1.0"}
            }
        }))
        .await
}

fn assert_unauthorized(response: &axum_test::TestResponse) {
    response.assert_status(StatusCode::UNAUTHORIZED);
    assert_eq!(
        response.header(header::WWW_AUTHENTICATE),
        "Bearer realm=\"proofplane-mcp\""
    );
}
