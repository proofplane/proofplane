use super::{helpers::*, *};

#[tokio::test]
async fn competing_machine_grants_allow_the_loser_after_the_winner_is_archived() {
    let app = harness::app().await;
    let subject = "auth0|agent-policy-competing-grants";
    let workspace_name = "Agent Policy Competing Grants";
    let policy_name = "Competing grants policy";
    let first_filename = "first-machine-policy.txt";
    let first_bytes = b"first competing machine policy";
    let second_filename = "second-machine-policy.txt";
    let second_bytes = b"second competing machine policy";
    let scenario = ScenarioBuilder::new(&app)
        .with_user(subject)
        .with_workspace(subject, workspace_name)
        .with_policy(workspace_name, policy_name)
        .build()
        .await;
    let user_id = scenario.user(subject).id;
    let workspace = scenario.workspace(workspace_name);
    let policy = workspace.policy(policy_name);
    let token =
        authorize_agent_connection(&app, subject, "Competing Policy Grant Agent", PERMISSIONS)
            .await;
    let connection_id =
        get_agent_connection_id_for(&app, subject, "Competing Policy Grant Agent").await;
    let client = McpClient::connect(app.mcp_server(), &token).await;
    let first_prepared = client
        .call_tool(
            "prepare_policy_document_upload",
            json!({
                "policy_id": policy.id,
                "filename": first_filename,
                "content_type": CONTENT_TYPE,
                "content_length": first_bytes.len(),
                "checksum_sha256": sha256(first_bytes),
            }),
        )
        .await;
    let second_prepared = client
        .call_tool(
            "prepare_policy_document_upload",
            json!({
                "policy_id": policy.id,
                "filename": second_filename,
                "content_type": CONTENT_TYPE,
                "content_length": second_bytes.len(),
                "checksum_sha256": sha256(second_bytes),
            }),
        )
        .await;
    let first = policy_machine_transfer(&first_prepared);
    let second = policy_machine_transfer(&second_prepared);
    let mut events = app.pipeline_events().subscribe();

    let (((left, right), gate, request_id), logs) = app
        .capture_audit_logs(async |request_id| {
            let mut gate = app
                .pipeline_controls()
                .hold(DOCUMENT_SCAN_REQUESTED, request_id);
            let results = tokio::join!(
                execute_transfer(&app, &first, first_bytes, request_id),
                execute_transfer(&app, &second, second_bytes, request_id),
            );
            let interception = gate.await_interception().await;
            let winner = if results.0.status == StatusCode::CREATED {
                &results.0
            } else {
                &results.1
            };
            assert_eq!(interception.aggregate_id, winner.body["document_id"]);
            (results, gate, request_id)
        })
        .await;
    let statuses = [left.status, right.status];
    assert_eq!(
        statuses
            .iter()
            .filter(|status| **status == StatusCode::CREATED)
            .count(),
        1
    );
    assert_eq!(
        statuses
            .iter()
            .filter(|status| **status == StatusCode::CONFLICT)
            .count(),
        1
    );
    let (
        winner,
        loser,
        winner_descriptor,
        loser_descriptor,
        winner_filename,
        winner_bytes,
        loser_filename,
        loser_bytes,
    ) = if left.status == StatusCode::CREATED {
        (
            &left,
            &right,
            &first,
            &second,
            first_filename,
            first_bytes.as_slice(),
            second_filename,
            second_bytes.as_slice(),
        )
    } else {
        (
            &right,
            &left,
            &second,
            &first,
            second_filename,
            second_bytes.as_slice(),
            first_filename,
            first_bytes.as_slice(),
        )
    };
    let document_id = assert_pending_result(winner, StatusCode::CREATED, policy.id);
    assert_policy_conflict(loser);
    assert_eq!(logs.len(), 1);
    assert_upload_audit_event(
        &logs[0],
        request_id,
        "agent_policy_document_upload.completed",
        "rest",
        "upload_agent_policy_document",
        user_id,
        connection_id,
        workspace.id,
        "agent_policy_document_upload_grant",
        winner_descriptor.upload_id,
        json!({
            "policy_document_id": document_id,
            "policy_id": policy.id,
            "lifecycle_status": "pending",
        }),
    );
    let current = client
        .call_tool("get_policy", json!({ "policy_id": policy.id }))
        .await;
    assert_eq!(
        assert_policy_projection(
            &current,
            policy,
            Some(ExpectedPolicyDocument {
                user_id,
                filename: winner_filename,
                bytes: winner_bytes,
                upload_status: "pending",
            }),
        ),
        Some(document_id)
    );

    let (losing_retry, retry_logs) = app
        .capture_audit_logs(async |retry_request_id| {
            execute_transfer(&app, loser_descriptor, loser_bytes, retry_request_id).await
        })
        .await;
    assert_policy_conflict(&losing_retry);
    assert!(retry_logs.is_empty());
    assert_eq!(
        client
            .call_tool("get_policy", json!({ "policy_id": policy.id }))
            .await,
        current
    );

    gate.release();
    assert_eq!(
        events
            .await_event(DOCUMENT_SCAN_REQUESTED, &document_id.to_string())
            .await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        events
            .await_event(DOCUMENT_FINALIZATION_REQUESTED, &document_id.to_string())
            .await,
        StatusCode::NO_CONTENT
    );

    let uploaded = client
        .call_tool("get_policy", json!({ "policy_id": policy.id }))
        .await;
    assert_eq!(
        assert_policy_projection(
            &uploaded,
            policy,
            Some(ExpectedPolicyDocument {
                user_id,
                filename: winner_filename,
                bytes: winner_bytes,
                upload_status: "uploaded",
            }),
        ),
        Some(document_id)
    );

    let archive_token = authorize_agent_connection(
        &app,
        subject,
        "Competing Policy Browser Manager",
        &WorkspacePermission::ALL,
    )
    .await;
    let archive_connection_id =
        get_agent_connection_id_for(&app, subject, "Competing Policy Browser Manager").await;
    let archive_client = McpClient::connect(app.mcp_server(), &archive_token).await;
    let management_grant = archive_client
        .call_tool("manage_policy_document", json!({ "policy_id": policy.id }))
        .await;
    let redeemed = app
        .app_server()
        .get(&local_path(
            management_grant["url"]
                .as_str()
                .expect("browser management grant URL is text"),
        ))
        .await;
    redeemed.assert_status(StatusCode::SEE_OTHER);
    let cookie = request_cookie(
        redeemed
            .header("set-cookie")
            .to_str()
            .expect("browser policy cookie is text"),
    );
    let ((archived, archive_request_id), archive_logs) = app
        .capture_audit_logs(async |request_id| {
            let response = app
                .app_server()
                .post(&format!("{BROWSER_UPLOAD_PATH}/{document_id}/archive"))
                .add_header("cookie", cookie)
                .add_header(REQUEST_ID_HEADER, request_id.to_string())
                .await;
            (response, request_id)
        })
        .await;
    archived.assert_status(StatusCode::SEE_OTHER);
    assert_eq!(archived.header("location"), MANAGEMENT_PATH);
    assert_eq!(archive_logs.len(), 1);
    assert_upload_audit_event(
        &archive_logs[0],
        archive_request_id,
        "policy_document.archived",
        "rest",
        "archive_policy_document",
        user_id,
        archive_connection_id,
        workspace.id,
        "policy_document",
        document_id,
        json!({
            "policy_document_id": document_id,
            "policy_id": policy.id,
        }),
    );
    let empty = client
        .call_tool("get_policy", json!({ "policy_id": policy.id }))
        .await;
    assert_eq!(assert_policy_projection(&empty, policy, None), None);

    let mut replacement_events = app.pipeline_events().subscribe();
    let ((replacement, replacement_request_id), replacement_logs) = app
        .capture_audit_logs(async |request_id| {
            (
                execute_transfer(&app, loser_descriptor, loser_bytes, request_id).await,
                request_id,
            )
        })
        .await;
    let replacement_id = assert_pending_result(&replacement, StatusCode::CREATED, policy.id);
    assert_ne!(replacement_id, document_id);
    assert_eq!(replacement_logs.len(), 1);
    assert_upload_audit_event(
        &replacement_logs[0],
        replacement_request_id,
        "agent_policy_document_upload.completed",
        "rest",
        "upload_agent_policy_document",
        user_id,
        connection_id,
        workspace.id,
        "agent_policy_document_upload_grant",
        loser_descriptor.upload_id,
        json!({
            "policy_document_id": replacement_id,
            "policy_id": policy.id,
            "lifecycle_status": "pending",
        }),
    );
    assert_eq!(
        replacement_events
            .await_event(DOCUMENT_SCAN_REQUESTED, &replacement_id.to_string())
            .await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        replacement_events
            .await_event(DOCUMENT_FINALIZATION_REQUESTED, &replacement_id.to_string(),)
            .await,
        StatusCode::NO_CONTENT
    );
    let replacement_uploaded = client
        .call_tool("get_policy", json!({ "policy_id": policy.id }))
        .await;
    assert_eq!(
        assert_policy_projection(
            &replacement_uploaded,
            policy,
            Some(ExpectedPolicyDocument {
                user_id,
                filename: loser_filename,
                bytes: loser_bytes,
                upload_status: "uploaded",
            }),
        ),
        Some(replacement_id)
    );
}

