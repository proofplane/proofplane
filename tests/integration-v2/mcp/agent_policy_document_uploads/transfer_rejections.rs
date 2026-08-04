use super::{helpers::*, *};

#[tokio::test]
async fn transfer_conceals_invalid_authority_and_rejects_header_mismatches_before_retry() {
    let app = harness::app().await;
    let subject = "auth0|agent-policy-transfer-rejections";
    let workspace_name = "Agent Policy Transfer Rejections";
    let policy_name = "Transfer rejection policy";
    let bytes = b"retryable policy authority and metadata";
    let scenario = ScenarioBuilder::new(&app)
        .with_user(subject)
        .with_workspace(subject, workspace_name)
        .with_policy(workspace_name, policy_name)
        .build()
        .await;
    let user_id = scenario.user(subject).id;
    let workspace = scenario.workspace(workspace_name);
    let policy = workspace.policy(policy_name);
    let token = authorize_agent_connection(
        &app,
        subject,
        "Policy Transfer Rejection Agent",
        PERMISSIONS,
    )
    .await;
    let connection_id =
        get_agent_connection_id_for(&app, subject, "Policy Transfer Rejection Agent").await;
    let client = McpClient::connect(app.mcp_server(), &token).await;
    let before = client
        .call_tool("get_policy", json!({ "policy_id": policy.id }))
        .await;
    let prepared = client
        .call_tool(
            "prepare_policy_document_upload",
            json!({
                "policy_id": policy.id,
                "filename": "policy-transfer-retry.txt",
                "content_type": CONTENT_TYPE,
                "content_length": bytes.len(),
                "checksum_sha256": sha256(bytes),
            }),
        )
        .await;
    let descriptor = policy_machine_transfer(&prepared);
    let tampered_authorization = tamper(&descriptor.authorization);
    let wrong_path = format!("/agent-policy-document-uploads/{}", Uuid::new_v4());

    let ((), rejected_logs) = app
        .capture_audit_logs(async |request_id| {
            let unavailable = [
                fail_transfer_on_purpose(
                    &app,
                    "/agent-policy-document-uploads/not-a-uuid",
                    Some(&descriptor.authorization),
                    Some(CONTENT_TYPE),
                    Some(bytes.len() as u64),
                    &[],
                    request_id,
                )
                .await,
                fail_transfer_on_purpose(
                    &app,
                    &wrong_path,
                    Some(&descriptor.authorization),
                    Some(CONTENT_TYPE),
                    Some(bytes.len() as u64),
                    &[],
                    request_id,
                )
                .await,
                fail_transfer_on_purpose(
                    &app,
                    &descriptor.path,
                    None,
                    Some(CONTENT_TYPE),
                    Some(bytes.len() as u64),
                    &[],
                    request_id,
                )
                .await,
                fail_transfer_on_purpose(
                    &app,
                    &descriptor.path,
                    Some("Bearer wrong-scheme"),
                    Some(CONTENT_TYPE),
                    Some(bytes.len() as u64),
                    &[],
                    request_id,
                )
                .await,
                fail_transfer_on_purpose(
                    &app,
                    &descriptor.path,
                    Some(&tampered_authorization),
                    Some(CONTENT_TYPE),
                    Some(bytes.len() as u64),
                    &[],
                    request_id,
                )
                .await,
            ];
            for response in unavailable {
                assert_http_error(
                    &response,
                    StatusCode::NOT_FOUND,
                    "not_found",
                    "route not found",
                    json!([]),
                );
            }

            let validations = [
                (
                    fail_transfer_on_purpose(
                        &app,
                        &descriptor.path,
                        Some(&descriptor.authorization),
                        None,
                        Some(bytes.len() as u64),
                        &[],
                        request_id,
                    )
                    .await,
                    "content-type header is required",
                ),
                (
                    fail_transfer_on_purpose(
                        &app,
                        &descriptor.path,
                        Some(&descriptor.authorization),
                        Some(CONTENT_TYPE),
                        None,
                        &[],
                        request_id,
                    )
                    .await,
                    "valid content-length header is required",
                ),
                (
                    fail_transfer_on_purpose(
                        &app,
                        &descriptor.path,
                        Some(&descriptor.authorization),
                        Some("application/pdf"),
                        Some(bytes.len() as u64),
                        &[],
                        request_id,
                    )
                    .await,
                    "content-type header does not match upload grant",
                ),
                (
                    fail_transfer_on_purpose(
                        &app,
                        &descriptor.path,
                        Some(&descriptor.authorization),
                        Some(CONTENT_TYPE),
                        Some(bytes.len() as u64 - 1),
                        &[],
                        request_id,
                    )
                    .await,
                    "content-length header does not match upload grant",
                ),
            ];
            for (response, detail) in validations {
                assert_http_error(
                    &response,
                    StatusCode::BAD_REQUEST,
                    "bad_request",
                    "request validation failed",
                    json!([detail]),
                );
            }
        })
        .await;
    assert!(rejected_logs.is_empty());
    assert_eq!(
        client
            .call_tool("get_policy", json!({ "policy_id": policy.id }))
            .await,
        before
    );

    let ((created, request_id), completion_logs) = app
        .capture_audit_logs(async |request_id| {
            (
                execute_transfer(&app, &descriptor, bytes, request_id).await,
                request_id,
            )
        })
        .await;
    let document_id = assert_pending_result(&created, StatusCode::CREATED, policy.id);
    assert_eq!(completion_logs.len(), 1);
    assert_upload_audit_event(
        &completion_logs[0],
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
}

#[tokio::test]
async fn checksum_and_body_limit_failures_leave_the_policy_grant_retryable() {
    let app = harness::app().await;
    let subject = "auth0|agent-policy-content-rejections";
    let workspace_name = "Agent Policy Content Rejections";
    let policy_name = "Content rejection policy";
    let bytes = b"declared machine policy bytes";
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
        authorize_agent_connection(&app, subject, "Policy Content Rejection Agent", PERMISSIONS)
            .await;
    let connection_id =
        get_agent_connection_id_for(&app, subject, "Policy Content Rejection Agent").await;
    let client = McpClient::connect(app.mcp_server(), &token).await;
    let before = client
        .call_tool("get_policy", json!({ "policy_id": policy.id }))
        .await;
    let prepared = client
        .call_tool(
            "prepare_policy_document_upload",
            json!({
                "policy_id": policy.id,
                "filename": "policy-content-retry.txt",
                "content_type": CONTENT_TYPE,
                "content_length": bytes.len(),
                "checksum_sha256": sha256(bytes),
            }),
        )
        .await;
    let descriptor = policy_machine_transfer(&prepared);

    let ((), rejected_logs) = app
        .capture_audit_logs(async |request_id| {
            let different = vec![b'x'; bytes.len()];
            let checksum = fail_transfer_on_purpose(
                &app,
                &descriptor.path,
                Some(&descriptor.authorization),
                Some(CONTENT_TYPE),
                Some(bytes.len() as u64),
                &different,
                request_id,
            )
            .await;
            assert_http_error(
                &checksum,
                StatusCode::BAD_REQUEST,
                "bad_request",
                "request validation failed",
                json!(["request body checksum does not match upload grant"]),
            );
            let oversized = fail_transfer_on_purpose(
                &app,
                &descriptor.path,
                Some(&descriptor.authorization),
                Some(CONTENT_TYPE),
                Some(MAX_DOCUMENT_BYTES + 1),
                &[],
                request_id,
            )
            .await;
            assert_http_error(
                &oversized,
                StatusCode::PAYLOAD_TOO_LARGE,
                "payload_too_large",
                "request payload is too large",
                json!([]),
            );
        })
        .await;
    assert!(rejected_logs.is_empty());
    assert_eq!(
        client
            .call_tool("get_policy", json!({ "policy_id": policy.id }))
            .await,
        before
    );

    let ((created, request_id), completion_logs) = app
        .capture_audit_logs(async |request_id| {
            (
                execute_transfer(&app, &descriptor, bytes, request_id).await,
                request_id,
            )
        })
        .await;
    let document_id = assert_pending_result(&created, StatusCode::CREATED, policy.id);
    assert_eq!(completion_logs.len(), 1);
    assert_upload_audit_event(
        &completion_logs[0],
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
}

#[tokio::test]
async fn interrupted_policy_transfer_returns_stable_error_and_remains_retryable() {
    let app = harness::app().await;
    let subject = "auth0|agent-policy-interrupted";
    let workspace_name = "Agent Policy Interrupted";
    let policy_name = "Interrupted policy";
    let bytes = b"interrupted machine policy body";
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
        authorize_agent_connection(&app, subject, "Interrupted Policy Agent", PERMISSIONS).await;
    let connection_id =
        get_agent_connection_id_for(&app, subject, "Interrupted Policy Agent").await;
    let client = McpClient::connect(app.mcp_server(), &token).await;
    let before = client
        .call_tool("get_policy", json!({ "policy_id": policy.id }))
        .await;
    let prepared = client
        .call_tool(
            "prepare_policy_document_upload",
            json!({
                "policy_id": policy.id,
                "filename": "interrupted-policy.txt",
                "content_type": CONTENT_TYPE,
                "content_length": bytes.len(),
                "checksum_sha256": sha256(bytes),
            }),
        )
        .await;
    let descriptor = policy_machine_transfer(&prepared);

    let (interrupted, interrupted_logs) = app
        .capture_audit_logs(async |request_id| {
            interrupted_transfer(
                &app,
                &descriptor,
                &bytes[..bytes.len() - 5],
                bytes.len(),
                request_id,
            )
            .await
        })
        .await;
    assert_http_error(
        &interrupted,
        StatusCode::BAD_REQUEST,
        "bad_request",
        "request validation failed",
        json!(["request body stream failed"]),
    );
    assert!(interrupted_logs.is_empty());
    assert_eq!(
        client
            .call_tool("get_policy", json!({ "policy_id": policy.id }))
            .await,
        before
    );

    let ((created, request_id), completion_logs) = app
        .capture_audit_logs(async |request_id| {
            (
                execute_transfer(&app, &descriptor, bytes, request_id).await,
                request_id,
            )
        })
        .await;
    let document_id = assert_pending_result(&created, StatusCode::CREATED, policy.id);
    assert_eq!(completion_logs.len(), 1);
    assert_upload_audit_event(
        &completion_logs[0],
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
}
