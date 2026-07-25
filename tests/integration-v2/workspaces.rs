use http::{header, Method, StatusCode};
use proofplane::routes::{
    authentication::AUTHORIZATION_HEADER, request_context::REQUEST_ID_HEADER,
};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::support::{
    auth::assert_unauthorized, harness, id::workspace_uuid, scenario::ScenarioBuilder,
};

#[tokio::test]
async fn create_workspace_makes_caller_the_owner() {
    let app = harness::app().await;
    let alice = "auth0|alice-create";

    let response = app
        .app_server()
        .post("/workspace")
        .add_header(AUTHORIZATION_HEADER, format!("Bearer {alice}"))
        .json(&json!({ "name": "Alice Workspace" }))
        .await;
    response.assert_status_ok();
    let created: Value = response.json();
    assert_eq!(created["role"], "owner");

    let membership = app
        .app_server()
        .get("/workspace")
        .add_header(AUTHORIZATION_HEADER, format!("Bearer {alice}"))
        .await;
    membership.assert_status_ok();
    let membership: Value = membership.json();

    assert_eq!(membership["role"], "owner");
    assert_eq!(membership["id"], created["id"]);
}

#[tokio::test]
async fn create_workspace_requires_authentication() {
    let app = harness::app().await;

    let missing = app
        .app_server()
        .post("/workspace")
        .json(&json!({ "name": "x" }))
        .await;
    assert_unauthorized(&missing.json(), missing.status_code());

    let invalid = app
        .app_server()
        .post("/workspace")
        .add_header(AUTHORIZATION_HEADER, "Bearer invalid")
        .json(&json!({ "name": "x" }))
        .await;
    assert_unauthorized(&invalid.json(), invalid.status_code());
}

#[tokio::test]
async fn workspace_preflight_does_not_require_authentication() {
    let app = harness::app().await;

    let response = app
        .app_server()
        .method(Method::OPTIONS, "/workspace")
        .add_header(header::ORIGIN.as_str(), "http://127.0.0.1:5173")
        .add_header(header::ACCESS_CONTROL_REQUEST_METHOD.as_str(), "GET")
        .add_header(
            header::ACCESS_CONTROL_REQUEST_HEADERS.as_str(),
            "authorization",
        )
        .await;

    assert_eq!(response.status_code(), StatusCode::OK);
    assert_eq!(response.header("access-control-allow-origin"), "*");
}

#[tokio::test]
async fn get_workspace_without_membership_returns_404() {
    let app = harness::app().await;
    let alice = "auth0|alice-no-workspace";
    app.login(alice).await;

    app.app_server()
        .get("/workspace")
        .add_header(AUTHORIZATION_HEADER, format!("Bearer {alice}"))
        .await
        .assert_status_not_found();
}

