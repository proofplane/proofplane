use axum::http::StatusCode;
use proofplane::domain::WorkspacePermission;
use proofplane::routes::authentication::AUTHORIZATION_HEADER;
use serde_json::Value;

use super::support::{TestApp, INTEGRATION_API_TOKEN_ID};

#[tokio::test]
async fn data_plane_routes_require_valid_opaque_bearer_tokens() {
    let app = TestApp::builder()
        .without_default_auth()
        .workspace("workspace", "Protected workspace")
        .with_default_membership()
        .build()
        .await;
    let workspace_id = app.workspace_id("workspace");
    let path = format!("/workspaces/{workspace_id}/evidence-requests");

    let missing = app.server().get(&path).await;
    assert_unauthorized(&missing.json(), missing.status_code());

    let malformed = app
        .server()
        .get(&path)
        .add_header(AUTHORIZATION_HEADER, "Bearer not-a-token")
        .await;
    assert_unauthorized(&malformed.json(), malformed.status_code());

    let old_headers = app
        .server()
        .get(&path)
        .add_header("x-proofplane-actor-id", INTEGRATION_API_TOKEN_ID)
        .add_header("x-proofplane-api-key", app.api_token())
        .await;
    assert_unauthorized(&old_headers.json(), old_headers.status_code());

    app.server()
        .get(&path)
        .add_header(AUTHORIZATION_HEADER, app.bearer_token())
        .await
        .assert_status_ok();
}

#[tokio::test]
async fn workspace_mismatch_and_missing_permission_return_not_found() {
    let app = TestApp::builder()
        .without_default_auth()
        .workspace("home", "Home workspace")
        .with_default_membership()
        .workspace("other", "Other workspace")
        .without_membership()
        .build()
        .await;
    let home_workspace_id = app.workspace_id("home");
    let other_workspace_id = app.workspace_id("other");
    let read_only = app
        .issue_api_token(
            home_workspace_id,
            vec![WorkspacePermission::ReadEvidenceRequests],
        )
        .await;

    let other = app
        .server()
        .get(&format!(
            "/workspaces/{other_workspace_id}/evidence-requests"
        ))
        .add_header(
            AUTHORIZATION_HEADER,
            format!("Bearer {}", read_only.raw_token),
        )
        .await;
    assert_eq!(other.status_code(), StatusCode::NOT_FOUND);

    let missing_permission = app
        .server()
        .post(&format!(
            "/workspaces/{home_workspace_id}/evidence-requests"
        ))
        .add_header(
            AUTHORIZATION_HEADER,
            format!("Bearer {}", read_only.raw_token),
        )
        .json(&serde_json::json!({
            "title": "Denied write",
            "description": "Denied write",
            "collection_instructions": "Denied write",
            "cadence": "quarterly",
            "due_at": "2026-06-30T17:00:00Z",
            "schedule_anchor_at": "2026-03-31T17:00:00Z",
            "freshness_window_days": 90,
            "status": "active"
        }))
        .await;
    assert_eq!(missing_permission.status_code(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn removed_membership_returns_unauthorized() {
    let app = TestApp::builder()
        .without_default_auth()
        .workspace("workspace", "Membership workspace")
        .with_default_membership()
        .build()
        .await;
    let workspace_id = app.workspace_id("workspace");
    let token = app
        .issue_api_token(
            workspace_id,
            vec![WorkspacePermission::ReadEvidenceRequests],
        )
        .await;

    app.postgres()
        .get()
        .await
        .expect("connection opens")
        .execute(
            "DELETE FROM workspace_memberships WHERE user_id = $1 AND workspace_id = $2",
            &[&uuid::Uuid::from(token.user_id), &workspace_id],
        )
        .await
        .expect("membership deletes");

    let response = app
        .server()
        .get(&format!("/workspaces/{workspace_id}/evidence-requests"))
        .add_header(AUTHORIZATION_HEADER, format!("Bearer {}", token.raw_token))
        .await;
    assert_unauthorized(&response.json(), response.status_code());
}

fn assert_unauthorized(body: &Value, status: StatusCode) {
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["error"]["code"], "unauthorized");
}
