use axum::http::StatusCode;
use chrono::{Duration as ChronoDuration, Utc};
use proofplane::routes::authentication::AUTHORIZATION_HEADER;
use serde_json::{json, Value};
use uuid::Uuid;

use super::support::TestApp;

#[tokio::test]
async fn workspace_member_issues_lists_and_revokes_own_api_token() {
    let app = TestApp::start_without_default_auth().await;
    let owner = "auth0|api-token-owner";
    let member = "auth0|api-token-member";
    app.login(owner).await;
    let member_id = app.login(member).await;
    let workspace_id = workspace_uuid(&app.create_workspace_as(owner, "API Token Workspace").await);
    insert_membership(&app, workspace_id, member_id, "admin").await;

    let issued = create_token(
        &app,
        member,
        workspace_id,
        json!({
            "name": "CI token",
            "expires_at": future_timestamp(30),
            "permissions": ["write_controls", "read_evidence_requests", "read_controls"],
        }),
    )
    .await;
    issued.assert_status_ok();
    let issued = issued.json::<Value>();
    let token_id = token_uuid(&issued);
    let raw = issued["api_token"]
        .as_str()
        .expect("api_token is returned once");

    assert!(raw.starts_with("v4.public."));
    assert_eq!(issued["name"], "CI token");
    assert_eq!(issued["workspace_id"], workspace_id.to_string());
    assert_eq!(
        issued["permissions"],
        json!(["read_evidence_requests", "read_controls", "write_controls"])
    );

    let stored = app
        .postgres()
        .get_api_token(proofplane::domain::ApiTokenId::from(token_id))
        .await
        .expect("token reads")
        .expect("token exists");
    assert_eq!(Uuid::from(stored.token.id), token_id);
    assert_eq!(stored.token.revoked_at, None);

    let listed = list_tokens(&app, member, workspace_id).await;
    listed.assert_status_ok();
    let list_text = listed.text();
    assert!(!list_text.contains(raw));
    assert!(!list_text.contains("api_token"));
    let listed = serde_json::from_str::<Value>(&list_text).expect("list response parses");
    assert_eq!(listed.as_array().expect("list is an array").len(), 1);
    assert_eq!(listed[0]["id"], token_id.to_string());
    assert_eq!(listed[0]["last_used_at"], Value::Null);

    revoke_token(&app, member, workspace_id, token_id)
        .await
        .assert_status(StatusCode::NO_CONTENT);
    revoke_token(&app, member, workspace_id, token_id)
        .await
        .assert_status(StatusCode::NO_CONTENT);

    let revoked = app
        .postgres()
        .get_api_token(proofplane::domain::ApiTokenId::from(token_id))
        .await
        .expect("revoked token reads")
        .expect("revoked token remains");
    assert!(revoked.token.revoked_at.is_some());
}

#[tokio::test]
async fn far_future_expiration_is_accepted_without_a_maximum_ttl() {
    let app = TestApp::start_without_default_auth().await;
    let owner = "auth0|api-token-future";
    app.login(owner).await;
    let workspace_id = workspace_uuid(&app.create_workspace_as(owner, "Future Workspace").await);

    create_token(
        &app,
        owner,
        workspace_id,
        json!({
            "name": "Long lived",
            "expires_at": (Utc::now() + ChronoDuration::days(365 * 50)).to_rfc3339(),
            "permissions": [],
        }),
    )
    .await
    .assert_status_ok();
}

