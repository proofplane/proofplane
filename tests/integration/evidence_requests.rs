use axum::http::StatusCode;
use serde_json::{json, Value};
use uuid::Uuid;

use super::support::TestApp;

#[tokio::test]
async fn create_returns_the_evidence_request() {
    let app = TestApp::builder()
        .workspace("workspace", "Create workspace")
        .with_default_membership()
        .build()
        .await;
    let workspace_id = app.workspace_id("workspace");
    let body = evidence_request("Access review", "2026-03-31T09:00:00Z", "active");

    let response = app
        .server()
        .post(&collection_path(workspace_id))
        .json(&body)
        .await;

    response.assert_status_ok();
    let created: Value = response.json();
    assert_uuid(&created["id"]);
    assert_eq!(created["workspace_id"], workspace_id.to_string());
    assert_request_matches(&created, &body);
    assert_timestamp(&created["created_at"]);
    assert_timestamp(&created["updated_at"]);
}

#[tokio::test]
async fn create_maps_validation_errors_to_bad_request() {
    let app = TestApp::builder()
        .workspace("workspace", "Invalid create workspace")
        .with_default_membership()
        .build()
        .await;
    let workspace_id = app.workspace_id("workspace");

    let response = app
        .server()
        .post(&collection_path(workspace_id))
        .json(&invalid_evidence_request())
        .await;

    assert_validation_error(&response.json(), response.status_code());
}

#[tokio::test]
async fn list_returns_requests_for_a_workspace_in_due_date_then_title_order() {
    let app = TestApp::builder()
        .workspace("workspace", "List workspace")
        .with_default_membership()
        .workspace("other_workspace", "Other list workspace")
        .without_membership()
        .build()
        .await;
    let workspace_id = app.workspace_id("workspace");
    let other_workspace_id = app.workspace_id("other_workspace");

    app.create_evidence_request(
        workspace_id,
        &evidence_request("Zulu request", "2026-04-01T00:00:00Z", "active"),
    )
    .await;
    app.create_evidence_request(
        workspace_id,
        &evidence_request("Bravo request", "2026-03-01T00:00:00Z", "paused"),
    )
    .await;
    app.create_evidence_request(
        workspace_id,
        &evidence_request("Alpha request", "2026-03-01T00:00:00Z", "active"),
    )
    .await;
    app.insert_evidence_request_row(other_workspace_id, "Hidden request")
        .await;

    let response = app.get(&collection_path(workspace_id)).await;

    response.assert_status_ok();
    let listed: Value = response.json();
    assert_eq!(
        titles(&listed),
        ["Alpha request", "Bravo request", "Zulu request"]
    );
    assert!(listed
        .as_array()
        .expect("list is an array")
        .iter()
        .all(|request| request["workspace_id"] == workspace_id.to_string()));
}

#[tokio::test]
async fn list_returns_an_empty_array_for_an_empty_workspace() {
    let app = TestApp::builder()
        .workspace("workspace", "Empty workspace")
        .with_default_membership()
        .build()
        .await;
    let workspace_id = app.workspace_id("workspace");

    let response = app.server().get(&collection_path(workspace_id)).await;

    response.assert_status_ok();
    assert_eq!(response.json::<Value>(), json!([]));
}

#[tokio::test]
async fn list_due_returns_only_active_due_requests_for_the_workspace() {
    let app = TestApp::builder()
        .workspace("workspace", "Due workspace")
        .with_default_membership()
        .workspace("other_workspace", "Other due workspace")
        .without_membership()
        .build()
        .await;
    let workspace_id = app.workspace_id("workspace");
    let other_workspace_id = app.workspace_id("other_workspace");

    app.create_evidence_request(
        workspace_id,
        &evidence_request("Due active", "2026-05-20T12:00:00Z", "active"),
    )
    .await;
    app.create_evidence_request(
        workspace_id,
        &evidence_request("Future active", "2026-05-22T12:00:00Z", "active"),
    )
    .await;
    app.create_evidence_request(
        workspace_id,
        &evidence_request("Due paused", "2026-05-19T12:00:00Z", "paused"),
    )
    .await;
    app.create_evidence_request(
        workspace_id,
        &evidence_request("Due retired", "2026-05-18T12:00:00Z", "retired"),
    )
    .await;
    app.insert_evidence_request_row(other_workspace_id, "Other due")
        .await;

    let due_path = format!(
        "{}/due?now=2026-05-21T12%3A00%3A00Z",
        collection_path(workspace_id)
    );
    let response = app.get(&due_path).await;

    response.assert_status_ok();
    let due: Value = response.json();
    assert_eq!(titles(&due), ["Due active"]);
}

