use axum::http::StatusCode;
use proofplane::routes::authentication::AUTHORIZATION_HEADER;
use serde_json::{json, Value};
use uuid::Uuid;

use super::support::TestApp;

#[tokio::test]
async fn create_workspace_makes_caller_the_owner() {
    let app = TestApp::start_without_default_auth().await;
    let alice = "auth0|alice-create";
    let alice_id = app.login(alice).await;

    let created = app.create_workspace_as(alice, "Alice Workspace").await;

    assert_eq!(created["role"], "owner");
    let workspace_id = workspace_uuid(&created);
    assert_eq!(
        membership_role(&app, workspace_id, alice_id).await,
        Some("owner".to_owned())
    );
}

#[tokio::test]
async fn create_workspace_requires_authentication() {
    let app = TestApp::start_without_default_auth().await;

    let missing = app
        .server()
        .post("/workspaces")
        .json(&json!({ "name": "x" }))
        .await;
    assert_eq!(missing.status_code(), StatusCode::UNAUTHORIZED);

    let invalid = app
        .server()
        .post("/workspaces")
        .add_header(AUTHORIZATION_HEADER, "Bearer invalid")
        .json(&json!({ "name": "x" }))
        .await;
    assert_eq!(invalid.status_code(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn list_workspaces_returns_only_the_callers_workspaces_with_roles() {
    let app = TestApp::start_without_default_auth().await;
    let alice = "auth0|alice-list";
    let bob = "auth0|bob-list";
    app.login(alice).await;
    app.login(bob).await;

    let alice_workspace = app.create_workspace_as(alice, "Alice Workspace").await["id"]
        .as_str()
        .expect("workspace id is a string")
        .to_owned();
    app.create_workspace_as(bob, "Bob Workspace").await;

    let alice_list = list_workspaces(&app, alice).await;
    assert_eq!(alice_list.len(), 1);
    assert_eq!(alice_list[0]["id"], alice_workspace);
    assert_eq!(alice_list[0]["role"], "owner");

    let bob_list = list_workspaces(&app, bob).await;
    assert_eq!(bob_list.len(), 1);
    assert_eq!(bob_list[0]["role"], "owner");
    assert_ne!(bob_list[0]["id"], alice_workspace);
}

#[tokio::test]
async fn managing_members_without_permission_returns_404() {
    let app = TestApp::start_without_default_auth().await;
    let alice = "auth0|alice-deny";
    let carol = "auth0|carol-deny";
    app.login(alice).await;
    let carol_id = app.login(carol).await;
    let workspace_id = workspace_uuid(&app.create_workspace_as(alice, "Private Workspace").await);

    // A non-member cannot manage members.
    let remove = remove_member(&app, carol, workspace_id, carol_id).await;
    assert_eq!(remove.status_code(), StatusCode::NOT_FOUND);

    // An unknown workspace is indistinguishable from one the caller cannot access.
    let unknown_workspace = Uuid::new_v4();
    let remove_unknown = remove_member(&app, alice, unknown_workspace, carol_id).await;
    assert_eq!(remove_unknown.status_code(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn cannot_remove_the_last_owner() {
    let app = TestApp::start_without_default_auth().await;
    let alice = "auth0|alice-lastowner";
    let bob = "auth0|bob-lastowner";
    let alice_id = app.login(alice).await;
    let bob_id = app.login(bob).await;
    let workspace_id = workspace_uuid(&app.create_workspace_as(alice, "Owned Workspace").await);

    let rejected = remove_member(&app, alice, workspace_id, alice_id).await;
    assert_eq!(rejected.status_code(), StatusCode::CONFLICT);
    assert_eq!(
        membership_role(&app, workspace_id, alice_id).await,
        Some("owner".to_owned())
    );

    insert_membership(&app, workspace_id, bob_id, "owner").await;
    let removed = remove_member(&app, alice, workspace_id, alice_id).await;
    removed.assert_status_ok();
    assert_eq!(membership_role(&app, workspace_id, alice_id).await, None);
}

async fn list_workspaces(app: &TestApp, sub: &str) -> Vec<Value> {
    let response = app
        .server()
        .get("/workspaces")
        .add_header(AUTHORIZATION_HEADER, format!("Bearer {sub}"))
        .await;
    response.assert_status_ok();
    response.json::<Vec<Value>>()
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

async fn remove_member(
    app: &TestApp,
    sub: &str,
    workspace_id: Uuid,
    user_id: Uuid,
) -> axum_test::TestResponse {
    app.server()
        .delete(&format!("/workspaces/{workspace_id}/members/{user_id}"))
        .add_header(AUTHORIZATION_HEADER, format!("Bearer {sub}"))
        .await
}

async fn membership_role(app: &TestApp, workspace_id: Uuid, user_id: Uuid) -> Option<String> {
    let client = app.postgres().get().await.expect("pool client opens");
    let rows = client
        .query(
            "SELECT role FROM workspace_memberships WHERE workspace_id = $1 AND user_id = $2",
            &[&workspace_id, &user_id],
        )
        .await
        .expect("membership query runs");

    rows.into_iter()
        .next()
        .map(|row| row.get::<_, String>("role"))
}

fn workspace_uuid(created: &Value) -> Uuid {
    Uuid::parse_str(created["id"].as_str().expect("workspace id is a string"))
        .expect("workspace id is a UUID")
}
