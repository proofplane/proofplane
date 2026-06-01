use axum::http::StatusCode;
use serde_json::{json, Value};
use uuid::Uuid;

use super::support::TestApp;

#[tokio::test]
async fn create_returns_the_submission() {
    let app = TestApp::builder()
        .workspace("workspace", "Submission create workspace")
        .with_default_membership()
        .build()
        .await;
    let workspace_id = app.workspace_id("workspace");
    let request = app
        .create_evidence_request(
            workspace_id,
            &evidence_request("Submission target", "2026-05-01T00:00:00Z"),
        )
        .await;
    let evidence_request_id = created_id(&request);
    let body = evidence_submission();

    let response = app
        .post(&collection_path(workspace_id, evidence_request_id))
        .json(&body)
        .await;

    response.assert_status_ok();
    let created: Value = response.json();
    assert_uuid(&created["id"]);
    assert_eq!(
        created["evidence_request_id"],
        evidence_request_id.to_string()
    );
    assert_eq!(created["submitted_by"], app.actor_id());
    assert_submission_matches(&created, &body);
    assert_timestamp(&created["received_at"]);
}

#[tokio::test]
async fn get_returns_submission_detail_with_empty_attachments() {
    let app = TestApp::builder()
        .workspace("workspace", "Submission detail workspace")
        .with_default_membership()
        .build()
        .await;
    let workspace_id = app.workspace_id("workspace");
    let request = app
        .create_evidence_request(
            workspace_id,
            &evidence_request("Detail target", "2026-05-01T00:00:00Z"),
        )
        .await;
    let evidence_request_id = created_id(&request);
    let created = app
        .post(&collection_path(workspace_id, evidence_request_id))
        .json(&evidence_submission())
        .await
        .json::<Value>();
    let submission_id = created_id(&created);

    let response = app.get(&item_path(workspace_id, submission_id)).await;

    response.assert_status_ok();
    let detail: Value = response.json();
    assert_eq!(detail["submission"], created);
    assert_eq!(detail["attachments"], json!([]));
}

#[tokio::test]
async fn create_maps_validation_errors_to_bad_request() {
    let app = TestApp::builder()
        .workspace("workspace", "Invalid submission workspace")
        .with_default_membership()
        .build()
        .await;
    let workspace_id = app.workspace_id("workspace");
    let request = app
        .create_evidence_request(
            workspace_id,
            &evidence_request("Invalid target", "2026-05-01T00:00:00Z"),
        )
        .await;
    let evidence_request_id = created_id(&request);

    let response = app
        .post(&collection_path(workspace_id, evidence_request_id))
        .json(&json!({
            "coverage_start_at": "2026-04-01T00:00:00Z",
            "coverage_end_at": "2026-03-31T23:59:59Z",
            "source_system": " ",
            "collection_method": "",
            "provenance": ["not", "an", "object"]
        }))
        .await;

    assert_eq!(response.status_code(), StatusCode::BAD_REQUEST);
    let body = response.json::<Value>();
    assert_eq!(body["error"]["code"], "bad_request");
    let details = body["error"]["details"]
        .as_array()
        .expect("details is an array");
    assert!(details
        .iter()
        .any(|detail| detail == "source_system must not be empty"));
    assert!(details
        .iter()
        .any(|detail| detail == "collection_method must not be empty"));
    assert!(details
        .iter()
        .any(|detail| detail == "provenance must be a JSON object"));
    assert!(details.iter().any(
        |detail| detail == "coverage_end_at must be greater than or equal to coverage_start_at"
    ));
}

#[tokio::test]
async fn create_defaults_omitted_provenance_to_empty_object() {
    let app = TestApp::builder()
        .workspace("workspace", "Default provenance workspace")
        .with_default_membership()
        .build()
        .await;
    let workspace_id = app.workspace_id("workspace");
    let request = app
        .create_evidence_request(
            workspace_id,
            &evidence_request("Default provenance target", "2026-05-01T00:00:00Z"),
        )
        .await;
    let evidence_request_id = created_id(&request);

    let response = app
        .post(&collection_path(workspace_id, evidence_request_id))
        .json(&json!({
            "coverage_start_at": "2026-01-01T00:00:00Z",
            "coverage_end_at": "2026-03-31T23:59:59Z",
            "source_system": "okta",
            "collection_method": "api_export"
        }))
        .await;

    response.assert_status_ok();
    assert_eq!(response.json::<Value>()["provenance"], json!({}));
}

#[tokio::test]
async fn create_returns_not_found_for_missing_or_cross_workspace_requests() {
    let app = TestApp::builder()
        .workspace("workspace", "Submission owner workspace")
        .with_default_membership()
        .workspace("other_workspace", "Submission other workspace")
        .with_default_membership()
        .build()
        .await;
    let workspace_id = app.workspace_id("workspace");
    let other_workspace_id = app.workspace_id("other_workspace");
    let request = app
        .create_evidence_request(
            workspace_id,
            &evidence_request("Owner target", "2026-05-01T00:00:00Z"),
        )
        .await;
    let evidence_request_id = created_id(&request);

    app.post(&collection_path(workspace_id, Uuid::new_v4()))
        .json(&evidence_submission())
        .await
        .assert_status_not_found();

    app.post(&collection_path(other_workspace_id, evidence_request_id))
        .json(&evidence_submission())
        .await
        .assert_status_not_found();
}

