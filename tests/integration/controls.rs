use axum::http::StatusCode;
use serde_json::{json, Value};
use uuid::Uuid;

use super::support::{cc61_id, cc71_id, TestApp};

#[tokio::test]
async fn create_list_get_and_update_controls_with_requirement_mappings() {
    let app = TestApp::builder()
        .with_soc2_reference_data()
        .workspace("workspace", "Control lifecycle workspace")
        .with_default_membership()
        .workspace("other_workspace", "Other control lifecycle workspace")
        .with_default_membership()
        .build()
        .await;
    let workspace_id = app.workspace_id("workspace");
    let other_workspace_id = app.workspace_id("other_workspace");

    let create = control("PP-LOG-01", "Log review", vec![cc71_id(), cc61_id()]);
    let created = app
        .post(&format!("/workspaces/{workspace_id}/controls"))
        .json(&create)
        .await
        .json::<Value>();
    let control_id = uuid_field(&created["id"]);
    assert_eq!(created["workspace_id"], workspace_id.to_string());
    assert_eq!(
        created["framework_requirements"][0]["id"],
        cc61_id().to_string()
    );
    assert_eq!(
        created["framework_requirements"][1]["id"],
        cc71_id().to_string()
    );
    let unmapped = app
        .post(&format!("/workspaces/{workspace_id}/controls"))
        .json(&control("PP-AUD-01", "Audit trail", vec![]))
        .await
        .json::<Value>();

    let listed = app
        .get(&format!("/workspaces/{workspace_id}/controls"))
        .await
        .json::<Value>();
    assert_eq!(codes(&listed), ["PP-AUD-01", "PP-LOG-01"]);
    assert_eq!(listed[0]["id"], unmapped["id"]);
    assert_eq!(listed[0]["framework_requirements"], json!([]));
    assert_eq!(listed[1]["id"], control_id.to_string());
    assert_eq!(
        requirement_codes(&listed[1]["framework_requirements"]),
        ["CC6.1", "CC7.1"]
    );

    app.get(&format!("/workspaces/{workspace_id}/controls/{control_id}"))
        .await
        .assert_status_ok();
    app.get(&format!(
        "/workspaces/{other_workspace_id}/controls/{control_id}"
    ))
    .await
    .assert_status_not_found();
    app.put(&format!(
        "/workspaces/{other_workspace_id}/controls/{control_id}"
    ))
    .json(&control("PP-LOG-03", "Wrong workspace", vec![cc61_id()]))
    .await
    .assert_status_not_found();

    let update = control("PP-LOG-02", "Updated log review", vec![cc71_id()]);
    let updated = app
        .put(&format!("/workspaces/{workspace_id}/controls/{control_id}"))
        .json(&update)
        .await
        .json::<Value>();
    assert_eq!(updated["id"], control_id.to_string());
    assert_eq!(updated["code"], "PP-LOG-02");
    assert_eq!(
        updated["framework_requirements"][0]["id"],
        cc71_id().to_string()
    );

    let duplicate = app
        .post(&format!("/workspaces/{workspace_id}/controls"))
        .json(&control("PP-LOG-02", "Duplicate", vec![cc61_id()]))
        .await;
    duplicate.assert_status(StatusCode::CONFLICT);
    let duplicate_body = duplicate.json::<Value>();
    assert_eq!(duplicate_body["error"]["code"], "control_code_taken");
    assert_eq!(
        duplicate_body["error"]["message"],
        "a control with this code already exists in the workspace"
    );
    app.post(&format!("/workspaces/{workspace_id}/controls"))
        .json(&control("PP-MISSING", "Missing", vec![Uuid::new_v4()]))
        .await
        .assert_status_bad_request();

    app.put(&format!("/workspaces/{workspace_id}/controls/{control_id}"))
        .json(&control("PP-MISSING", "Missing", vec![Uuid::new_v4()]))
        .await
        .assert_status_bad_request();
}

