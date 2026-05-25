use axum::http::StatusCode;
use serde_json::{json, Value};
use uuid::Uuid;

use super::support::TestApp;

#[tokio::test]
async fn repeated_seed_exposes_soc2_framework_requirements_and_workspace_controls() {
    let app = TestApp::builder()
        .workspace("workspace", "Seeded controls workspace")
        .with_default_membership()
        .build()
        .await;
    let workspace_id = app.workspace_id("workspace");

    seed_soc2(&app, workspace_id).await;
    seed_soc2(&app, workspace_id).await;

    let frameworks = app
        .server()
        .get(&format!("/workspaces/{workspace_id}/frameworks"))
        .await
        .json::<Value>();
    assert_eq!(frameworks.as_array().unwrap().len(), 1);
    assert_eq!(frameworks[0]["code"], "soc2");

    let requirements = app
        .server()
        .get(&format!(
            "/workspaces/{workspace_id}/frameworks/{}/requirements",
            soc2_framework_id()
        ))
        .await
        .json::<Value>();
    assert_eq!(codes(&requirements), ["CC6.1", "CC7.1"]);

    let controls = app
        .server()
        .get(&format!("/workspaces/{workspace_id}/controls"))
        .await
        .json::<Value>();
    assert_eq!(codes(&controls), ["PP-AC-01"]);
    assert_eq!(controls[0]["framework_requirements"][0]["code"], "CC6.1");
}

#[tokio::test]
async fn create_list_get_and_update_controls_with_requirement_mappings() {
    let app = TestApp::builder()
        .workspace("workspace", "Control lifecycle workspace")
        .with_default_membership()
        .workspace("other_workspace", "Other control lifecycle workspace")
        .with_default_membership()
        .build()
        .await;
    let workspace_id = app.workspace_id("workspace");
    let other_workspace_id = app.workspace_id("other_workspace");
    seed_soc2_reference(&app).await;

    let create = control("PP-LOG-01", "Log review", vec![cc61_id()]);
    let created = app
        .server()
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

    let listed = app
        .server()
        .get(&format!("/workspaces/{workspace_id}/controls"))
        .await
        .json::<Value>();
    assert_eq!(listed.as_array().unwrap().len(), 1);
    assert_eq!(listed[0]["id"], control_id.to_string());

    app.server()
        .get(&format!("/workspaces/{workspace_id}/controls/{control_id}"))
        .await
        .assert_status_ok();
    app.server()
        .get(&format!(
            "/workspaces/{other_workspace_id}/controls/{control_id}"
        ))
        .await
        .assert_status_not_found();

    let update = control("PP-LOG-02", "Updated log review", vec![cc71_id()]);
    let updated = app
        .server()
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

    app.server()
        .post(&format!("/workspaces/{workspace_id}/controls"))
        .json(&control("PP-LOG-02", "Duplicate", vec![cc61_id()]))
        .await
        .assert_status(StatusCode::CONFLICT);
    app.server()
        .post(&format!("/workspaces/{workspace_id}/controls"))
        .json(&control("PP-MISSING", "Missing", vec![Uuid::new_v4()]))
        .await
        .assert_status_not_found();
}

#[tokio::test]
async fn evidence_request_control_mappings_create_list_delete_and_conflict() {
    let app = TestApp::builder()
        .workspace("workspace", "Mapping workspace")
        .with_default_membership()
        .workspace("other_workspace", "Other mapping workspace")
        .with_default_membership()
        .build()
        .await;
    let workspace_id = app.workspace_id("workspace");
    let other_workspace_id = app.workspace_id("other_workspace");
    seed_soc2_reference(&app).await;
    let control = app
        .server()
        .post(&format!("/workspaces/{workspace_id}/controls"))
        .json(&control("PP-AC-02", "Access approval", vec![cc61_id()]))
        .await
        .json::<Value>();
    let control_id = uuid_field(&control["id"]);
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
        .server()
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

    app.server()
        .post(&path)
        .json(&json!({
            "control_id": control_id,
            "rationale": "Duplicate mapping"
        }))
        .await
        .assert_status(StatusCode::CONFLICT);

    let listed = app.server().get(&path).await.json::<Value>();
    assert_eq!(listed.as_array().unwrap().len(), 1);

    app.server()
        .get(&mapping_path(other_workspace_id, evidence_request_id))
        .await
        .assert_status_not_found();
    app.server()
        .post(&mapping_path(workspace_id, Uuid::new_v4()))
        .json(&json!({
            "control_id": control_id,
            "rationale": "Missing request"
        }))
        .await
        .assert_status_not_found();

    app.server()
        .delete(&format!("{path}/{control_id}"))
        .await
        .assert_status(StatusCode::NO_CONTENT);
    assert_eq!(app.server().get(&path).await.json::<Value>(), json!([]));
    app.server()
        .delete(&format!("{path}/{control_id}"))
        .await
        .assert_status_not_found();
}