#[tokio::test]
async fn get_returns_not_found_for_missing_or_cross_workspace_submissions() {
    let app = TestApp::builder()
        .workspace("workspace", "Submission get owner workspace")
        .with_default_membership()
        .workspace("other_workspace", "Submission get other workspace")
        .with_default_membership()
        .build()
        .await;
    let workspace_id = app.workspace_id("workspace");
    let other_workspace_id = app.workspace_id("other_workspace");
    let request = app
        .create_evidence_request(
            workspace_id,
            &evidence_request("Get owner target", "2026-05-01T00:00:00Z"),
        )
        .await;
    let evidence_request_id = created_id(&request);
    let created = app
        .post(&collection_path(workspace_id, evidence_request_id))
        .json(&evidence_submission())
        .await
        .json::<Value>();
    let submission_id = created_id(&created);

    app.get(&item_path(workspace_id, Uuid::new_v4()))
        .await
        .assert_status_not_found();
    app.get(&item_path(other_workspace_id, submission_id))
        .await
        .assert_status_not_found();
}

#[tokio::test]
async fn ungranted_workspace_returns_not_found_for_submission_routes() {
    let app = TestApp::builder()
        .workspace("granted_workspace", "Granted submission workspace")
        .with_default_membership()
        .workspace("ungranted_workspace", "Ungranted submission workspace")
        .without_membership()
        .build()
        .await;
    let granted_workspace_id = app.workspace_id("granted_workspace");
    let ungranted_workspace_id = app.workspace_id("ungranted_workspace");
    let request = app
        .create_evidence_request(
            granted_workspace_id,
            &evidence_request("Protected target", "2026-05-01T00:00:00Z"),
        )
        .await;
    let evidence_request_id = created_id(&request);
    let created = app
        .post(&collection_path(granted_workspace_id, evidence_request_id))
        .json(&evidence_submission())
        .await
        .json::<Value>();
    let submission_id = created_id(&created);

    app.server()
        .post(&collection_path(
            ungranted_workspace_id,
            evidence_request_id,
        ))
        .json(&evidence_submission())
        .await
        .assert_status_not_found();
    app.server()
        .get(&item_path(ungranted_workspace_id, submission_id))
        .await
        .assert_status_not_found();
}

#[tokio::test]
async fn unsupported_submission_methods_return_method_not_allowed() {
    let app = TestApp::builder()
        .workspace("workspace", "Unsupported submission method workspace")
        .with_default_membership()
        .build()
        .await;
    let workspace_id = app.workspace_id("workspace");
    let request_id = Uuid::new_v4();
    let submission_id = Uuid::new_v4();

    let create_response = app
        .server()
        .patch(&collection_path(workspace_id, request_id))
        .await;
    assert_method_not_allowed(&create_response.json(), create_response.status_code());

    let get_response = app
        .server()
        .post(&item_path(workspace_id, submission_id))
        .await;
    assert_eq!(get_response.status_code(), StatusCode::METHOD_NOT_ALLOWED);
}

fn evidence_request(title: &str, due_at: &str) -> Value {
    json!({
        "title": title,
        "description": format!("Collect evidence for {title}."),
        "collection_instructions": format!("Upload the artifact for {title}."),
        "cadence": "quarterly",
        "due_at": due_at,
        "schedule_anchor_at": "2026-01-01T00:00:00Z",
        "freshness_window_days": 90,
        "status": "active"
    })
}

fn evidence_submission() -> Value {
    json!({
        "coverage_start_at": "2026-01-01T00:00:00Z",
        "coverage_end_at": "2026-03-31T23:59:59Z",
        "source_system": "okta",
        "collection_method": "api_export",
        "provenance": {
            "external_run_id": "run-123",
            "exported_at": "2026-04-01T00:00:00Z"
        }
    })
}

fn assert_submission_matches(response: &Value, request: &Value) {
    for field in [
        "coverage_start_at",
        "coverage_end_at",
        "source_system",
        "collection_method",
        "provenance",
    ] {
        assert_eq!(response[field], request[field], "field {field} differs");
    }
}

fn assert_method_not_allowed(body: &Value, status: StatusCode) {
    assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(body["error"]["code"], "method_not_allowed");
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

fn collection_path(workspace_id: Uuid, evidence_request_id: Uuid) -> String {
    format!("/workspaces/{workspace_id}/evidence-requests/{evidence_request_id}/submissions")
}

fn item_path(workspace_id: Uuid, submission_id: Uuid) -> String {
    format!("/workspaces/{workspace_id}/evidence-submissions/{submission_id}")
}
