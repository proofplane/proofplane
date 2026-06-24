use axum::http::StatusCode;
use proofplane::routes::authentication::AUTHORIZATION_HEADER;
use proofplane::routes::request_context::REQUEST_ID_HEADER;
use serde_json::{json, Value};
use uuid::Uuid;

use super::support::{capture_audit_logs, TestApp};

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

#[tokio::test]
async fn creating_a_workspace_with_a_taken_slug_returns_conflict() {
    let app = TestApp::start_without_default_auth().await;
    let alice = "auth0|alice-slug";
    app.login(alice).await;

    create_workspace_with_slug(&app, alice, "First Workspace", "acme")
        .await
        .assert_status_ok();

    let conflict = create_workspace_with_slug(&app, alice, "Second Workspace", "acme").await;
    assert_eq!(conflict.status_code(), StatusCode::CONFLICT);

    let body = conflict.json::<Value>();
    assert_eq!(body["error"]["code"], "slug_taken");
    assert_eq!(
        body["error"]["message"],
        "a workspace with this slug already exists"
    );
}

#[tokio::test]
async fn workspace_mutations_emit_success_audit_logs_after_commit() {
    let app = TestApp::start_without_default_auth().await;
    let alice = "auth0|alice-workspace-audit";
    let bob = "auth0|bob-workspace-audit";
    let alice_id = app.login(alice).await;
    let bob_id = app.login(bob).await;

    let (created_response, created_logs) = capture_audit_logs(|request_id| {
        create_workspace_with_slug_and_request_id(
            &app,
            alice,
            "Audited Workspace",
            "audited-workspace",
            request_id,
        )
    })
    .await;
    created_response.assert_status_ok();
    let created = created_response.json::<Value>();
    let workspace_id = workspace_uuid(&created);

    assert_eq!(created_logs.len(), 2);
    assert_audit_event(
        &created_logs,
        "workspace.created",
        alice_id,
        workspace_id,
        "workspace",
        workspace_id,
    );
    assert_audit_event(
        &created_logs,
        "workspace.member_added",
        alice_id,
        workspace_id,
        "user",
        alice_id,
    );

    let (conflict, conflict_logs) = capture_audit_logs(|request_id| {
        create_workspace_with_slug_and_request_id(
            &app,
            alice,
            "Duplicate Workspace",
            "audited-workspace",
            request_id,
        )
    })
    .await;
    assert_eq!(conflict.status_code(), StatusCode::CONFLICT);
    assert!(conflict_logs.is_empty());

    insert_membership(&app, workspace_id, bob_id, "admin").await;
    let (removed, removed_logs) = capture_audit_logs(|request_id| {
        remove_member_with_request_id(&app, alice, workspace_id, bob_id, request_id)
    })
    .await;
    removed.assert_status_ok();

    assert_eq!(removed_logs.len(), 1);
    assert_audit_event(
        &removed_logs,
        "workspace.member_removed",
        alice_id,
        workspace_id,
        "user",
        bob_id,
    );
}

async fn create_workspace_with_slug(
    app: &TestApp,
    sub: &str,
    name: &str,
    slug: &str,
) -> axum_test::TestResponse {
    create_workspace_with_slug_request(app, sub, name, slug, None).await
}

async fn create_workspace_with_slug_and_request_id(
    app: &TestApp,
    sub: &str,
    name: &str,
    slug: &str,
    request_id: Uuid,
) -> axum_test::TestResponse {
    create_workspace_with_slug_request(app, sub, name, slug, Some(request_id)).await
}

async fn create_workspace_with_slug_request(
    app: &TestApp,
    sub: &str,
    name: &str,
    slug: &str,
    request_id: Option<Uuid>,
) -> axum_test::TestResponse {
    let mut request = app
        .server()
        .post("/workspaces")
        .add_header(AUTHORIZATION_HEADER, format!("Bearer {sub}"));
    if let Some(request_id) = request_id {
        request = request.add_header(REQUEST_ID_HEADER, request_id.to_string());
    }

    request.json(&json!({ "name": name, "slug": slug })).await
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
    remove_member_request(app, sub, workspace_id, user_id, None).await
}

async fn remove_member_with_request_id(
    app: &TestApp,
    sub: &str,
    workspace_id: Uuid,
    user_id: Uuid,
    request_id: Uuid,
) -> axum_test::TestResponse {
    remove_member_request(app, sub, workspace_id, user_id, Some(request_id)).await
}

async fn remove_member_request(
    app: &TestApp,
    sub: &str,
    workspace_id: Uuid,
    user_id: Uuid,
    request_id: Option<Uuid>,
) -> axum_test::TestResponse {
    let mut request = app
        .server()
        .delete(&format!("/workspaces/{workspace_id}/members/{user_id}"))
        .add_header(AUTHORIZATION_HEADER, format!("Bearer {sub}"));
    if let Some(request_id) = request_id {
        request = request.add_header(REQUEST_ID_HEADER, request_id.to_string());
    }

    request.await
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

fn assert_audit_event(
    records: &[Value],
    event_name: &str,
    actor_user_id: Uuid,
    workspace_id: Uuid,
    object_type: &str,
    object_id: Uuid,
) {
    let record = records
        .iter()
        .find(|record| record["fields"]["event_name"] == event_name)
        .unwrap_or_else(|| panic!("{event_name} audit record exists"));
    let fields = &record["fields"];

    assert_eq!(fields["type"], "audit_log");
    assert_eq!(fields["outcome"], "success");
    assert_eq!(fields["actor_type"], "user");
    assert_eq!(fields["user_id"], actor_user_id.to_string());
    assert_eq!(fields["client_type"], "rest");
    assert_eq!(fields["workspace_id"], workspace_id.to_string());
    assert_eq!(fields["object_type"], object_type);
    assert_eq!(fields["object_id"], object_id.to_string());
    assert!(Uuid::parse_str(fields["request_id"].as_str().expect("request id is set")).is_ok());
}
