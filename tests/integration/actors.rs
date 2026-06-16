use axum::http::StatusCode;
use proofplane::domain::WorkspacePermission;
use proofplane::routes::authentication::{ACTOR_ID_HEADER, API_KEY_HEADER, AUTHORIZATION_HEADER};
use serde_json::{json, Value};
use uuid::Uuid;

use super::support::TestApp;

#[tokio::test]
async fn owner_creates_workspace_scoped_actor_with_explicit_permissions() {
    let app = TestApp::start_without_default_auth().await;
    let owner = "auth0|actor-owner";
    let owner_id = app.login(owner).await;
    let workspace = app.create_workspace_as(owner, "Actor workspace").await;
    let workspace_id = workspace_uuid(&workspace);

    let created = create_actor(
        &app,
        owner,
        workspace_id,
        &json!({
            "kind": "service_account",
            "display_name": "CI Robot",
            "permissions": ["read_evidence_requests", "write_evidence_requests"],
        }),
    )
    .await;
    created.assert_status_ok();

    let actor = created.json::<Value>();
    assert_eq!(actor["workspace_id"], workspace_id.to_string());
    assert_eq!(actor["created_by_user_id"], owner_id.to_string());
    assert_eq!(actor["kind"], "service_account");
    assert_eq!(actor["display_name"], "CI Robot");
    assert_eq!(
        actor["permissions"],
        json!(["read_evidence_requests", "write_evidence_requests"])
    );

    let listed = app
        .server()
        .get(&format!("/workspaces/{workspace_id}/actors"))
        .add_header(AUTHORIZATION_HEADER, format!("Bearer {owner}"))
        .await;
    listed.assert_status_ok();
    let actors = listed.json::<Value>();
    let actors = actors.as_array().expect("actor list is an array");
    assert_eq!(actors.len(), 1);
    assert_eq!(actors[0]["id"], actor["id"]);
}