#[tokio::test]
async fn evidence_request_control_mappings_create_list_delete_and_conflict() {
    let app = TestApp::builder()
        .with_soc2_reference_data()
        .workspace("workspace", "Mapping workspace")
        .with_control("PP-AC-02", "Access approval", vec![cc61_id()])
        .with_default_membership()
        .workspace("other_workspace", "Other mapping workspace")
        .with_control("PP-AC-03", "Other access approval", vec![cc61_id()])
        .with_default_membership()
        .build()
        .await;
    let workspace_id = app.workspace_id("workspace");
    let other_workspace_id = app.workspace_id("other_workspace");
    let control_id = app.control_id("workspace", "PP-AC-02");
    let other_control_id = app.control_id("other_workspace", "PP-AC-03");
    let evidence_request = app
        .create_evidence_request(
            workspace_id,
            &json!({
                "title": "Access approval evidence",
                "description": "Collect approval evidence.",
                "collection_instructions": "Upload approval export.",
                "cadence": "quarterly",
                "due_at": "2026-07-01T00:00:00Z",
                "schedule_anchor_at": "2026-01-01T00:00:00Z",
                "freshness_window_days": 90,
                "status": "active"
            }),
        )
        .await;
    let evidence_request_id = uuid_field(&evidence_request["id"]);
    let path = mapping_path(workspace_id, evidence_request_id);

    let created = app
        .post(&path)
        .json(&json!({
            "control_id": control_id,
            "rationale": "This request proves access approvals were reviewed."
        }))
        .await
        .json::<Value>();
    assert_eq!(created["control"]["id"], control_id.to_string());
    assert_eq!(
        created["rationale"],
        "This request proves access approvals were reviewed."
    );

    app.post(&path)
        .json(&json!({
            "control_id": control_id,
            "rationale": "Duplicate mapping"
        }))
        .await
        .assert_status(StatusCode::CONFLICT);

    let listed = app.get(&path).await.json::<Value>();
    assert_eq!(listed.as_array().unwrap().len(), 1);

    app.get(&mapping_path(other_workspace_id, evidence_request_id))
        .await
        .assert_status_not_found();
    app.post(&mapping_path(workspace_id, Uuid::new_v4()))
        .json(&json!({
            "control_id": control_id,
            "rationale": "Missing request"
        }))
        .await
        .assert_status_not_found();
    app.post(&path)
        .json(&json!({
            "control_id": other_control_id,
            "rationale": "Cross-workspace control"
        }))
        .await
        .assert_status_not_found();

    app.delete(&format!("{path}/{control_id}"))
        .await
        .assert_status(StatusCode::NO_CONTENT);
    assert_eq!(app.get(&path).await.json::<Value>(), json!([]));
    app.delete(&format!("{path}/{control_id}"))
        .await
        .assert_status_not_found();
}

#[tokio::test]
async fn ungranted_workspace_returns_not_found_for_control_routes() {
    let app = TestApp::builder()
        .with_soc2_reference_data()
        .workspace("workspace", "Ungranted controls workspace")
        .without_membership()
        .build()
        .await;
    let workspace_id = app.workspace_id("workspace");

    app.server()
        .get(&format!("/workspaces/{workspace_id}/frameworks"))
        .await
        .assert_status_not_found();
    app.server()
        .get(&format!("/workspaces/{workspace_id}/controls"))
        .await
        .assert_status_not_found();
    app.server()
        .post(&format!("/workspaces/{workspace_id}/controls"))
        .json(&control("PP-DENIED", "Denied", vec![cc61_id()]))
        .await
        .assert_status_not_found();
    app.server()
        .get(&mapping_path(workspace_id, Uuid::new_v4()))
        .await
        .assert_status_not_found();
}

fn control(code: &str, title: &str, requirement_ids: Vec<Uuid>) -> Value {
    json!({
        "code": code,
        "title": title,
        "description": format!("Control description for {title}."),
        "framework_requirement_ids": requirement_ids
    })
}

fn mapping_path(workspace_id: Uuid, evidence_request_id: Uuid) -> String {
    format!("/workspaces/{workspace_id}/evidence-requests/{evidence_request_id}/control-mappings")
}

fn codes(list: &Value) -> Vec<&str> {
    list.as_array()
        .unwrap()
        .iter()
        .map(|item| item["code"].as_str().unwrap())
        .collect()
}

fn requirement_codes(list: &Value) -> Vec<&str> {
    list.as_array()
        .unwrap()
        .iter()
        .map(|item| item["code"].as_str().unwrap())
        .collect()
}

fn uuid_field(value: &Value) -> Uuid {
    Uuid::parse_str(value.as_str().unwrap()).unwrap()
}