#[tokio::test]
async fn token_management_does_not_leak_workspace_or_token_ownership() {
    let app = TestApp::start_without_default_auth().await;
    let owner = "auth0|api-token-owner-isolation";
    let other = "auth0|api-token-other";
    app.login(owner).await;
    app.login(other).await;
    let workspace_a = workspace_uuid(&app.create_workspace_as(owner, "Token Workspace A").await);
    let workspace_b = workspace_uuid(&app.create_workspace_as(owner, "Token Workspace B").await);
    let token = create_token(
        &app,
        owner,
        workspace_a,
        json!({
            "name": "Private token",
            "expires_at": future_timestamp(10),
            "permissions": ["read_controls"],
        }),
    )
    .await
    .json::<Value>();
    let token_id = token_uuid(&token);

    assert_eq!(
        list_tokens(&app, other, workspace_a).await.status_code(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        create_token(
            &app,
            other,
            workspace_a,
            json!({
                "name": "Nope",
                "expires_at": future_timestamp(10),
                "permissions": ["read_controls"],
            }),
        )
        .await
        .status_code(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        revoke_token(&app, other, workspace_a, token_id)
            .await
            .status_code(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        revoke_token(&app, owner, workspace_b, token_id)
            .await
            .status_code(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        revoke_token(&app, owner, Uuid::new_v4(), token_id)
            .await
            .status_code(),
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn token_create_rejects_invalid_duplicate_missing_and_past_expiration() {
    let app = TestApp::start_without_default_auth().await;
    let owner = "auth0|api-token-validation";
    app.login(owner).await;
    let workspace_id =
        workspace_uuid(&app.create_workspace_as(owner, "Validation Workspace").await);

    for body in [
        json!({
            "name": "Invalid permission",
            "expires_at": future_timestamp(10),
            "permissions": ["read_controls", "delete_everything"],
        }),
        json!({
            "name": "Duplicate permission",
            "expires_at": future_timestamp(10),
            "permissions": ["read_controls", "read_controls"],
        }),
        json!({
            "name": "Missing expiration",
            "permissions": ["read_controls"],
        }),
        json!({
            "name": "Past expiration",
            "expires_at": (Utc::now() - ChronoDuration::minutes(5)).to_rfc3339(),
            "permissions": ["read_controls"],
        }),
    ] {
        let response = create_token(&app, owner, workspace_id, body).await;
        assert_eq!(response.status_code(), StatusCode::BAD_REQUEST);
        assert_eq!(response.json::<Value>()["error"]["code"], "bad_request");
    }
}

async fn create_token(
    app: &TestApp,
    sub: &str,
    workspace_id: Uuid,
    body: Value,
) -> axum_test::TestResponse {
    app.server()
        .post(&format!("/workspaces/{workspace_id}/api-tokens"))
        .add_header(AUTHORIZATION_HEADER, format!("Bearer {sub}"))
        .json(&body)
        .await
}

async fn list_tokens(app: &TestApp, sub: &str, workspace_id: Uuid) -> axum_test::TestResponse {
    app.server()
        .get(&format!("/workspaces/{workspace_id}/api-tokens"))
        .add_header(AUTHORIZATION_HEADER, format!("Bearer {sub}"))
        .await
}

async fn revoke_token(
    app: &TestApp,
    sub: &str,
    workspace_id: Uuid,
    token_id: Uuid,
) -> axum_test::TestResponse {
    app.server()
        .delete(&format!("/workspaces/{workspace_id}/api-tokens/{token_id}"))
        .add_header(AUTHORIZATION_HEADER, format!("Bearer {sub}"))
        .await
}

async fn insert_membership(app: &TestApp, workspace_id: Uuid, user_id: Uuid, role: &str) {
    let client = app.postgres().get().await.expect("pool client opens");
    client
        .execute(
            "INSERT INTO workspace_memberships (workspace_id, user_id, role) VALUES ($1, $2, $3)",
            &[&workspace_id, &user_id, &role],
        )
        .await
        .expect("membership insert runs");
}

fn workspace_uuid(created: &Value) -> Uuid {
    Uuid::parse_str(created["id"].as_str().expect("workspace id is a string"))
        .expect("workspace id is a UUID")
}

fn token_uuid(created: &Value) -> Uuid {
    Uuid::parse_str(created["id"].as_str().expect("token id is a string"))
        .expect("token id is a UUID")
}

fn future_timestamp(minutes: i64) -> String {
    (Utc::now() + ChronoDuration::minutes(minutes)).to_rfc3339()
}
