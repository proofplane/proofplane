use super::{helpers::*, *};

#[tokio::test]
async fn valid_machine_upload_exposes_descriptor_provenance_audits_and_reaches_uploaded() {
    let app = harness::app().await;
    let subject = "auth0|agent-evidence-upload-valid";
    let workspace_name = "Agent Evidence Upload Valid";
    let evidence_title = "Machine upload evidence";
    let bytes = b"agent-native evidence upload";

    let scenario = ScenarioBuilder::new(&app)
        .with_user(subject)
        .with_workspace(subject, workspace_name)
        .with_evidence(workspace_name, evidence_title)
        .build()
        .await;
    let user_id = scenario.user(subject).id;
    let workspace = scenario.workspace(workspace_name);
    let evidence_id = workspace.evidence(evidence_title).id;
    let token = authorize_agent_connection(&app, subject, "Valid Upload Agent", PERMISSIONS).await;
    let connection_id = get_agent_connection_id_for(&app, subject, "Valid Upload Agent").await;
    let mut events = app.pipeline_events().subscribe();

    let ((prepared, transferred, gate, request_id), logs) = app
        .capture_audit_logs(async |request_id| {
            let mut gate = app
                .pipeline_controls()
                .hold(DOCUMENT_SCAN_REQUESTED, request_id);
            let client =
                McpClient::connect_with_request_id(app.mcp_server(), &token, request_id).await;
            let prepared = client
                .call_tool(
                    "prepare_evidence_submission_upload",
                    json!({
                        "evidence_id": evidence_id,
                        "valid_from": VALID_FROM,
                        "valid_until": VALID_UNTIL,
                        "filename": "agent-evidence.txt",
                        "content_type": CONTENT_TYPE,
                        "content_length": bytes.len(),
                        "checksum_sha256": sha256(bytes),
                    }),
                )
                .await;
            let descriptor = machine_transfer(&prepared, CONTENT_TYPE);
            let transferred = execute_transfer(&app, &descriptor, bytes, request_id).await;
            let interception = gate.await_interception().await;
            assert_eq!(interception.aggregate_id, transferred.body["document_id"]);
            (prepared, transferred, gate, request_id)
        })
        .await;

    let descriptor = machine_transfer(&prepared, CONTENT_TYPE);
    assert_eq!(transferred.status, StatusCode::CREATED);
    let document_id = uuid_at(&transferred.body["document_id"], "document id");
    assert_eq!(
        transferred.body,
        json!({
            "submission_id": descriptor.submission_id,
            "document_id": document_id,
            "upload_status": "pending",
        })
    );

    assert_eq!(logs.len(), 2);
    assert_upload_audit_event(
        &logs[0],
        request_id,
        "agent_evidence_upload_grant.issued",
        "mcp",
        "prepare_evidence_submission_upload",
        user_id,
        connection_id,
        workspace.id,
        "agent_evidence_upload_grant",
        descriptor.upload_id,
        json!({
            "evidence_id": evidence_id,
            "evidence_submission_id": descriptor.submission_id,
        }),
    );
    assert_upload_audit_event(
        &logs[1],
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

    let client = McpClient::connect(app.mcp_server(), &token).await;
    let uploaded = client
        .call_tool(
            "get_evidence_submission",
            json!({ "submission_id": descriptor.submission_id }),
        )
        .await;
    assert_submission_read_model(
        &uploaded,
        descriptor.submission_id,
        document_id,
        evidence_id,
        user_id,
        connection_id,
        "agent-evidence.txt",
        bytes,
        "uploaded",
    );
}
