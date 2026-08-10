use http::StatusCode;
use proofplane::{
    domain::WorkspacePermission,
    routes::request_context::REQUEST_ID_HEADER,
    worker::{DOCUMENT_FINALIZATION_REQUESTED, DOCUMENT_SCAN_REQUESTED},
};
use serde_json::{json, Value};
use url::Url;
use uuid::Uuid;

use crate::support::{
    auditor_access::{assert_portal_read_audit_event, authenticate_auditor, invite_token},
    documents::upload_form,
    harness,
    http::{local_path, request_cookie},
    mcp::McpClient,
    oauth::authorize_agent_connection,
    scenario::{
        types::{TestEvidenceSubmission, TestPolicyDocument},
        ScenarioBuilder,
    },
};

use super::{assertions::*, helpers::*};

#[tokio::test]
async fn complete_safe_graph_is_workspace_scoped_and_emits_exact_read_audits() {
    let app = harness::app().await;
    let subject = "auth0|auditor-portal-complete-owner";
    let foreign_subject = "auth0|auditor-portal-complete-foreign";
    let workspace_name = "Auditor Portal Complete Graph";
    let foreign_workspace_name = "Auditor Portal Complete Foreign";
    let auditor_email = "auditor-portal-complete@example.com";

    let scenario = ScenarioBuilder::new(&app)
        .with_user(subject)
        .with_workspace(subject, workspace_name)
        .with_evidence(workspace_name, "Mapped access review evidence")
        .with_evidence_document(
            workspace_name,
            "Mapped access review evidence",
            "uploaded-evidence.txt",
            b"uploaded evidence bytes",
            "2026-01-01T00:00:00.000Z",
            "2026-03-31T23:59:59.000Z",
        )
        .with_evidence(workspace_name, "Unmapped owner evidence")
        .with_control(workspace_name, "PP-AUD-01", "Mapped auditor control")
        .with_control(workspace_name, "PP-AUD-02", "Standalone auditor control")
        .with_policy(workspace_name, "Mapped Policy")
        .with_policy(workspace_name, "Unmapped Policy")
        .with_evidence_control_mapping(
            workspace_name,
            "Mapped access review evidence",
            "PP-AUD-01",
            "Shows that access reviews were performed.",
        )
        .with_policy_control_mapping(workspace_name, "Mapped Policy", "PP-AUD-01")
        .with_user(foreign_subject)
        .with_workspace(foreign_subject, foreign_workspace_name)
        .with_evidence(foreign_workspace_name, "Foreign mapped evidence")
        .with_control(foreign_workspace_name, "PP-FOREIGN-01", "Foreign control")
        .with_policy(foreign_workspace_name, "Foreign Policy")
        .with_evidence_control_mapping(
            foreign_workspace_name,
            "Foreign mapped evidence",
            "PP-FOREIGN-01",
            "Foreign mapping rationale.",
        )
        .with_policy_control_mapping(foreign_workspace_name, "Foreign Policy", "PP-FOREIGN-01")
        .build()
        .await;
    let workspace = scenario.workspace(workspace_name);
    let workspace_id = workspace.id;
    let mapped_evidence = workspace.evidence("Mapped access review evidence");
    let mapped_control = workspace.control("PP-AUD-01");
    let standalone_control = workspace.control("PP-AUD-02");
    let mapped_policy = workspace.policy("Mapped Policy");
    let unmapped_policy = workspace.policy("Unmapped Policy");
    let uploaded_submission = mapped_evidence.submission("uploaded-evidence.txt");

    let owner_token = authorize_agent_connection(
        &app,
        subject,
        "Auditor Portal Complete Owner",
        &WorkspacePermission::ALL,
    )
    .await;
    let owner = McpClient::connect(app.mcp_server(), &owner_token).await;

    let evidence_grant = owner
        .call_tool(
            "manage_evidence_submissions",
            json!({
                "evidence_id": mapped_evidence.id,
                "valid_from": "2026-01-01T00:00:00.000Z",
                "valid_until": "2026-03-31T23:59:59.000Z",
            }),
        )
        .await;
    let evidence_redeemed = app
        .app_server()
        .get(&local_path(
            evidence_grant["url"]
                .as_str()
                .expect("evidence grant URL is a string"),
        ))
        .await;
    evidence_redeemed.assert_status(StatusCode::SEE_OTHER);
    assert_eq!(
        evidence_redeemed.header("location"),
        "/evidence-document-uploads"
    );
    let evidence_cookie = request_cookie(
        evidence_redeemed
            .header("set-cookie")
            .to_str()
            .expect("evidence session cookie is text"),
    );

    let pending_evidence_request_id = Uuid::new_v4();
    let mut pending_evidence_gate = app
        .pipeline_controls()
        .hold(DOCUMENT_SCAN_REQUESTED, pending_evidence_request_id);
    let mut pending_evidence_events = app.pipeline_events().subscribe();
    app.app_server()
        .post(EVIDENCE_UPLOAD_PATH)
        .add_header("cookie", evidence_cookie)
        .add_header(REQUEST_ID_HEADER, pending_evidence_request_id.to_string())
        .multipart(upload_form(
            b"pending evidence bytes",
            "pending-evidence.txt",
        ))
        .await
        .assert_status(StatusCode::SEE_OTHER);
    let pending_evidence_interception = pending_evidence_gate.await_interception().await;
    let pending_evidence_submission = owner
        .call_tool(
            "list_evidence_submissions",
            json!({ "evidence_id": mapped_evidence.id }),
        )
        .await["submissions"]
        .as_array()
        .expect("submissions is an array")
        .iter()
        .find(|submission| submission["document"]["filename"] == "pending-evidence.txt")
        .expect("pending evidence submission is listed")
        .clone();
    let pending_evidence_document_id = pending_evidence_submission["document"]["id"]
        .as_str()
        .expect("pending evidence document id is a string")
        .to_owned();
    assert_eq!(
        pending_evidence_interception.aggregate_id,
        pending_evidence_document_id
    );
    assert_eq!(
        pending_evidence_submission["document"]["upload_status"],
        "pending"
    );
    let pending_evidence_submission =
        TestEvidenceSubmission::from_mcp(&pending_evidence_submission);

    let policy_grant = owner
        .call_tool(
            "manage_policy_document",
            json!({ "policy_id": mapped_policy.id }),
        )
        .await;
    let policy_redeemed = app
        .app_server()
        .get(&local_path(
            policy_grant["url"]
                .as_str()
                .expect("policy grant URL is a string"),
        ))
        .await;
    policy_redeemed.assert_status(StatusCode::SEE_OTHER);
    assert_eq!(
        policy_redeemed.header("location"),
        "/policy-document-uploads"
    );
    let policy_cookie = request_cookie(
        policy_redeemed
            .header("set-cookie")
            .to_str()
            .expect("policy session cookie is text"),
    );
    let pending_policy_request_id = Uuid::new_v4();
    let mut pending_policy_gate = app
        .pipeline_controls()
        .hold(DOCUMENT_SCAN_REQUESTED, pending_policy_request_id);
    let mut pending_policy_events = app.pipeline_events().subscribe();
    app.app_server()
        .post(POLICY_UPLOAD_PATH)
        .add_header("cookie", policy_cookie)
        .add_header(REQUEST_ID_HEADER, pending_policy_request_id.to_string())
        .multipart(upload_form(b"pending policy bytes", "pending-policy.txt"))
        .await
        .assert_status(StatusCode::SEE_OTHER);
    let pending_policy_interception = pending_policy_gate.await_interception().await;
    let mapped_policy_read = owner
        .call_tool("get_policy", json!({ "policy_id": mapped_policy.id }))
        .await;
    let pending_policy_document_id = mapped_policy_read["document"]["id"]
        .as_str()
        .expect("pending policy document id is a string")
        .to_owned();
    assert_eq!(
        pending_policy_interception.aggregate_id,
        pending_policy_document_id
    );
    assert_eq!(mapped_policy_read["document"]["upload_status"], "pending");
    let pending_policy_document =
        TestPolicyDocument::from_mcp(mapped_policy.id, &mapped_policy_read["document"]);

    let auditor_token = authorize_agent_connection(
        &app,
        subject,
        "Auditor Portal Complete Access",
        &[WorkspacePermission::ManageAuditorAccess],
    )
    .await;
    let auditor_client = McpClient::connect(app.mcp_server(), &auditor_token).await;
    let access = auditor_client
        .call_tool(
            "create_auditor_access_link",
            json!({
                "email": auditor_email,
                "expires_at": "2099-01-01T00:00:00Z",
                "period_start": "2026-01-01T00:00:00Z",
                "period_end": "2026-03-31T23:59:59Z",
            }),
        )
        .await;
    let invite_url = Url::parse(
        access["url"]
            .as_str()
            .expect("auditor access URL is a string"),
    )
    .expect("auditor access URL parses");
    let invite_token = invite_token(&invite_url);
    app.app_server()
        .get(&local_path(invite_url.as_str()))
        .await
        .assert_status_ok();
    let auditor_cookie = authenticate_auditor(
        &app,
        workspace_id,
        &invite_token,
        "auth0|auditor-portal-complete-identity",
        auditor_email,
    )
    .await;

    let ((portal_response, portal_request_id), portal_logs) = app
        .capture_audit_logs(async |request_id| {
            let response = app
                .app_server()
                .get(PORTAL_DATA_PATH)
                .add_header("cookie", auditor_cookie)
                .add_header(REQUEST_ID_HEADER, request_id.to_string())
                .await;
            (response, request_id)
        })
        .await;

    pending_evidence_gate.release();
    assert_eq!(
        pending_evidence_events
            .await_event(DOCUMENT_SCAN_REQUESTED, &pending_evidence_document_id)
            .await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        pending_evidence_events
            .await_event(
                DOCUMENT_FINALIZATION_REQUESTED,
                &pending_evidence_document_id,
            )
            .await,
        StatusCode::NO_CONTENT
    );
    pending_policy_gate.release();
    assert_eq!(
        pending_policy_events
            .await_event(DOCUMENT_SCAN_REQUESTED, &pending_policy_document_id)
            .await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        pending_policy_events
            .await_event(DOCUMENT_FINALIZATION_REQUESTED, &pending_policy_document_id)
            .await,
        StatusCode::NO_CONTENT
    );

    portal_response.assert_status_ok();
    let body = portal_response.json::<Value>();
    assert_portal_envelope(&body, workspace_name, auditor_email);
    assert_framework_catalog(&body, &scenario);

    let controls = body["controls"].as_array().expect("controls is an array");
    assert_eq!(controls.len(), 2);
    assert_eq!(
        controls
            .iter()
            .map(|control| control["code"].as_str().expect("control code is text"))
            .collect::<Vec<_>>(),
        ["PP-AUD-01", "PP-AUD-02"]
    );
    assert_control_read_model(&controls[0], mapped_control);
    assert_eq!(
        controls[0]["framework_requirements"],
        json!([]),
        "test controls deliberately remain unlinked"
    );
    let evidence = controls[0]["evidence"]
        .as_array()
        .expect("control evidence is an array");
    assert_eq!(evidence.len(), 1);
    assert_evidence_read_model(
        &evidence[0],
        mapped_evidence.id,
        "Mapped access review evidence",
        "Shows that access reviews were performed.",
    );
    let submissions = evidence[0]["submissions"]
        .as_array()
        .expect("portal submissions is an array");
    assert_eq!(submissions.len(), 2);
    assert_eq!(
        submissions
            .iter()
            .map(|submission| submission["document"]["filename"]
                .as_str()
                .expect("filename is text"))
            .collect::<Vec<_>>(),
        ["pending-evidence.txt", "uploaded-evidence.txt"]
    );
    assert_submission_read_model(&submissions[0], &pending_evidence_submission, false);
    assert_submission_read_model(&submissions[1], uploaded_submission, true);
    let control_policies = controls[0]["policies"]
        .as_array()
        .expect("control policies is an array");
    assert_eq!(control_policies.len(), 1);
    assert_policy_summary_read_model(&control_policies[0], mapped_policy, Some(false));

    assert_control_read_model(&controls[1], standalone_control);
    assert_eq!(controls[1]["framework_requirements"], json!([]));
    assert_eq!(controls[1]["evidence"], json!([]));
    assert_eq!(controls[1]["policies"], json!([]));

    let policies = body["policies"].as_array().expect("policies is an array");
    assert_eq!(policies.len(), 2);
    assert_eq!(
        policies
            .iter()
            .map(|policy| policy["name"].as_str().expect("policy name is text"))
            .collect::<Vec<_>>(),
        ["Mapped Policy", "Unmapped Policy"]
    );
    assert_policy_read_model(
        &policies[0],
        mapped_policy,
        &[mapped_control],
        Some((&pending_policy_document, false)),
    );
    assert_policy_read_model(&policies[1], unmapped_policy, &[], None);

    assert_eq!(portal_logs.len(), 2);
    assert_portal_read_audit_event(
        &portal_logs[0],
        "auditor_portal.read",
        "read_auditor_portal",
        workspace_id,
        auditor_email,
        portal_request_id,
    );
    assert_portal_read_audit_event(
        &portal_logs[1],
        "auditor_policy_catalog.read",
        "read_auditor_policy_catalog",
        workspace_id,
        auditor_email,
        portal_request_id,
    );
    assert_eq!(
        portal_logs[0]["fields"]["object_id"],
        portal_logs[1]["fields"]["object_id"]
    );
}
