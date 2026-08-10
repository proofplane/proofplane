use super::{helpers::*, *};

#[tokio::test]
async fn valid_machine_policy_upload_exposes_descriptor_provenance_audits_and_reaches_uploaded() {
    let app = harness::app().await;
    let subject = "auth0|agent-policy-upload-valid";
    let workspace_name = "Agent Policy Upload Valid";
    let policy_name = "Machine upload policy";
    let filename = "agent-policy.txt";
    let bytes = b"agent-native policy upload";

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
        authorize_agent_connection(&app, subject, "Valid Policy Upload Agent", PERMISSIONS).await;
    let connection_id =
        get_agent_connection_id_for(&app, subject, "Valid Policy Upload Agent").await;
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
            let transferred = execute_transfer(&app, &descriptor, bytes, request_id).await;
            let interception = gate.await_interception().await;
            assert_eq!(interception.aggregate_id, transferred.body["document_id"]);
            (prepared, transferred, gate, request_id)
        })
        .await;

    let descriptor = policy_machine_transfer(&prepared);
    let document_id = assert_pending_result(&transferred, StatusCode::CREATED, policy.id);
    assert_eq!(logs.len(), 2);
    assert_upload_audit_event(
        &logs[0],
        request_id,
        "agent_policy_document_upload_grant.issued",
        "mcp",
        "prepare_policy_document_upload",
        user_id,
        connection_id,
        workspace.id,
        "agent_policy_document_upload_grant",
        descriptor.upload_id,
        json!({ "policy_id": policy.id }),
    );
    assert_upload_audit_event(
        &logs[1],
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

    let client = McpClient::connect(app.mcp_server(), &token).await;
    let pending = client
        .call_tool("get_policy", json!({ "policy_id": policy.id }))
        .await;
    assert_eq!(
        assert_policy_read_model(
            &pending,
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
    let uploaded = client
        .call_tool("get_policy", json!({ "policy_id": policy.id }))
        .await;
    assert_eq!(
        assert_policy_read_model(
            &uploaded,
            policy,
            Some(ExpectedPolicyDocument {
                user_id,
                filename,
                bytes,
                upload_status: "uploaded",
            }),
        ),
        Some(document_id)
    );
}
