use super::{assertions::*, *};

const EVIDENCE_UPLOAD_PATH: &str = "/evidence-document-uploads/files";

#[tokio::test]
async fn framework_pages_render_ordered_counts_coverage_breadcrumbs_and_document_actions() {
    let app = harness::app().await;
    let subject = "auth0|auditor-browser-framework";
    let workspace_name = "Auditor Browser Framework";
    let auditor_email = "auditor-browser-framework@example.com";

    let scenario = ScenarioBuilder::new(&app)
        .with_user(subject)
        .with_workspace(subject, workspace_name)
        .with_evidence(workspace_name, "Access review evidence")
        .with_evidence_document(
            workspace_name,
            "Access review evidence",
            "uploaded-access-review.txt",
            b"uploaded access review",
            PERIOD_START,
            PERIOD_END,
        )
        .with_evidence(workspace_name, "Monitoring evidence")
        .with_control(workspace_name, "PP-BR-01", "Access reviews")
        .with_control_framework_requirement(workspace_name, "PP-BR-01", "example", "REQ-1")
        .with_control(workspace_name, "PP-BR-02", "Access approvals")
        .with_control_framework_requirement(workspace_name, "PP-BR-02", "example", "REQ-1")
        .with_control(workspace_name, "PP-BR-03", "Security monitoring")
        .with_control_framework_requirement(workspace_name, "PP-BR-03", "example", "REQ-3")
        .with_evidence_control_mapping(
            workspace_name,
            "Access review evidence",
            "PP-BR-01",
            "Shows access reviews were performed.",
        )
        .with_evidence_control_mapping(
            workspace_name,
            "Monitoring evidence",
            "PP-BR-03",
            "Shows security monitoring coverage.",
        )
        .build()
        .await;
    let workspace = scenario.workspace(workspace_name);
    let workspace_id = workspace.id;
    let access_review = workspace.evidence("Access review evidence");
    let uploaded = access_review.submission("uploaded-access-review.txt");
    let access_control = workspace.control("PP-BR-01");
    let approval_control = workspace.control("PP-BR-02");
    let monitoring_control = workspace.control("PP-BR-03");
    let framework = scenario.framework("example");
    let req1 = framework.requirement("REQ-1");
    let req3 = framework.requirement("REQ-3");

    let owner_token = authorize_agent_connection(
        &app,
        subject,
        "Auditor Browser Framework Owner",
        &WorkspacePermission::ALL,
    )
    .await;
    let owner = McpClient::connect(app.mcp_server(), &owner_token).await;
    let evidence_grant = owner
        .call_tool(
            "manage_evidence_submissions",
            json!({
                "evidence_id": access_review.id,
                "valid_from": PERIOD_START,
                "valid_until": PERIOD_END,
            }),
        )
        .await;
    let evidence_redeemed = app
        .app_server()
        .get(&local_path(
            evidence_grant["url"]
                .as_str()
                .expect("evidence grant URL is text"),
        ))
        .await;
    evidence_redeemed.assert_status(StatusCode::SEE_OTHER);
    let evidence_cookie = request_cookie(
        evidence_redeemed
            .header("set-cookie")
            .to_str()
            .expect("evidence cookie is text"),
    );
    let pending_request_id = Uuid::new_v4();
    let mut pending_gate = app
        .pipeline_controls()
        .hold(DOCUMENT_SCAN_REQUESTED, pending_request_id);
    let mut pending_events = app.pipeline_events().subscribe();
    app.app_server()
        .post(EVIDENCE_UPLOAD_PATH)
        .add_header("cookie", evidence_cookie)
        .add_header(REQUEST_ID_HEADER, pending_request_id.to_string())
        .multipart(upload_form(
            b"pending access review",
            "pending-access-review.txt",
        ))
        .await
        .assert_status(StatusCode::SEE_OTHER);
    let pending_interception = pending_gate.await_interception().await;
    let pending_read_model = owner
        .call_tool(
            "list_evidence_submissions",
            json!({ "evidence_id": access_review.id }),
        )
        .await["submissions"]
        .as_array()
        .expect("submissions is an array")
        .iter()
        .find(|submission| submission["document"]["filename"] == "pending-access-review.txt")
        .expect("pending submission is listed")
        .clone();
    let pending = TestEvidenceSubmission::from_mcp(&pending_read_model);
    assert_eq!(
        pending_interception.aggregate_id,
        pending.document_id.to_string()
    );
    assert_eq!(pending.upload_status, "pending");

    let access_token = authorize_agent_connection(
        &app,
        subject,
        "Auditor Browser Framework Access",
        &[WorkspacePermission::ManageAuditorAccess],
    )
    .await;
    let access_client = McpClient::connect(app.mcp_server(), &access_token).await;
    let created = access_client
        .call_tool(
            "create_auditor_access_link",
            json!({
                "email": auditor_email,
                "expires_at": EXPIRES_AT,
                "period_start": PERIOD_START,
                "period_end": PERIOD_END,
            }),
        )
        .await;
    let invite_url = Url::parse(created["url"].as_str().expect("invite URL is text"))
        .expect("invite URL parses");
    let invite_token = invite_token(&invite_url);
    app.app_server()
        .get(&local_path(invite_url.as_str()))
        .await
        .assert_status_ok();
    let auditor_cookie = authenticate_auditor(
        &app,
        workspace_id,
        &invite_token,
        "auth0|auditor-browser-framework-identity",
        auditor_email,
    )
    .await;

    let portal = app
        .app_server()
        .get("/auditor-access/portal")
        .add_header("cookie", auditor_cookie.clone())
        .await;
    portal.assert_status_ok();
    let portal_rows = [
        requirement_row(req1, 2, 1, 2, "available", "Evidence on file"),
        requirement_row(req3, 1, 1, 0, "gap", "Awaiting submission"),
    ]
    .join("");
    assert_eq!(
        body_read_model(&portal.text()),
        portal_body(workspace_name, auditor_email, 2, 3, &portal_rows)
    );

    let req1_response = app
        .app_server()
        .get(&format!(
            "/auditor-access/portal/framework-requirements/{}",
            req1.id
        ))
        .add_header("cookie", auditor_cookie.clone())
        .await;
    req1_response.assert_status_ok();
    let req1_rows = [
        control_row(
            req1.id,
            access_control.id,
            "PP-BR-01",
            "Access reviews",
            1,
            2,
            "available",
            "Evidence on file",
        ),
        control_row(
            req1.id,
            approval_control.id,
            "PP-BR-02",
            "Access approvals",
            0,
            0,
            "gap",
            "No evidence mapped",
        ),
    ]
    .join("");
    assert_eq!(
        body_read_model(&req1_response.text()),
        requirement_body(workspace_name, auditor_email, req1, 2, &req1_rows)
    );

    let req3_response = app
        .app_server()
        .get(&format!(
            "/auditor-access/portal/framework-requirements/{}",
            req3.id
        ))
        .add_header("cookie", auditor_cookie.clone())
        .await;
    req3_response.assert_status_ok();
    let req3_rows = control_row(
        req3.id,
        monitoring_control.id,
        "PP-BR-03",
        "Security monitoring",
        1,
        0,
        "gap",
        "Awaiting submission",
    );
    assert_eq!(
        body_read_model(&req3_response.text()),
        requirement_body(workspace_name, auditor_email, req3, 1, &req3_rows)
    );

    let control_response = app
        .app_server()
        .get(&format!(
            "/auditor-access/portal/framework-requirements/{}/controls/{}",
            req1.id, access_control.id
        ))
        .add_header("cookie", auditor_cookie)
        .await;
    control_response.assert_status_ok();
    let submissions = [
        submission_row(&pending, false),
        submission_row(uploaded, true),
    ]
    .join("");
    let evidence = evidence_block(
        "Access review evidence",
        "Collect Access review evidence.",
        2,
        &submissions,
    );
    assert_eq!(
        body_read_model(&control_response.text()),
        control_body(
            workspace_name,
            auditor_email,
            Some((req1.id, "REQ-1")),
            access_control.id,
            "PP-BR-01",
            "Access reviews",
            "Implement Access reviews.",
            1,
            2,
            0,
            &no_policies(),
            &evidence,
        )
    );

    pending_gate.release();
    assert_eq!(
        pending_events
            .await_event(DOCUMENT_SCAN_REQUESTED, &pending.document_id.to_string())
            .await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        pending_events
            .await_event(
                DOCUMENT_FINALIZATION_REQUESTED,
                &pending.document_id.to_string(),
            )
            .await,
        StatusCode::NO_CONTENT
    );
}