#[tokio::test]
async fn managing_actors_without_owner_or_admin_role_returns_404() {
    let app = TestApp::start_without_default_auth().await;
    let owner = "auth0|actor-owner-2";
    app.login(owner).await;
    let workspace = app.create_workspace_as(owner, "Guarded workspace").await;
    let workspace_id = workspace_uuid(&workspace);

    let intruder = "auth0|actor-intruder";
    app.login(intruder).await;

    let created = create_actor(
        &app,
        intruder,
        workspace_id,
        &json!({
            "kind": "service_account",
            "display_name": "Sneaky",
            "permissions": ["read_controls"],
        }),
    )
    .await;
    assert_eq!(created.status_code(), StatusCode::NOT_FOUND);

    let listed = app
        .server()
        .get(&format!("/workspaces/{workspace_id}/actors"))
        .add_header(AUTHORIZATION_HEADER, format!("Bearer {intruder}"))
        .await;
    assert_eq!(listed.status_code(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn create_actor_rejects_invalid_permission() {
    let app = TestApp::start_without_default_auth().await;
    let owner = "auth0|actor-owner-3";
    app.login(owner).await;
    let workspace = app.create_workspace_as(owner, "Validation workspace").await;
    let workspace_id = workspace_uuid(&workspace);

    let created = create_actor(
        &app,
        owner,
        workspace_id,
        &json!({
            "kind": "service_account",
            "display_name": "Bad perms",
            "permissions": ["read_evidence_requests", "delete_everything"],
        }),
    )
    .await;

    assert_eq!(created.status_code(), StatusCode::BAD_REQUEST);
    assert_eq!(created.json::<Value>()["error"]["code"], "bad_request");
}

#[tokio::test]
async fn credentials_rotate_independently_and_revocation_is_idempotent() {
    let app = TestApp::start_without_default_auth().await;
    let owner = "auth0|actor-owner-4";
    app.login(owner).await;
    let workspace = app.create_workspace_as(owner, "Rotation workspace").await;
    let workspace_id = workspace_uuid(&workspace);
    let actor_id = create_actor_id(&app, owner, workspace_id, WorkspacePermission::ALL).await;

    let first = issue_credential(&app, owner, workspace_id, &actor_id, "first").await;
    first.assert_status_ok();
    let first = first.json::<Value>();
    let second = issue_credential(&app, owner, workspace_id, &actor_id, "second").await;
    second.assert_status_ok();
    let second = second.json::<Value>();
    let first_key = first["api_key"].as_str().expect("raw key is a string");
    let second_key = second["api_key"].as_str().expect("raw key is a string");

    assert_data_access(&app, workspace_id, &actor_id, first_key, StatusCode::OK).await;
    assert_data_access(&app, workspace_id, &actor_id, second_key, StatusCode::OK).await;

    let credential_id = first["id"].as_str().expect("credential id is a string");
    revoke_credential(&app, owner, workspace_id, &actor_id, credential_id)
        .await
        .assert_status(StatusCode::NO_CONTENT);

    // Revoked key fails; the sibling still authenticates.
    assert_data_access(
        &app,
        workspace_id,
        &actor_id,
        first_key,
        StatusCode::UNAUTHORIZED,
    )
    .await;
    assert_data_access(&app, workspace_id, &actor_id, second_key, StatusCode::OK).await;

    // Revoking again is idempotent.
    revoke_credential(&app, owner, workspace_id, &actor_id, credential_id)
        .await
        .assert_status(StatusCode::NO_CONTENT);

    // An unknown key is rejected.
    assert_data_access(
        &app,
        workspace_id,
        &actor_id,
        "proof-test-not-a-real-key",
        StatusCode::UNAUTHORIZED,
    )
    .await;
}

#[tokio::test]
async fn issued_key_is_returned_once_and_stored_only_as_a_hash() {
    let app = TestApp::start_without_default_auth().await;
    let owner = "auth0|actor-owner-5";
    app.login(owner).await;
    let workspace = app.create_workspace_as(owner, "Secrecy workspace").await;
    let workspace_id = workspace_uuid(&workspace);
    let actor_id = create_actor_id(&app, owner, workspace_id, WorkspacePermission::ALL).await;

    let issued = issue_credential(&app, owner, workspace_id, &actor_id, "primary").await;
    issued.assert_status_ok();
    let issued = issued.json::<Value>();
    let raw_key = issued["api_key"].as_str().expect("raw key is a string");
    let credential_id = issued["id"].as_str().expect("credential id is a string");
    assert!(raw_key.starts_with("proof-"));

    let stored = app
        .postgres()
        .get_api_credential(credential_id)
        .await
        .expect("credential reads")
        .expect("credential exists");
    assert!(stored.credential_hash.starts_with("$argon2id$"));
    assert_ne!(stored.credential_hash, raw_key);

    // Listing actors never re-exposes a raw key.
    let listed = app
        .server()
        .get(&format!("/workspaces/{workspace_id}/actors"))
        .add_header(AUTHORIZATION_HEADER, format!("Bearer {owner}"))
        .await;
    listed.assert_status_ok();
    assert!(!listed.text().contains(raw_key));
}

#[tokio::test]
async fn credential_operations_reject_actor_outside_the_path_workspace() {
    let app = TestApp::start_without_default_auth().await;
    let owner = "auth0|actor-owner-6";
    app.login(owner).await;
    let workspace_a = workspace_uuid(&app.create_workspace_as(owner, "Workspace A").await);
    let workspace_b = workspace_uuid(&app.create_workspace_as(owner, "Workspace B").await);
    let actor_id = create_actor_id(&app, owner, workspace_a, WorkspacePermission::ALL).await;

    // Issue under workspace B for an actor that lives in workspace A.
    let issued = issue_credential(&app, owner, workspace_b, &actor_id, "wrong-workspace").await;
    assert_eq!(issued.status_code(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn data_plane_enforces_each_actor_permission() {
    // No default auth headers so the scoped actor keys below are the only
    // credentials on each request.
    let app = TestApp::builder()
        .without_default_auth()
        .workspace("workspace", "Permission workspace")
        .with_default_membership()
        .build()
        .await;
    let workspace_id = app.workspace_id("workspace");

    // Read-only actor: may GET evidence requests, may not create them.
    let (reader_id, reader_key) = app
        .issue_actor(
            workspace_id,
            vec![WorkspacePermission::ReadEvidenceRequests],
        )
        .await;
    assert_data_access(&app, workspace_id, &reader_id, &reader_key, StatusCode::OK).await;
    let denied_write = app
        .server()
        .post(&format!("/workspaces/{workspace_id}/evidence-requests"))
        .add_header(ACTOR_ID_HEADER, &reader_id)
        .add_header(API_KEY_HEADER, &reader_key)
        .json(&evidence_request_body())
        .await;
    assert_eq!(denied_write.status_code(), StatusCode::NOT_FOUND);

    // Evidence-only actor: may read evidence requests, may not read controls.
    let (evidence_id, evidence_key) = app
        .issue_actor(
            workspace_id,
            vec![
                WorkspacePermission::ReadEvidenceRequests,
                WorkspacePermission::WriteEvidenceRequests,
            ],
        )
        .await;
    assert_data_access(
        &app,
        workspace_id,
        &evidence_id,
        &evidence_key,
        StatusCode::OK,
    )
    .await;
    let denied_controls = app
        .server()
        .get(&format!("/workspaces/{workspace_id}/controls"))
        .add_header(ACTOR_ID_HEADER, &evidence_id)
        .add_header(API_KEY_HEADER, &evidence_key)
        .await;
    assert_eq!(denied_controls.status_code(), StatusCode::NOT_FOUND);
}

async fn create_actor(
    app: &TestApp,
    bearer: &str,
    workspace_id: Uuid,
    body: &Value,
) -> axum_test::TestResponse {
    app.server()
        .post(&format!("/workspaces/{workspace_id}/actors"))
        .add_header(AUTHORIZATION_HEADER, format!("Bearer {bearer}"))
        .json(body)
        .await
}

async fn create_actor_id(
    app: &TestApp,
    bearer: &str,
    workspace_id: Uuid,
    permissions: [WorkspacePermission; 6],
) -> String {
    let permissions: Vec<&str> = permissions.iter().map(|p| p.as_str()).collect();
    let response = create_actor(
        app,
        bearer,
        workspace_id,
        &json!({
            "kind": "service_account",
            "display_name": "Data plane actor",
            "permissions": permissions,
        }),
    )
    .await;
    response.assert_status_ok();
    response.json::<Value>()["id"]
        .as_str()
        .expect("actor id is a string")
        .to_owned()
}

async fn issue_credential(
    app: &TestApp,
    bearer: &str,
    workspace_id: Uuid,
    actor_id: &str,
    name: &str,
) -> axum_test::TestResponse {
    app.server()
        .post(&format!(
            "/workspaces/{workspace_id}/actors/{actor_id}/credentials"
        ))
        .add_header(AUTHORIZATION_HEADER, format!("Bearer {bearer}"))
        .json(&json!({ "name": name }))
        .await
}

async fn revoke_credential(
    app: &TestApp,
    bearer: &str,
    workspace_id: Uuid,
    actor_id: &str,
    credential_id: &str,
) -> axum_test::TestResponse {
    app.server()
        .delete(&format!(
            "/workspaces/{workspace_id}/actors/{actor_id}/credentials/{credential_id}"
        ))
        .add_header(AUTHORIZATION_HEADER, format!("Bearer {bearer}"))
        .await
}

async fn assert_data_access(
    app: &TestApp,
    workspace_id: Uuid,
    actor_id: &str,
    api_key: &str,
    expected: StatusCode,
) {
    let response = app
        .server()
        .get(&format!("/workspaces/{workspace_id}/evidence-requests"))
        .add_header(ACTOR_ID_HEADER, actor_id)
        .add_header(API_KEY_HEADER, api_key)
        .await;
    assert_eq!(response.status_code(), expected);
}

fn evidence_request_body() -> Value {
    json!({
        "title": "Denied request",
        "description": "Should be rejected.",
        "collection_instructions": "n/a",
        "cadence": "quarterly",
        "due_at": "2026-05-20T12:00:00Z",
        "schedule_anchor_at": "2026-01-01T00:00:00Z",
        "freshness_window_days": 90,
        "status": "active"
    })
}

fn workspace_uuid(workspace: &Value) -> Uuid {
    Uuid::parse_str(workspace["id"].as_str().expect("workspace id is a string"))
        .expect("workspace id is a UUID")
}