#[tokio::test]
async fn ungranted_workspace_returns_not_found_for_control_routes() {
    let app = TestApp::builder()
        .workspace("workspace", "Ungranted controls workspace")
        .without_membership()
        .build()
        .await;
    let workspace_id = app.workspace_id("workspace");
    seed_soc2_reference(&app).await;

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

async fn seed_soc2(app: &TestApp, workspace_id: Uuid) {
    seed_soc2_reference(app).await;
    let mut client = app.postgres().get().await.unwrap();
    let transaction = client.transaction().await.unwrap();
    transaction
        .execute(
            r#"
INSERT INTO controls (id, workspace_id, code, title, description)
VALUES ($1, $2, 'PP-AC-01', 'Quarterly access review', 'Review access quarterly.')
ON CONFLICT (workspace_id, code) DO UPDATE
SET title = EXCLUDED.title,
    description = EXCLUDED.description
"#,
            &[&seed_control_id(), &workspace_id],
        )
        .await
        .unwrap();
    transaction
        .execute(
            "DELETE FROM control_framework_requirement_mappings WHERE control_id = $1",
            &[&seed_control_id()],
        )
        .await
        .unwrap();
    transaction
        .execute(
            r#"
INSERT INTO control_framework_requirement_mappings (control_id, framework_requirement_id)
VALUES ($1, $2)
ON CONFLICT DO NOTHING
"#,
            &[&seed_control_id(), &cc61_id()],
        )
        .await
        .unwrap();
    transaction.commit().await.unwrap();
}

async fn seed_soc2_reference(app: &TestApp) {
    let client = app.postgres().get().await.unwrap();
    client
        .execute(
            r#"
INSERT INTO frameworks (id, code, name, description)
VALUES ($1, 'soc2', 'SOC 2', 'SOC 2 Trust Services Criteria.')
ON CONFLICT (code) DO UPDATE
SET name = EXCLUDED.name,
    description = EXCLUDED.description
"#,
            &[&soc2_framework_id()],
        )
        .await
        .unwrap();
    for (id, code, title) in [
        (cc61_id(), "CC6.1", "Logical access security"),
        (cc71_id(), "CC7.1", "System monitoring"),
    ] {
        client
            .execute(
                r#"
INSERT INTO framework_requirements (id, framework_id, code, title, description)
VALUES ($1, $2, $3, $4, 'Seeded SOC 2 requirement.')
ON CONFLICT (framework_id, code) DO UPDATE
SET title = EXCLUDED.title,
    description = EXCLUDED.description
"#,
                &[&id, &soc2_framework_id(), &code, &title],
            )
            .await
            .unwrap();
    }
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

fn uuid_field(value: &Value) -> Uuid {
    Uuid::parse_str(value.as_str().unwrap()).unwrap()
}

fn soc2_framework_id() -> Uuid {
    Uuid::parse_str("30000000-0000-4000-8000-000000000000").unwrap()
}

fn cc61_id() -> Uuid {
    Uuid::parse_str("30000000-0000-4000-8000-000000000001").unwrap()
}

fn cc71_id() -> Uuid {
    Uuid::parse_str("30000000-0000-4000-8000-000000000002").unwrap()
}

fn seed_control_id() -> Uuid {
    Uuid::parse_str("30000000-0000-4000-8000-000000000101").unwrap()
}
