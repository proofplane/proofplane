use axum::http::StatusCode;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use chrono::{Duration, Utc};
use proofplane::{
    authorization::spicedb::UserWorkspacePermission,
    domain::{WorkspaceId, WorkspaceRole},
    repository::OutboxMessage,
    routes::authentication::AUTHORIZATION_HEADER,
    worker::WORKSPACE_MEMBER_ADDED,
};
use serde_json::{json, Value};
use uuid::Uuid;

use super::support::TestApp;

#[tokio::test]
async fn create_workspace_makes_caller_the_owner_in_postgres_and_spicedb() {
    let app = TestApp::start_without_default_auth().await;
    let alice = "auth0|alice-create";
    let alice_id = app.login(alice).await;

    let created = app.create_workspace_as(alice, "Alice Workspace").await;

    assert_eq!(created["role"], "owner");
    let workspace_id = Uuid::parse_str(created["id"].as_str().expect("workspace id is a string"))
        .expect("workspace id is a UUID");

    assert_eq!(
        membership_role(&app, workspace_id, alice_id).await,
        Some("owner".to_owned())
    );
    assert!(can_manage_workspace(&app, workspace_id, alice_id).await);
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
async fn owner_can_add_a_member_who_has_logged_in() {
    let app = TestApp::start_without_default_auth().await;
    let alice = "auth0|alice-add";
    let bob = "auth0|bob-add";
    app.login(alice).await;
    let bob_id = app.login(bob).await;

    let workspace_id = workspace_uuid(&app.create_workspace_as(alice, "Shared Workspace").await);

    let response = add_member(&app, alice, workspace_id, bob_id, "admin").await;
    response.assert_status_ok();

    let bob_list = list_workspaces(&app, bob).await;
    assert_eq!(bob_list.len(), 1);
    assert_eq!(bob_list[0]["role"], "admin");
    assert!(can_manage_members(&app, workspace_id, bob_id).await);
}

#[tokio::test]
async fn managing_members_without_permission_returns_404() {
    let app = TestApp::start_without_default_auth().await;
    let alice = "auth0|alice-deny";
    let carol = "auth0|carol-deny";
    app.login(alice).await;
    let carol_id = app.login(carol).await;
    let workspace_id = workspace_uuid(&app.create_workspace_as(alice, "Private Workspace").await);

    let add = add_member(&app, carol, workspace_id, carol_id, "admin").await;
    assert_eq!(add.status_code(), StatusCode::NOT_FOUND);

    let remove = app
        .server()
        .delete(&format!("/workspaces/{workspace_id}/members/{carol_id}"))
        .add_header(AUTHORIZATION_HEADER, format!("Bearer {carol}"))
        .await;
    assert_eq!(remove.status_code(), StatusCode::NOT_FOUND);

    let unknown_workspace = Uuid::new_v4();
    let add_unknown = add_member(&app, alice, unknown_workspace, carol_id, "admin").await;
    assert_eq!(add_unknown.status_code(), StatusCode::NOT_FOUND);
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

    add_member(&app, alice, workspace_id, bob_id, "owner")
        .await
        .assert_status_ok();
    let removed = remove_member(&app, alice, workspace_id, alice_id).await;
    removed.assert_status_ok();
    assert_eq!(membership_role(&app, workspace_id, alice_id).await, None);
}

#[tokio::test]
async fn adding_a_member_who_never_logged_in_is_rejected() {
    let app = TestApp::start_without_default_auth().await;
    let alice = "auth0|alice-unknown-target";
    app.login(alice).await;
    let workspace_id = workspace_uuid(&app.create_workspace_as(alice, "Workspace").await);

    let response = add_member(&app, alice, workspace_id, Uuid::new_v4(), "admin").await;
    assert_eq!(response.status_code(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn failed_synchronous_spicedb_write_is_reconciled_by_the_worker() {
    let app = TestApp::start_without_default_auth().await;
    let alice = "auth0|alice-reconcile";
    let alice_id = app.login(alice).await;
    let workspace_id =
        workspace_uuid(&app.create_workspace_as(alice, "Reconciled Workspace").await);

    // Simulate the best-effort synchronous SpiceDB write never landing: remove the
    // tuple the route wrote so only Postgres + the outbox row reflect the owner.
    app.spicedb()
        .delete_workspace_user_role(
            WorkspaceId::from(workspace_id),
            &alice_id.to_string(),
            WorkspaceRole::Owner,
        )
        .await
        .expect("owner tuple deletes");
    assert!(!can_manage_workspace(&app, workspace_id, alice_id).await);

    let worker = app.worker_server().await;
    let message = membership_outbox_message(&app, workspace_id).await;
    deliver_twice(&worker, &message).await;

    assert!(
        can_manage_workspace(&app, workspace_id, alice_id).await,
        "worker reconciles the owner tuple from the outbox"
    );
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

async fn add_member(
    app: &TestApp,
    sub: &str,
    workspace_id: Uuid,
    user_id: Uuid,
    role: &str,
) -> axum_test::TestResponse {
    app.server()
        .post(&format!("/workspaces/{workspace_id}/members"))
        .add_header(AUTHORIZATION_HEADER, format!("Bearer {sub}"))
        .json(&json!({ "user_id": user_id, "role": role }))
        .await
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

async fn can_manage_workspace(app: &TestApp, workspace_id: Uuid, user_id: Uuid) -> bool {
    app.spicedb()
        .check_user_workspace_permission(
            WorkspaceId::from(workspace_id),
            &user_id.to_string(),
            UserWorkspacePermission::ManageWorkspace,
        )
        .await
        .expect("manage_workspace check runs")
}

async fn can_manage_members(app: &TestApp, workspace_id: Uuid, user_id: Uuid) -> bool {
    app.spicedb()
        .check_user_workspace_permission(
            WorkspaceId::from(workspace_id),
            &user_id.to_string(),
            UserWorkspacePermission::ManageMembers,
        )
        .await
        .expect("manage_members check runs")
}

async fn membership_outbox_message(app: &TestApp, workspace_id: Uuid) -> OutboxMessage {
    let messages = app
        .postgres()
        .list_due_outbox_messages(Utc::now() + Duration::seconds(1), 50)
        .await
        .expect("outbox messages list");

    messages
        .into_iter()
        .find(|message| {
            message.event_type == WORKSPACE_MEMBER_ADDED
                && message.aggregate_id == workspace_id.to_string()
        })
        .expect("membership outbox message exists")
}

async fn deliver_twice(worker: &axum_test::TestServer, message: &OutboxMessage) {
    let envelope = pubsub_envelope(message);

    for _ in 0..2 {
        worker
            .post("/pubsub/messages")
            .json(&envelope)
            .await
            .assert_status(StatusCode::NO_CONTENT);
    }
}

fn pubsub_envelope(message: &OutboxMessage) -> Value {
    let data = json!({
        "event_type": message.event_type,
        "aggregate_type": message.aggregate_type,
        "aggregate_id": message.aggregate_id,
        "request_id": message.request_id,
        "payload": message.payload,
    });

    json!({
        "message": {
            "messageId": format!("outbox-{}", message.id),
            "data": STANDARD.encode(data.to_string()),
        },
        "deliveryAttempt": 1,
    })
}

fn workspace_uuid(created: &Value) -> Uuid {
    Uuid::parse_str(created["id"].as_str().expect("workspace id is a string"))
        .expect("workspace id is a UUID")
}
