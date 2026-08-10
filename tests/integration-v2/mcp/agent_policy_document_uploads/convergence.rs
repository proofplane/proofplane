use super::{helpers::*, *};

#[tokio::test]
async fn matching_policy_replay_tracks_the_lifecycle_and_rejects_completed_mismatches() {
    let app = harness::app().await;
    let subject = "auth0|agent-policy-matching-replay";
    let workspace_name = "Agent Policy Matching Replay";
    let policy_name = "Replay policy";
    let filename = "matching-policy-replay.txt";
    let bytes = b"matching replay policy";
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
        authorize_agent_connection(&app, subject, "Policy Matching Replay Agent", PERMISSIONS)
            .await;
    let connection_id =
        get_agent_connection_id_for(&app, subject, "Policy Matching Replay Agent").await;
    let client = McpClient::connect(app.mcp_server(), &token).await;
    let prepared = client
        .call_tool(
            "prepare_policy_document_upload",
            json!({
                "policy_id": policy.id,
                "filename": filename,
                "content_type": CONTENT_TYPE,
                "content_length": bytes.len(),
                "checksum_sha256": sha256(bytes),
            }),
        )
        .await;
    let descriptor = policy_machine_transfer(&prepared);
    let mut events = app.pipeline_events().subscribe();

    let ((created, pending_replay, mismatch, scan_gate, mut finalization_gate, request_id), logs) =
        app.capture_audit_logs(async |request_id| {
            let mut scan_gate = app
                .pipeline_controls()
                .hold(DOCUMENT_SCAN_REQUESTED, request_id);
            let finalization_gate = app
                .pipeline_controls()
                .hold(DOCUMENT_FINALIZATION_REQUESTED, request_id);
            let created = execute_transfer(&app, &descriptor, bytes, request_id).await;
            let scan_interception = scan_gate.await_interception().await;
            assert_eq!(scan_interception.aggregate_id, created.body["document_id"]);
            let pending_replay = execute_transfer(&app, &descriptor, bytes, request_id).await;
            let mismatch = fail_transfer_on_purpose(
                &app,
                &descriptor.path,
                Some(&descriptor.authorization),
                Some("application/pdf"),
                Some(bytes.len() as u64),
                bytes,
                request_id,
            )
            .await;

            (
                created,
                pending_replay,
                mismatch,
                scan_gate,
                finalization_gate,
                request_id,
            )
        })
        .await;
    let document_id = assert_pending_result(&created, StatusCode::CREATED, policy.id);
    assert_eq!(pending_replay.status, StatusCode::OK);
    assert_eq!(pending_replay.body, created.body);
    assert_http_error(
        &mismatch,
        StatusCode::BAD_REQUEST,
        "bad_request",
        "request validation failed",
        json!(["content-type header does not match upload grant"]),
    );
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
        descriptor.upload_id,
        json!({
            "policy_document_id": document_id,
            "policy_id": policy.id,
            "lifecycle_status": "pending",
        }),
    );

    scan_gate.release();
    assert_eq!(
        events
            .await_event(DOCUMENT_SCAN_REQUESTED, &document_id.to_string())
            .await,
        StatusCode::NO_CONTENT
    );
    let finalizing_interception = finalization_gate.await_interception().await;
    assert_eq!(
        finalizing_interception.aggregate_id,
        document_id.to_string()
    );
    let finalizing_replay = execute_transfer(&app, &descriptor, bytes, request_id).await;
    assert_eq!(finalizing_replay.status, StatusCode::OK);
    assert_eq!(
        finalizing_replay.body,
        json!({
            "policy_id": policy.id,
            "document_id": document_id,
            "upload_status": "finalizing",
        })
    );
    let current = client
        .call_tool("get_policy", json!({ "policy_id": policy.id }))
        .await;
    assert_eq!(
        assert_policy_read_model(
            &current,
            policy,
            Some(ExpectedPolicyDocument {
                user_id,
                filename,
                bytes,
                upload_status: "finalizing",
            }),
        ),
        Some(document_id)
    );

    finalization_gate.release();
    assert_eq!(
        events
            .await_event(DOCUMENT_FINALIZATION_REQUESTED, &document_id.to_string())
            .await,
        StatusCode::NO_CONTENT
    );
}

#[tokio::test]
async fn concurrent_matching_policy_transfers_converge_on_one_document() {
    let app = harness::app().await;
    let subject = "auth0|agent-policy-concurrent";
    let workspace_name = "Agent Policy Concurrent";
    let policy_name = "Concurrent machine policy";
    let filename = "concurrent-policy.txt";
    let bytes = b"concurrent machine policy";
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
        authorize_agent_connection(&app, subject, "Concurrent Policy Agent", PERMISSIONS).await;
    let connection_id = get_agent_connection_id_for(&app, subject, "Concurrent Policy Agent").await;
    let client = McpClient::connect(app.mcp_server(), &token).await;
    let prepared = client
        .call_tool(
            "prepare_policy_document_upload",
            json!({
                "policy_id": policy.id,
                "filename": filename,
                "content_type": CONTENT_TYPE,
                "content_length": bytes.len(),
                "checksum_sha256": sha256(bytes),
            }),
        )
        .await;
    let descriptor = policy_machine_transfer(&prepared);
    let mut events = app.pipeline_events().subscribe();

    let (((left, right), gate, request_id), logs) = app
        .capture_audit_logs(async |request_id| {
            let mut gate = app
                .pipeline_controls()
                .hold(DOCUMENT_SCAN_REQUESTED, request_id);
            let results = tokio::join!(
                execute_transfer(&app, &descriptor, bytes, request_id),
                execute_transfer(&app, &descriptor, bytes, request_id),
            );
            let interception = gate.await_interception().await;
            assert_eq!(interception.aggregate_id, results.0.body["document_id"]);
            (results, gate, request_id)
        })
        .await;
    let mut statuses = [left.status, right.status];
    statuses.sort_by_key(|status| status.as_u16());
    assert_eq!(statuses, [StatusCode::OK, StatusCode::CREATED]);
    assert_eq!(left.body, right.body);
    let document_id = assert_pending_result(&left, left.status, policy.id);
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
        descriptor.upload_id,
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
        assert_policy_read_model(
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