#[tokio::test]
async fn get_returns_a_request_and_not_found_for_missing_or_cross_workspace_ids() {
    let app = TestApp::builder()
        .workspace("workspace", "Get workspace")
        .with_default_membership()
        .workspace("other_workspace", "Other get workspace")
        .without_membership()
        .build()
        .await;
    let workspace_id = app.workspace_id("workspace");
    let other_workspace_id = app.workspace_id("other_workspace");
    let body = evidence_request("SOC report", "2026-06-01T00:00:00Z", "active");
    let created = app.create_evidence_request(workspace_id, &body).await;
    let id = created_id(&created);

    let response = app.get(&item_path(workspace_id, id)).await;

    response.assert_status_ok();
    assert_eq!(response.json::<Value>(), created);

    app.get(&item_path(workspace_id, Uuid::new_v4()))
        .await
        .assert_status_not_found();
    app.get(&item_path(other_workspace_id, id))
        .await
        .assert_status_not_found();
}

#[tokio::test]
async fn replace_updates_all_mutable_fields_but_keeps_identity() {
    let app = TestApp::builder()
        .workspace("workspace", "Replace workspace")
        .with_default_membership()
        .build()
        .await;
    let workspace_id = app.workspace_id("workspace");
    let created = app
        .create_evidence_request(
            workspace_id,
            &evidence_request("Original request", "2026-01-01T00:00:00Z", "active"),
        )
        .await;
    let id = created_id(&created);
    let update = json!({
        "title": "Updated request",
        "description": "Updated evidence description.",
        "collection_instructions": "Upload the signed updated report.",
        "cadence": "annually",
        "due_at": "2026-12-31T23:00:00Z",
        "schedule_anchor_at": "2026-12-01T00:00:00Z",
        "freshness_window_days": null,
        "status": "paused"
    });

    let response = app
        .server()
        .put(&item_path(workspace_id, id))
        .json(&update)
        .await;

    response.assert_status_ok();
    let replaced: Value = response.json();
    assert_eq!(replaced["id"], created["id"]);
    assert_eq!(replaced["workspace_id"], created["workspace_id"]);
    assert_eq!(replaced["created_at"], created["created_at"]);
    assert_request_matches(&replaced, &update);
    assert_timestamp(&replaced["updated_at"]);
}

#[tokio::test]
async fn replace_maps_validation_errors_to_bad_request() {
    let app = TestApp::builder()
        .workspace("workspace", "Invalid replace workspace")
        .with_default_membership()
        .build()
        .await;
    let workspace_id = app.workspace_id("workspace");
    let created = app
        .create_evidence_request(
            workspace_id,
            &evidence_request("Replace target", "2026-05-01T00:00:00Z", "active"),
        )
        .await;

    let response = app
        .server()
        .put(&item_path(workspace_id, created_id(&created)))
        .json(&invalid_evidence_request())
        .await;

    assert_validation_error(&response.json(), response.status_code());
}

#[tokio::test]
async fn replace_returns_not_found_for_a_missing_request() {
    let app = TestApp::builder()
        .workspace("workspace", "Missing replace workspace")
        .with_default_membership()
        .build()
        .await;
    let workspace_id = app.workspace_id("workspace");

    app.server()
        .put(&item_path(workspace_id, Uuid::new_v4()))
        .json(&evidence_request(
            "Missing request",
            "2026-08-01T00:00:00Z",
            "active",
        ))
        .await
        .assert_status_not_found();
}

#[tokio::test]
async fn replace_rejects_cross_workspace_ids_without_modifying_the_owner_copy() {
    let app = TestApp::builder()
        .workspace("workspace", "Owning replace workspace")
        .with_default_membership()
        .workspace("other_workspace", "Other replace workspace")
        .without_membership()
        .build()
        .await;
    let workspace_id = app.workspace_id("workspace");
    let other_workspace_id = app.workspace_id("other_workspace");
    let original = app
        .create_evidence_request(
            workspace_id,
            &evidence_request("Owner copy", "2026-02-01T00:00:00Z", "active"),
        )
        .await;
    let id = created_id(&original);

    app.put(&item_path(other_workspace_id, id))
        .json(&evidence_request(
            "Cross workspace update",
            "2027-02-01T00:00:00Z",
            "retired",
        ))
        .await
        .assert_status_not_found();

    let owned = app.get(&item_path(workspace_id, id)).await.json::<Value>();
    assert_eq!(owned, original);
}

#[tokio::test]
async fn authorized_actor_can_create_list_due_get_and_replace_requests() {
    let app = TestApp::builder()
        .workspace("workspace", "Authorized lifecycle workspace")
        .with_default_membership()
        .build()
        .await;
    let workspace_id = app.workspace_id("workspace");
    let body = evidence_request("Authorized request", "2026-05-20T12:00:00Z", "active");

    let created = app.create_evidence_request(workspace_id, &body).await;
    let id = created_id(&created);

    app.server()
        .get(&collection_path(workspace_id))
        .await
        .assert_status_ok();
    app.server()
        .get(&format!(
            "{}/due?now=2026-05-21T12%3A00%3A00Z",
            collection_path(workspace_id)
        ))
        .await
        .assert_status_ok();
    app.server()
        .get(&item_path(workspace_id, id))
        .await
        .assert_status_ok();
    app.server()
        .put(&item_path(workspace_id, id))
        .json(&evidence_request(
            "Authorized replacement",
            "2026-06-20T12:00:00Z",
            "paused",
        ))
        .await
        .assert_status_ok();
}