#[tokio::test]
async fn machine_and_browser_transfers_choose_one_current_document() {
    let app = harness::app().await;
    let subject = "auth0|agent-policy-machine-browser-race";
    let workspace_name = "Agent Policy Machine Browser Race";
    let policy_name = "Machine browser race policy";
    let machine_filename = "machine-race-policy.txt";
    let machine_bytes = b"machine race policy";
    let browser_filename = "browser-race-policy.txt";
    let browser_bytes = b"browser race policy";
    let scenario = ScenarioBuilder::new(&app)
        .with_user(subject)
        .with_workspace(subject, workspace_name)
        .with_policy(workspace_name, policy_name)
        .build()
        .await;
    let user_id = scenario.user(subject).id;
    let workspace = scenario.workspace(workspace_name);
    let policy = workspace.policy(policy_name);
    let machine_token =
        authorize_agent_connection(&app, subject, "Machine Race Policy Agent", PERMISSIONS).await;
    let browser_token = authorize_agent_connection(
        &app,
        subject,
        "Browser Race Policy Agent",
        &WorkspacePermission::ALL,
    )
    .await;
    let machine_connection_id =
        get_agent_connection_id_for(&app, subject, "Machine Race Policy Agent").await;
    let browser_connection_id =
        get_agent_connection_id_for(&app, subject, "Browser Race Policy Agent").await;
    let machine_client = McpClient::connect(app.mcp_server(), &machine_token).await;
    let machine_prepared = machine_client
        .call_tool(
            "prepare_policy_document_upload",
            json!({
                "policy_id": policy.id,
                "filename": machine_filename,
                "content_type": CONTENT_TYPE,
                "content_length": machine_bytes.len(),
                "checksum_sha256": sha256(machine_bytes),
            }),
        )
        .await;
    let machine = policy_machine_transfer(&machine_prepared);
    let browser_grant = McpClient::connect(app.mcp_server(), &browser_token)
        .await
        .call_tool("manage_policy_document", json!({ "policy_id": policy.id }))
        .await;
    let redeemed = app
        .app_server()
        .get(&local_path(
            browser_grant["url"]
                .as_str()
                .expect("browser grant URL is text"),
        ))
        .await;
    redeemed.assert_status(StatusCode::SEE_OTHER);
    let cookie = request_cookie(
        redeemed
            .header("set-cookie")
            .to_str()
            .expect("browser policy cookie is text"),
    );
    let mut events = app.pipeline_events().subscribe();

    let ((machine_result, browser_result, interception, gate, request_id), logs) = app
        .capture_audit_logs(async |request_id| {
            let mut gate = app
                .pipeline_controls()
                .hold(DOCUMENT_SCAN_REQUESTED, request_id);
            let machine_upload = execute_transfer(&app, &machine, machine_bytes, request_id);
            let browser_upload = app
                .app_server()
                .post(BROWSER_UPLOAD_PATH)
                .add_header("cookie", cookie)
                .add_header(REQUEST_ID_HEADER, request_id.to_string())
                .multipart(upload_form(browser_bytes, browser_filename));
            let (machine_result, browser_result) = tokio::join!(machine_upload, browser_upload);
            let interception = gate.await_interception().await;
            (
                machine_result,
                browser_result,
                interception,
                gate,
                request_id,
            )
        })
        .await;
    assert_eq!(logs.len(), 1);

    let current = machine_client
        .call_tool("get_policy", json!({ "policy_id": policy.id }))
        .await;
    let document_id = uuid_at(&current["document"]["id"], "race winner document id");
    assert_eq!(interception.aggregate_id, document_id.to_string());
    let (filename, bytes) = if machine_result.status == StatusCode::CREATED {
        assert_pending_result(&machine_result, StatusCode::CREATED, policy.id);
        assert_browser_conflict(&browser_result);
        assert_upload_audit_event(
            &logs[0],
            request_id,
            "agent_policy_document_upload.completed",
            "rest",
            "upload_agent_policy_document",
            user_id,
            machine_connection_id,
            workspace.id,
            "agent_policy_document_upload_grant",
            machine.upload_id,
            json!({
                "policy_document_id": document_id,
                "policy_id": policy.id,
                "lifecycle_status": "pending",
            }),
        );
        (machine_filename, machine_bytes.as_slice())
    } else {
        assert_policy_conflict(&machine_result);
        browser_result.assert_status(StatusCode::SEE_OTHER);
        assert_eq!(browser_result.header("location"), MANAGEMENT_PATH);
        assert_upload_audit_event(
            &logs[0],
            request_id,
            "policy_document.accepted",
            "rest",
            "accept_policy_document",
            user_id,
            browser_connection_id,
            workspace.id,
            "policy_document",
            document_id,
            json!({
                "policy_document_id": document_id,
                "policy_id": policy.id,
                "lifecycle_status": "pending",
            }),
        );
        (browser_filename, browser_bytes.as_slice())
    };
    assert_eq!(
        assert_policy_projection(
            &current,
            policy,
            Some(ExpectedPolicyDocument {
                user_id,
                filename,
                bytes,
                upload_status: "pending",
            }),
        ),
        Some(document_id)
    );

    gate.release();
    assert_eq!(
        events
            .await_event(DOCUMENT_SCAN_REQUESTED, &document_id.to_string())
            .await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        events
            .await_event(DOCUMENT_FINALIZATION_REQUESTED, &document_id.to_string())
            .await,
        StatusCode::NO_CONTENT
    );
}
