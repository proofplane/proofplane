use super::{helpers::*, *};

#[tokio::test]
async fn matching_replay_returns_the_original_upload_without_duplicate_work() {
    let app = harness::app().await;
    let subject = "auth0|agent-evidence-matching-replay";
    let workspace_name = "Agent Evidence Matching Replay";
    let bytes = b"matching replay evidence";
    let scenario = ScenarioBuilder::new(&app)
        .with_user(subject)
        .with_workspace(subject, workspace_name)
        .with_evidence(workspace_name, "Replay evidence")
        .build()
        .await;
    let user_id = scenario.user(subject).id;
    let workspace = scenario.workspace(workspace_name);
    let evidence_id = workspace.evidence("Replay evidence").id;
    let token =
        authorize_agent_connection(&app, subject, "Matching Replay Agent", PERMISSIONS).await;
    let connection_id = get_agent_connection_id_for(&app, subject, "Matching Replay Agent").await;
    let client = McpClient::connect(app.mcp_server(), &token).await;
    let prepared = client
        .call_tool(
            "prepare_evidence_submission_upload",
            json!({
                "evidence_id": evidence_id,
                "valid_from": VALID_FROM,
                "valid_until": VALID_UNTIL,
                "filename": "matching-replay.txt",
                "content_type": CONTENT_TYPE,
                "content_length": bytes.len(),
                "checksum_sha256": sha256(bytes),
            }),
        )
        .await;
    let descriptor = machine_transfer(&prepared, CONTENT_TYPE);
    let mut events = app.pipeline_events().subscribe();

    let ((created, replayed, gate, request_id), logs) = app
        .capture_audit_logs(async |request_id| {
            let mut gate = app
                .pipeline_controls()
                .hold(DOCUMENT_SCAN_REQUESTED, request_id);
            let created = execute_transfer(&app, &descriptor, bytes, request_id).await;
            let interception = gate.await_interception().await;
            assert_eq!(interception.aggregate_id, created.body["document_id"]);
            let replayed = execute_transfer(&app, &descriptor, bytes, request_id).await;
            (created, replayed, gate, request_id)
        })
        .await;
    assert_eq!(created.status, StatusCode::CREATED);
    assert_eq!(replayed.status, StatusCode::OK);
    assert_eq!(replayed.body, created.body);
    let document_id = uuid_at(&created.body["document_id"], "replayed document id");
    assert_eq!(logs.len(), 1);
    assert_upload_audit_event(
        &logs[0],
        request_id,
        "agent_evidence_upload.completed",
        "rest",
        "upload_agent_evidence",
        user_id,
        connection_id,
        workspace.id,
        "agent_evidence_upload_grant",
        descriptor.upload_id,
        json!({
            "evidence_document_id": document_id,
            "evidence_id": evidence_id,
            "evidence_submission_id": descriptor.submission_id,
            "lifecycle_status": "pending",
        }),
    );

    let listed = client
        .call_tool(
            "list_evidence_submissions",
            json!({ "evidence_id": evidence_id }),
        )
        .await;
    assert_eq!(object_keys(&listed), ["submissions"].into_iter().collect());
    assert_eq!(
        listed["submissions"]
            .as_array()
            .expect("submissions array")
            .len(),
        1
    );
    assert_submission_projection(
        &listed["submissions"][0],
        descriptor.submission_id,
        document_id,
        evidence_id,
        user_id,
        connection_id,
        "matching-replay.txt",
        bytes,
        "pending",
    );
    let direct = client
        .call_tool(
            "get_evidence_submission",
            json!({ "submission_id": descriptor.submission_id }),
        )
        .await;
    assert_eq!(direct, listed["submissions"][0]);

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

#[tokio::test]
async fn concurrent_matching_transfers_converge_on_one_submission_and_document() {
    let app = harness::app().await;
    let subject = "auth0|agent-evidence-concurrent";
    let workspace_name = "Agent Evidence Concurrent";
    let bytes = b"concurrent machine evidence";
    let scenario = ScenarioBuilder::new(&app)
        .with_user(subject)
        .with_workspace(subject, workspace_name)
        .with_evidence(workspace_name, "Concurrent evidence")
        .build()
        .await;
    let user_id = scenario.user(subject).id;
    let workspace = scenario.workspace(workspace_name);
    let evidence_id = workspace.evidence("Concurrent evidence").id;
    let token =
        authorize_agent_connection(&app, subject, "Concurrent Upload Agent", PERMISSIONS).await;
    let connection_id = get_agent_connection_id_for(&app, subject, "Concurrent Upload Agent").await;
    let client = McpClient::connect(app.mcp_server(), &token).await;
    let prepared = client
        .call_tool(
            "prepare_evidence_submission_upload",
            json!({
                "evidence_id": evidence_id,
                "valid_from": VALID_FROM,
                "valid_until": VALID_UNTIL,
                "filename": "concurrent.txt",
                "content_type": CONTENT_TYPE,
                "content_length": bytes.len(),
                "checksum_sha256": sha256(bytes),
            }),
        )
        .await;
    let descriptor = machine_transfer(&prepared, CONTENT_TYPE);
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
    let document_id = uuid_at(&left.body["document_id"], "concurrent document id");
    assert_eq!(
        left.body["submission_id"],
        descriptor.submission_id.to_string()
    );
    assert_eq!(left.body["upload_status"], "pending");
    assert_eq!(logs.len(), 1);
    assert_upload_audit_event(
        &logs[0],
        request_id,
        "agent_evidence_upload.completed",
        "rest",
        "upload_agent_evidence",
        user_id,
        connection_id,
        workspace.id,
        "agent_evidence_upload_grant",
        descriptor.upload_id,
        json!({
            "evidence_document_id": document_id,
            "evidence_id": evidence_id,
            "evidence_submission_id": descriptor.submission_id,
            "lifecycle_status": "pending",
        }),
    );

    let listed = client
        .call_tool(
            "list_evidence_submissions",
            json!({ "evidence_id": evidence_id }),
        )
        .await;
    assert_eq!(object_keys(&listed), ["submissions"].into_iter().collect());
    assert_eq!(
        listed["submissions"]
            .as_array()
            .expect("submissions array")
            .len(),
        1
    );
    assert_submission_projection(
        &listed["submissions"][0],
        descriptor.submission_id,
        document_id,
        evidence_id,
        user_id,
        connection_id,
        "concurrent.txt",
        bytes,
        "pending",
    );
    let direct = client
        .call_tool(
            "get_evidence_submission",
            json!({ "submission_id": descriptor.submission_id }),
        )
        .await;
    assert_eq!(direct, listed["submissions"][0]);

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