#[tokio::test]
async fn managing_members_without_permissions_returns_404() {
    let app = harness::app().await;

    let alice = "auth0|alice-deny";
    let carol = "auth0|carol-deny";

    let test_scenario = ScenarioBuilder::new(&app)
        .with_user(alice)
        .with_user(carol)
        .with_workspace(alice, "Private Workspace")
        .build()
        .await;

    let carol_id = test_scenario.user(carol).id;

    // A non-member cannot manage members.
    let remove = app
        .app_server()
        .delete(&format!("/workspace/members/{carol_id}"))
        .add_header(AUTHORIZATION_HEADER, format!("Bearer {carol}"))
        .await;
    assert_eq!(remove.status_code(), StatusCode::NOT_FOUND);

    // A member that's not part of the workspace has no permissions in it.
    let remove = app
        .app_server()
        .delete(&format!("/workspace/members/{carol_id}"))
        .add_header(AUTHORIZATION_HEADER, format!("Bearer {alice}"))
        .await;
    assert_eq!(remove.status_code(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn cannot_remove_the_last_owner() {
    let app = harness::app().await;

    let alice = "auth0|alice-lastowner";
    let bob = "auth0|bob-lastowner"; // Technically not used yet.

    let test_scenario = ScenarioBuilder::new(&app)
        .with_user(alice)
        .with_user(bob)
        .with_workspace(alice, "Owned Workspace")
        .build()
        .await;

    let alice_id = test_scenario.user(alice).id;

    let rejected = app
        .app_server()
        .delete(&format!("/workspace/members/{alice_id}"))
        .add_header(AUTHORIZATION_HEADER, format!("Bearer {alice}"))
        .await;
    assert_eq!(rejected.status_code(), StatusCode::CONFLICT);

    // TODO: There's currently no API for adding members to workspaces. Once
    // we add that, we should add a test here that does this which is copy-pasted
    // from the older integration tests.

    // insert_membership(&app, workspace_id, bob_id, "owner").await;
    // let removed = remove_member(&app, alice, workspace_id, alice_id).await;
    // removed.assert_status_ok();
    // assert_eq!(membership_role(&app, workspace_id, alice_id).await, None);
}

#[tokio::test]
async fn creating_a_second_workspace_returns_single_workspace_conflict() {
    let app = harness::app().await;
    let alice = "auth0|alice-slug";

    ScenarioBuilder::new(&app).with_user(alice).build().await;

    app.app_server()
        .post("/workspace")
        .add_header(AUTHORIZATION_HEADER, format!("Bearer {alice}"))
        .json(&json!({ "name": "First Workspace", "slug": "acme" }))
        .await
        .assert_status_ok();

    let conflict = app
        .app_server()
        .post("/workspace")
        .add_header(AUTHORIZATION_HEADER, format!("Bearer {alice}"))
        .json(&json!({ "name": "Second Workspace", "slug": "other" }))
        .await;
    assert_eq!(conflict.status_code(), StatusCode::CONFLICT);

    let body = conflict.json::<Value>();
    assert_eq!(body["error"]["code"], "user_already_has_workspace");
    assert_eq!(
        body["error"]["message"],
        "the user already belongs to a workspace"
    );
}

#[tokio::test]
async fn workspace_mutations_emit_success_audit_logs_after_commit() {
    let app = harness::app().await;
    let alice_sub = String::from("auth0|alice-workspace-audit");
    let bob_sub = String::from("auth0|bob-workspace-audit");

    let test_scenario = ScenarioBuilder::new(&app)
        .with_user(alice_sub.as_str())
        .with_user(bob_sub.as_str())
        .build()
        .await;

    let alice = test_scenario.user(alice_sub.as_str());

    let (created_response, created_logs) = app
        .capture_audit_logs(async |request_id| {
            app.app_server()
                .post("/workspace")
                .add_header(AUTHORIZATION_HEADER, format!("Bearer {alice_sub}"))
                .add_header(REQUEST_ID_HEADER, request_id.to_string())
                .json(&json!({ "name": "Audited Workspace", "slug": "audited-workspace" }))
                .await
        })
        .await;
    created_response.assert_status_ok();
    let created = created_response.json::<Value>();
    let workspace_id = workspace_uuid(&created);

    assert_eq!(created_logs.len(), 2);
    assert_audit_event(
        &created_logs,
        "workspace.created",
        alice.id,
        workspace_id,
        "workspace",
        workspace_id,
    );
    assert_audit_event(
        &created_logs,
        "workspace.member_added",
        alice.id,
        workspace_id,
        "user",
        alice.id,
    );

    let (conflict, conflict_logs) = app
        .capture_audit_logs(async |request_id| {
            app.app_server()
                .post("/workspace")
                .add_header(AUTHORIZATION_HEADER, format!("Bearer {alice_sub}"))
                .add_header(REQUEST_ID_HEADER, request_id.to_string())
                .json(&json!({ "name": "Duplicate Workspace", "slug": "audited-workspace" }))
                .await
        })
        .await;
    assert_eq!(conflict.status_code(), StatusCode::CONFLICT);
    assert!(conflict_logs.is_empty());

    // TODO: There's currently no API for adding members to workspaces. Once
    // we add that, we should add a test here that does this which is copy-pasted
    // from the older integration tests.

    // insert_membership(&app, workspace_id, bob_id, "admin").await;
    // let (removed, removed_logs) = capture_audit_logs(|request_id| {
    //     remove_member_with_request_id(&app, alice, workspace_id, bob_id, request_id)
    // })
    // .await;
    // removed.assert_status_ok();

    // assert_eq!(removed_logs.len(), 1);
    // assert_audit_event(
    //     &removed_logs,
    //     "workspace.member_removed",
    //     alice_id,
    //     workspace_id,
    //     "user",
    //     bob_id,
    // );
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