#[tokio::test]
async fn ungranted_workspace_returns_not_found_for_evidence_request_routes() {
    let app = TestApp::builder()
        .workspace("workspace", "Ungranted workspace")
        .without_membership()
        .build()
        .await;
    let workspace_id = app.workspace_id("workspace");
    let request_id = Uuid::new_v4();

    app.server()
        .get(&collection_path(workspace_id))
        .await
        .assert_status_not_found();
    app.server()
        .get(&format!(
            "{}/due?now=2026-05-21T12%3A00%3A00Z",
            collection_path(workspace_id)
        ))
        .await
        .assert_status_not_found();
    app.server()
        .post(&collection_path(workspace_id))
        .json(&evidence_request(
            "Denied create",
            "2026-05-20T12:00:00Z",
            "active",
        ))
        .await
        .assert_status_not_found();
    app.server()
        .get(&item_path(workspace_id, request_id))
        .await
        .assert_status_not_found();
    app.server()
        .put(&item_path(workspace_id, request_id))
        .json(&evidence_request(
            "Denied replacement",
            "2026-05-20T12:00:00Z",
            "active",
        ))
        .await
        .assert_status_not_found();
}

#[tokio::test]
async fn unsupported_evidence_request_methods_return_method_not_allowed() {
    let app = TestApp::builder()
        .workspace("workspace", "Unsupported method workspace")
        .with_default_membership()
        .build()
        .await;
    let workspace_id = app.workspace_id("workspace");

    let response = app.server().patch(&collection_path(workspace_id)).await;

    assert_eq!(response.status_code(), StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(
        response.json::<Value>()["error"]["code"],
        "method_not_allowed"
    );
}

#[tokio::test]
async fn ungranted_cross_workspace_replace_does_not_modify_owner_copy() {
    let app = TestApp::builder()
        .workspace("workspace", "Granted owner workspace")
        .with_default_membership()
        .workspace("ungranted_workspace", "Ungranted replace workspace")
        .without_membership()
        .build()
        .await;
    let workspace_id = app.workspace_id("workspace");
    let ungranted_workspace_id = app.workspace_id("ungranted_workspace");
    let original = app
        .create_evidence_request(
            workspace_id,
            &evidence_request("Protected owner copy", "2026-02-01T00:00:00Z", "active"),
        )
        .await;
    let id = created_id(&original);

    app.put(&item_path(ungranted_workspace_id, id))
        .json(&evidence_request(
            "Unauthorized cross workspace update",
            "2027-02-01T00:00:00Z",
            "retired",
        ))
        .await
        .assert_status_not_found();

    let owned = app.get(&item_path(workspace_id, id)).await.json::<Value>();
    assert_eq!(owned, original);
}

fn evidence_request(title: &str, due_at: &str, status: &str) -> Value {
    json!({
        "title": title,
        "description": format!("Collect evidence for {title}."),
        "collection_instructions": format!("Upload the artifact for {title}."),
        "cadence": "quarterly",
        "due_at": due_at,
        "schedule_anchor_at": "2026-01-01T00:00:00Z",
        "freshness_window_days": 90,
        "status": status
    })
}

fn invalid_evidence_request() -> Value {
    json!({
        "title": " ",
        "description": "",
        "collection_instructions": "\t",
        "cadence": "weekly",
        "due_at": "2026-01-01T00:00:00Z",
        "schedule_anchor_at": "2026-01-01T00:00:00Z",
        "freshness_window_days": 0,
        "status": "draft"
    })
}

fn assert_request_matches(response: &Value, request: &Value) {
    for field in [
        "title",
        "description",
        "collection_instructions",
        "cadence",
        "due_at",
        "schedule_anchor_at",
        "freshness_window_days",
        "status",
    ] {
        assert_eq!(response[field], request[field], "field {field} differs");
    }
}

fn assert_validation_error(body: &Value, status: StatusCode) {
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "bad_request");
    assert!(body["error"]["details"]
        .as_array()
        .is_some_and(|details| !details.is_empty()));
}

fn assert_uuid(value: &Value) {
    Uuid::parse_str(value.as_str().expect("UUID field is a string")).expect("UUID field parses");
}

fn assert_timestamp(value: &Value) {
    assert!(value
        .as_str()
        .is_some_and(|timestamp| timestamp.ends_with('Z')));
}

fn created_id(created: &Value) -> Uuid {
    Uuid::parse_str(created["id"].as_str().expect("created response has an id"))
        .expect("created response id is a UUID")
}

fn collection_path(workspace_id: Uuid) -> String {
    format!("/workspaces/{workspace_id}/evidence-requests")
}

fn item_path(workspace_id: Uuid, evidence_request_id: Uuid) -> String {
    format!("{}/{}", collection_path(workspace_id), evidence_request_id)
}

fn titles(list: &Value) -> Vec<&str> {
    list.as_array()
        .expect("list response is an array")
        .iter()
        .map(|request| request["title"].as_str().expect("request has a title"))
        .collect()
}
