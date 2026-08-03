use super::{assertions::*, *};

#[tokio::test]
async fn policy_pages_render_every_document_label_download_rule_and_safe_catalog_logs_audit_event()
{
    let app = harness::app().await;
    let subject = "auth0|auditor-browser-policies";
    let workspace_name = "Auditor Browser Policies";
    let auditor_email = "auditor-browser-policies@example.com";

    let scenario = ScenarioBuilder::new(&app)
        .with_user(subject)
        .with_workspace(subject, workspace_name)
        .with_control(workspace_name, "PP-POL-01", "Policy governance")
        .with_policy(workspace_name, "Alpha Uploading")
        .with_policy(workspace_name, "Bravo Scanning")
        .with_policy(workspace_name, "Charlie Upload failed")
        .with_policy(workspace_name, "Delta Uploaded")
        .with_policy_document(
            workspace_name,
            "Delta Uploaded",
            "approved-policy.txt",
            b"approved policy",
        )
        .with_policy(workspace_name, "Echo Missing")
        .with_policy_control_mapping(workspace_name, "Alpha Uploading", "PP-POL-01")
        .with_policy_control_mapping(workspace_name, "Bravo Scanning", "PP-POL-01")
        .with_policy_control_mapping(workspace_name, "Charlie Upload failed", "PP-POL-01")
        .with_policy_control_mapping(workspace_name, "Delta Uploaded", "PP-POL-01")
        .with_policy_control_mapping(workspace_name, "Echo Missing", "PP-POL-01")
        .build()
        .await;
    let workspace = scenario.workspace(workspace_name);
    let workspace_id = workspace.id;
    let standalone = workspace.control("PP-POL-01");
    let uploading_policy = workspace.policy("Alpha Uploading");
    let scanning_policy = workspace.policy("Bravo Scanning");
    let failed_policy = workspace.policy("Charlie Upload failed");
    let uploaded_policy = workspace.policy("Delta Uploaded");
    let missing_policy = workspace.policy("Echo Missing");
    let uploaded_document = uploaded_policy.document();
    let standalone_path = format!("/auditor-access/portal/controls/{}", standalone.id);

    let owner_token = authorize_agent_connection(
        &app,
        subject,
        "Auditor Browser Policy Owner",
        &WorkspacePermission::ALL,
    )
    .await;
    let owner = McpClient::connect(app.mcp_server(), &owner_token).await;

    let uploading_grant = owner
        .call_tool(
            "manage_policy_document",
            json!({ "policy_id": uploading_policy.id }),
        )
        .await;
    let uploading_redeemed = app
        .app_server()
        .get(&local_path(
            uploading_grant["url"]
                .as_str()
                .expect("uploading grant URL is text"),
        ))
        .await;
    uploading_redeemed.assert_status(StatusCode::SEE_OTHER);
    let uploading_cookie = request_cookie(
        uploading_redeemed
            .header("set-cookie")
            .to_str()
            .expect("uploading cookie is text"),
    );
    let uploading_request_id = Uuid::new_v4();
    let mut uploading_gate = app
        .pipeline_controls()
        .hold(DOCUMENT_SCAN_REQUESTED, uploading_request_id);
    let mut uploading_events = app.pipeline_events().subscribe();
    app.app_server()
        .post(POLICY_UPLOAD_PATH)
        .add_header("cookie", uploading_cookie)
        .add_header(REQUEST_ID_HEADER, uploading_request_id.to_string())
        .multipart(upload_form(b"uploading policy", "uploading-policy.txt"))
        .await
        .assert_status(StatusCode::SEE_OTHER);
    let uploading_interception = uploading_gate.await_interception().await;
    let uploading_read = owner
        .call_tool("get_policy", json!({ "policy_id": uploading_policy.id }))
        .await;
    assert_eq!(uploading_read["document"]["upload_status"], "pending");
    let uploading_document_id = Uuid::parse_str(
        uploading_read["document"]["id"]
            .as_str()
            .expect("uploading document id is text"),
    )
    .expect("uploading document id is a UUID");
    assert_eq!(
        uploading_interception.aggregate_id,
        uploading_document_id.to_string()
    );

    let scanning_grant = owner
        .call_tool(
            "manage_policy_document",
            json!({ "policy_id": scanning_policy.id }),
        )
        .await;
    let scanning_redeemed = app
        .app_server()
        .get(&local_path(
            scanning_grant["url"]
                .as_str()
                .expect("scanning grant URL is text"),
        ))
        .await;
    scanning_redeemed.assert_status(StatusCode::SEE_OTHER);
    let scanning_cookie = request_cookie(
        scanning_redeemed
            .header("set-cookie")
            .to_str()
            .expect("scanning cookie is text"),
    );
    let scanning_request_id = Uuid::new_v4();
    let mut scanning_gate = app
        .pipeline_controls()
        .hold(DOCUMENT_FINALIZATION_REQUESTED, scanning_request_id);
    let mut scanning_events = app.pipeline_events().subscribe();
    app.app_server()
        .post(POLICY_UPLOAD_PATH)
        .add_header("cookie", scanning_cookie)
        .add_header(REQUEST_ID_HEADER, scanning_request_id.to_string())
        .multipart(upload_form(b"scanning policy", "scanning-policy.txt"))
        .await
        .assert_status(StatusCode::SEE_OTHER);
    let scanning_read = owner
        .call_tool("get_policy", json!({ "policy_id": scanning_policy.id }))
        .await;
    let scanning_document_id = Uuid::parse_str(
        scanning_read["document"]["id"]
            .as_str()
            .expect("scanning document id is text"),
    )
    .expect("scanning document id is a UUID");
    assert_eq!(
        scanning_events
            .await_event(DOCUMENT_SCAN_REQUESTED, &scanning_document_id.to_string())
            .await,
        StatusCode::NO_CONTENT
    );
    let scanning_interception = scanning_gate.await_interception().await;
    assert_eq!(
        scanning_interception.aggregate_id,
        scanning_document_id.to_string()
    );
    let scanning_read = owner
        .call_tool("get_policy", json!({ "policy_id": scanning_policy.id }))
        .await;
    assert_eq!(scanning_read["document"]["upload_status"], "finalizing");

    let failed_grant = owner
        .call_tool(
            "manage_policy_document",
            json!({ "policy_id": failed_policy.id }),
        )
        .await;
    let failed_redeemed = app
        .app_server()
        .get(&local_path(
            failed_grant["url"]
                .as_str()
                .expect("failed grant URL is text"),
        ))
        .await;
    failed_redeemed.assert_status(StatusCode::SEE_OTHER);
    let failed_cookie = request_cookie(
        failed_redeemed
            .header("set-cookie")
            .to_str()
            .expect("failed cookie is text"),
    );
    let mut failed_events = app.pipeline_events().subscribe();
    app.app_server()
        .post(POLICY_UPLOAD_PATH)
        .add_header("cookie", failed_cookie)
        .multipart(upload_form(EICAR, "failed-policy.txt"))
        .await
        .assert_status(StatusCode::SEE_OTHER);
    let failed_read = owner
        .call_tool("get_policy", json!({ "policy_id": failed_policy.id }))
        .await;
    let failed_document_id = Uuid::parse_str(
        failed_read["document"]["id"]
            .as_str()
            .expect("failed document id is text"),
    )
    .expect("failed document id is a UUID");
    assert_eq!(
        failed_events
            .await_event(DOCUMENT_SCAN_REQUESTED, &failed_document_id.to_string())
            .await,
        StatusCode::NO_CONTENT
    );
    let failed_read = owner
        .call_tool("get_policy", json!({ "policy_id": failed_policy.id }))
        .await;
    assert_eq!(failed_read["document"]["upload_status"], "contains_virus");

    let access_token = authorize_agent_connection(
        &app,
        subject,
        "Auditor Browser Policy Access",
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
        "auth0|auditor-browser-policies-identity",
        auditor_email,
    )
    .await;

    let ((catalog, catalog_request_id), catalog_logs) = app
        .capture_audit_logs(async |request_id| {
            let response = app
                .app_server()
                .get("/auditor-access/portal/policies")
                .add_header("cookie", auditor_cookie.clone())
                .add_header(REQUEST_ID_HEADER, request_id.to_string())
                .await;
            (response, request_id)
        })
        .await;
    catalog.assert_status_ok();
    let policy_rows = [
        policy_row(
            uploading_policy.id,
            "Alpha Uploading",
            1,
            "gap",
            "Uploading",
        ),
        policy_row(scanning_policy.id, "Bravo Scanning", 1, "gap", "Scanning"),
        policy_row(
            failed_policy.id,
            "Charlie Upload failed",
            1,
            "gap",
            "Upload failed",
        ),
        policy_row(
            uploaded_policy.id,
            "Delta Uploaded",
            1,
            "available",
            "Uploaded",
        ),
        policy_row(missing_policy.id, "Echo Missing", 1, "gap", "Missing"),
    ]
    .join("");
    assert_eq!(
        body_projection(&catalog.text()),
        policies_body(workspace_name, auditor_email, 5, 5, &policy_rows)
    );
    assert_eq!(catalog_logs.len(), 2);
    assert_portal_read_audit_event(
        &catalog_logs[0],
        "auditor_portal.read",
        "read_auditor_portal",
        workspace_id,
        auditor_email,
        catalog_request_id,
    );
    assert_portal_read_audit_event(
        &catalog_logs[1],
        "auditor_policy_catalog.read",
        "read_auditor_policy_catalog",
        workspace_id,
        auditor_email,
        catalog_request_id,
    );
    assert_eq!(
        catalog_logs[0]["fields"]["object_id"],
        catalog_logs[1]["fields"]["object_id"]
    );

    for (policy_id, name, status, document) in [
        (uploading_policy.id, "Alpha Uploading", "Uploading", None),
        (scanning_policy.id, "Bravo Scanning", "Scanning", None),
        (
            failed_policy.id,
            "Charlie Upload failed",
            "Upload failed",
            None,
        ),
        (
            uploaded_policy.id,
            "Delta Uploaded",
            "Uploaded",
            Some((uploaded_document.document_id, "approved-policy.txt")),
        ),
        (missing_policy.id, "Echo Missing", "Missing", None),
    ] {
        let response = app
            .app_server()
            .get(&format!("/auditor-access/portal/policies/{policy_id}"))
            .add_header("cookie", auditor_cookie.clone())
            .await;
        response.assert_status_ok();
        assert_eq!(
            body_projection(&response.text()),
            policy_body(
                workspace_name,
                auditor_email,
                policy_id,
                name,
                "No description",
                status,
                document,
                &standalone_path,
                "PP-POL-01",
                "Policy governance",
            )
        );
    }

    let standalone_response = app
        .app_server()
        .get(&format!(
            "/auditor-access/portal/controls/{}",
            standalone.id
        ))
        .add_header("cookie", auditor_cookie)
        .await;
    standalone_response.assert_status_ok();
    let attached = attached_policies(&[
        (uploading_policy.id, "Alpha Uploading", "gap", "Uploading"),
        (scanning_policy.id, "Bravo Scanning", "gap", "Scanning"),
        (
            failed_policy.id,
            "Charlie Upload failed",
            "gap",
            "Upload failed",
        ),
        (
            uploaded_policy.id,
            "Delta Uploaded",
            "available",
            "Uploaded",
        ),
        (missing_policy.id, "Echo Missing", "gap", "Missing"),
    ]);
    assert_eq!(
        body_projection(&standalone_response.text()),
        control_body(
            workspace_name,
            auditor_email,
            None,
            standalone.id,
            "PP-POL-01",
            "Policy governance",
            "Implement Policy governance.",
            0,
            0,
            5,
            &attached,
            &no_evidence(),
        )
    );

    uploading_gate.release();
    assert_eq!(
        uploading_events
            .await_event(DOCUMENT_SCAN_REQUESTED, &uploading_document_id.to_string())
            .await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        uploading_events
            .await_event(
                DOCUMENT_FINALIZATION_REQUESTED,
                &uploading_document_id.to_string(),
            )
            .await,
        StatusCode::NO_CONTENT
    );
    scanning_gate.release();
    assert_eq!(
        scanning_events
            .await_event(
                DOCUMENT_FINALIZATION_REQUESTED,
                &scanning_document_id.to_string(),
            )
            .await,
        StatusCode::NO_CONTENT
    );
}
