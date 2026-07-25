use super::{assertions::*, *};

#[tokio::test]
async fn browser_bodies_escape_client_fields_and_pin_validated_portable_filenames() {
    let app = harness::app().await;
    let subject = "auth0|auditor-browser-escaping";
    let workspace_name = "Auditor Browser Escaping";
    let auditor_email = "auditor-browser-escaping@example.com";
    let evidence_title = "Evidence <script>alert('evidence')</script> & review";
    let evidence_filename = "portable-evidence_final.txt";
    let control_title = "Control <strong>owner & reviewer</strong>";
    let policy_name = "Policy <script>alert('policy')</script> & handbook";
    let policy_description = "Read <em>everything</em> & verify \"owners\".";
    let policy_filename = "portable-policy_final.txt";

    let scenario = ScenarioBuilder::new(&app)
        .with_user(subject)
        .with_workspace(subject, workspace_name)
        .with_evidence(workspace_name, evidence_title)
        .with_evidence_document(
            workspace_name,
            evidence_title,
            evidence_filename,
            b"portable evidence",
            PERIOD_START,
            PERIOD_END,
        )
        .with_control(workspace_name, "PP-ESC-01", control_title)
        .with_control_framework_requirement(workspace_name, "PP-ESC-01", "soc2", "CC6.1")
        .with_evidence_control_mapping(
            workspace_name,
            evidence_title,
            "PP-ESC-01",
            "<img src=x onerror=alert('mapping')> mapping rationale",
        )
        .build()
        .await;
    let workspace = scenario.workspace(workspace_name);
    let workspace_id = workspace.id;
    let control = workspace.control("PP-ESC-01");
    let evidence = workspace.evidence(evidence_title);
    let evidence_document = evidence.submission(evidence_filename);
    let cc61 = scenario.framework("soc2").requirement("CC6.1");

    let owner_token = authorize_agent_connection(
        &app,
        subject,
        "Auditor Browser Escaping Owner",
        &WorkspacePermission::ALL,
    )
    .await;
    let owner = McpClient::connect(app.mcp_server(), &owner_token).await;
    let policy = owner
        .call_tool(
            "create_policy",
            json!({
                "name": policy_name,
                "description": policy_description,
                "control_ids": [control.id],
            }),
        )
        .await;
    let policy_id = Uuid::parse_str(policy["id"].as_str().expect("policy id is text"))
        .expect("policy id is a UUID");
    let policy_grant = owner
        .call_tool("manage_policy_document", json!({ "policy_id": policy_id }))
        .await;
    let policy_redeemed = app
        .app_server()
        .get(&local_path(
            policy_grant["url"]
                .as_str()
                .expect("policy grant URL is text"),
        ))
        .await;
    policy_redeemed.assert_status(StatusCode::SEE_OTHER);
    let policy_cookie = request_cookie(
        policy_redeemed
            .header("set-cookie")
            .to_str()
            .expect("policy cookie is text"),
    );
    let mut policy_events = app.pipeline_events().subscribe();
    app.app_server()
        .post(POLICY_UPLOAD_PATH)
        .add_header("cookie", policy_cookie)
        .multipart(upload_form(b"portable policy", policy_filename))
        .await
        .assert_status(StatusCode::SEE_OTHER);
    let pending_policy = owner
        .call_tool("get_policy", json!({ "policy_id": policy_id }))
        .await;
    let policy_document_id = Uuid::parse_str(
        pending_policy["document"]["id"]
            .as_str()
            .expect("policy document id is text"),
    )
    .expect("policy document id is a UUID");
    assert_eq!(
        policy_events
            .await_event(DOCUMENT_SCAN_REQUESTED, &policy_document_id.to_string())
            .await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        policy_events
            .await_event(
                DOCUMENT_FINALIZATION_REQUESTED,
                &policy_document_id.to_string(),
            )
            .await,
        StatusCode::NO_CONTENT
    );
    let settled_policy = owner
        .call_tool("get_policy", json!({ "policy_id": policy_id }))
        .await;
    assert_eq!(settled_policy["document"]["upload_status"], "uploaded");

    let access_token = authorize_agent_connection(
        &app,
        subject,
        "Auditor Browser Escaping Access",
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
    app.app_server()
        .post(&format!(
            "/auditor-access/{workspace_id}/otp/request/browser"
        ))
        .form(&[("token", invite_token.as_str())])
        .await
        .assert_status_ok();
    let sent = app.mailer().sent_mail_for(auditor_email);
    assert_eq!(sent.len(), 1);
    let code = sent[0].code.clone();
    let verified = app
        .app_server()
        .post(&format!(
            "/auditor-access/{workspace_id}/otp/verify/browser"
        ))
        .form(&[("token", invite_token.as_str()), ("code", code.as_str())])
        .await;
    verified.assert_status(StatusCode::SEE_OTHER);
    let auditor_cookie = request_cookie(
        verified
            .header("set-cookie")
            .to_str()
            .expect("auditor cookie is text"),
    );

    let escaped_policy_name = escape_html(policy_name);
    let escaped_policy_description = escape_html(policy_description);
    let escaped_control_title = escape_html(control_title);
    let escaped_control_description = escape_html(&format!("Implement {control_title}."));
    let escaped_evidence_title = escape_html(evidence_title);
    let escaped_evidence_description = escape_html(&format!("Collect {evidence_title}."));
    let control_path = format!(
        "/auditor-access/portal/framework-requirements/{}/controls/{}",
        cc61.id, control.id
    );

    let catalog = app
        .app_server()
        .get("/auditor-access/portal/policies")
        .add_header("cookie", auditor_cookie.clone())
        .await;
    catalog.assert_status_ok();
    let catalog_row = policy_row_with_description(
        policy_id,
        &escaped_policy_name,
        &escaped_policy_description,
        1,
        "available",
        "Uploaded",
    );
    assert_eq!(
        body_projection(&catalog.text()),
        policies_body(workspace_name, auditor_email, 1, 1, &catalog_row)
    );

    let policy_response = app
        .app_server()
        .get(&format!("/auditor-access/portal/policies/{policy_id}"))
        .add_header("cookie", auditor_cookie.clone())
        .await;
    policy_response.assert_status_ok();
    assert_eq!(
        body_projection(&policy_response.text()),
        policy_body(
            workspace_name,
            auditor_email,
            policy_id,
            &escaped_policy_name,
            &escaped_policy_description,
            "Uploaded",
            Some((policy_document_id, policy_filename)),
            &control_path,
            "PP-ESC-01",
            &escaped_control_title,
        )
    );

    let control_response = app
        .app_server()
        .get(&control_path)
        .add_header("cookie", auditor_cookie)
        .await;
    control_response.assert_status_ok();
    let attached = attached_policy_with_description(
        policy_id,
        &escaped_policy_name,
        &escaped_policy_description,
        "available",
        "Uploaded",
    );
    let submissions = submission_row(evidence_document, true);
    let evidence_block = evidence_block(
        &escaped_evidence_title,
        &escaped_evidence_description,
        1,
        &submissions,
    );
    assert_eq!(
        body_projection(&control_response.text()),
        control_body(
            workspace_name,
            auditor_email,
            Some((cc61.id, "CC6.1")),
            control.id,
            "PP-ESC-01",
            &escaped_control_title,
            &escaped_control_description,
            1,
            1,
            1,
            &attached,
            &evidence_block,
        )
    );
}
