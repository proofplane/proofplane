use super::{helpers::*, *};

#[tokio::test]
async fn transfer_conceals_invalid_authority_and_rejects_header_mismatches_before_retry() {
    let app = harness::app().await;
    let subject = "auth0|agent-evidence-transfer-rejections";
    let workspace_name = "Agent Evidence Transfer Rejections";
    let bytes = b"retryable authority and metadata";
    let scenario = ScenarioBuilder::new(&app)
        .with_user(subject)
        .with_workspace(subject, workspace_name)
        .with_evidence(workspace_name, "Transfer rejection evidence")
        .build()
        .await;
    let user_id = scenario.user(subject).id;
    let workspace_id = scenario.workspace(workspace_name).id;
    let evidence_id = scenario
        .workspace(workspace_name)
        .evidence("Transfer rejection evidence")
        .id;
    let token =
        authorize_agent_connection(&app, subject, "Transfer Rejection Agent", PERMISSIONS).await;
    let connection_id =
        get_agent_connection_id_for(&app, subject, "Transfer Rejection Agent").await;
    let client = McpClient::connect(app.mcp_server(), &token).await;
    let prepared = client
        .call_tool(
            "prepare_evidence_submission_upload",
            json!({
                "evidence_id": evidence_id,
                "valid_from": VALID_FROM,
                "valid_until": VALID_UNTIL,
                "filename": "transfer-retry.txt",
                "content_type": CONTENT_TYPE,
                "content_length": bytes.len(),
                "checksum_sha256": sha256(bytes),
            }),
        )
        .await;
    let descriptor = machine_transfer(&prepared, CONTENT_TYPE);
    let path = descriptor.path.clone();
    let tampered_authorization = tamper(&descriptor.authorization);
    let wrong_path = format!("/agent-evidence-uploads/{}", Uuid::new_v4());
    let malformed_path = "/agent-evidence-uploads/not-a-uuid";

    let ((), rejected_logs) = app
        .capture_audit_logs(async |captured_request_id| {
            let unavailable = [
                fail_transfer_on_purpose(
                    &app,
                    malformed_path,
                    Some(&descriptor.authorization),
                    Some(CONTENT_TYPE),
                    Some(bytes.len() as u64),
                    &[],
                    captured_request_id,
                )
                .await,
                fail_transfer_on_purpose(
                    &app,
                    &wrong_path,
                    Some(&descriptor.authorization),
                    Some(CONTENT_TYPE),
                    Some(bytes.len() as u64),
                    &[],
                    captured_request_id,
                )
                .await,
                fail_transfer_on_purpose(
                    &app,
                    &path,
                    None,
                    Some(CONTENT_TYPE),
                    Some(bytes.len() as u64),
                    &[],
                    captured_request_id,
                )
                .await,
                fail_transfer_on_purpose(
                    &app,
                    &path,
                    Some("Bearer wrong-scheme"),
                    Some(CONTENT_TYPE),
                    Some(bytes.len() as u64),
                    &[],
                    captured_request_id,
                )
                .await,
                fail_transfer_on_purpose(
                    &app,
                    &path,
                    Some(&tampered_authorization),
                    Some(CONTENT_TYPE),
                    Some(bytes.len() as u64),
                    &[],
                    captured_request_id,
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

            let missing_type = fail_transfer_on_purpose(
                &app,
                &path,
                Some(&descriptor.authorization),
                None,
                Some(bytes.len() as u64),
                &[],
                captured_request_id,
            )
            .await;
            assert_http_error(
                &missing_type,
                StatusCode::BAD_REQUEST,
                "bad_request",
                "request validation failed",
                json!(["content-type header is required"]),
            );
            let missing_length = fail_transfer_on_purpose(
                &app,
                &path,
                Some(&descriptor.authorization),
                Some(CONTENT_TYPE),
                None,
                &[],
                captured_request_id,
            )
            .await;
            assert_http_error(
                &missing_length,
                StatusCode::BAD_REQUEST,
                "bad_request",
                "request validation failed",
                json!(["valid content-length header is required"]),
            );
            let wrong_type = fail_transfer_on_purpose(
                &app,
                &path,
                Some(&descriptor.authorization),
                Some("application/pdf"),
                Some(bytes.len() as u64),
                &[],
                captured_request_id,
            )
            .await;
            assert_http_error(
                &wrong_type,
                StatusCode::BAD_REQUEST,
                "bad_request",
                "request validation failed",
                json!(["content-type header does not match upload grant"]),
            );
            let wrong_length = fail_transfer_on_purpose(
                &app,
                &path,
                Some(&descriptor.authorization),
                Some(CONTENT_TYPE),
                Some(bytes.len() as u64 - 1),
                &[],
                captured_request_id,
            )
            .await;
            assert_http_error(
                &wrong_length,
                StatusCode::BAD_REQUEST,
                "bad_request",
                "request validation failed",
                json!(["content-length header does not match upload grant"]),
            );
        })
        .await;
    assert!(rejected_logs.is_empty());
    assert_preallocated_submission_is_concealed(&client, evidence_id, descriptor.submission_id)
        .await;

    let ((created, completion_request_id), completion_logs) = app
        .capture_audit_logs(async |captured_request_id| {
            (
                execute_transfer(&app, &descriptor, bytes, captured_request_id).await,
                captured_request_id,
            )
        })
        .await;
    let document_id = assert_pending_result(&created, StatusCode::CREATED, &descriptor);
    assert_eq!(completion_logs.len(), 1);
    assert_upload_audit_event(
        &completion_logs[0],
        completion_request_id,
        "agent_evidence_upload.completed",
        "rest",
        "upload_agent_evidence",
        user_id,
        connection_id,
        workspace_id,
        "agent_evidence_upload_grant",
        descriptor.upload_id,
        json!({
            "evidence_document_id": document_id,
            "evidence_id": evidence_id,
            "evidence_submission_id": descriptor.submission_id,
            "lifecycle_status": "pending",
        }),
    );
}

#[tokio::test]
async fn length_checksum_and_body_limit_failures_leave_the_grant_retryable() {
    let app = harness::app().await;
    let subject = "auth0|agent-evidence-content-rejections";
    let workspace_name = "Agent Evidence Content Rejections";
    let bytes = b"declared machine upload bytes";
    let scenario = ScenarioBuilder::new(&app)
        .with_user(subject)
        .with_workspace(subject, workspace_name)
        .with_evidence(workspace_name, "Content rejection evidence")
        .build()
        .await;
    let user_id = scenario.user(subject).id;
    let workspace_id = scenario.workspace(workspace_name).id;
    let evidence_id = scenario
        .workspace(workspace_name)
        .evidence("Content rejection evidence")
        .id;
    let token =
        authorize_agent_connection(&app, subject, "Content Rejection Agent", PERMISSIONS).await;
    let connection_id = get_agent_connection_id_for(&app, subject, "Content Rejection Agent").await;
    let client = McpClient::connect(app.mcp_server(), &token).await;
    let prepared = client
        .call_tool(
            "prepare_evidence_submission_upload",
            json!({
                "evidence_id": evidence_id,
                "valid_from": VALID_FROM,
                "valid_until": VALID_UNTIL,
                "filename": "content-retry.txt",
                "content_type": CONTENT_TYPE,
                "content_length": bytes.len(),
                "checksum_sha256": sha256(bytes),
            }),
        )
        .await;
    let descriptor = machine_transfer(&prepared, CONTENT_TYPE);

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
    assert_preallocated_submission_is_concealed(&client, evidence_id, descriptor.submission_id)
        .await;

    let ((created, completion_request_id), completion_logs) = app
        .capture_audit_logs(async |request_id| {
            (
                execute_transfer(&app, &descriptor, bytes, request_id).await,
                request_id,
            )
        })
        .await;
    let document_id = assert_pending_result(&created, StatusCode::CREATED, &descriptor);
    assert_eq!(completion_logs.len(), 1);
    assert_upload_audit_event(
        &completion_logs[0],
        completion_request_id,
        "agent_evidence_upload.completed",
        "rest",
        "upload_agent_evidence",
        user_id,
        connection_id,
        workspace_id,
        "agent_evidence_upload_grant",
        descriptor.upload_id,
        json!({
            "evidence_document_id": document_id,
            "evidence_id": evidence_id,
            "evidence_submission_id": descriptor.submission_id,
            "lifecycle_status": "pending",
        }),
    );
}

#[tokio::test]
async fn interrupted_transfer_returns_stable_error_and_remains_retryable() {
    let app = harness::app().await;
    let subject = "auth0|agent-evidence-interrupted";
    let workspace_name = "Agent Evidence Interrupted";
    let bytes = b"interrupted machine upload body";
    let scenario = ScenarioBuilder::new(&app)
        .with_user(subject)
        .with_workspace(subject, workspace_name)
        .with_evidence(workspace_name, "Interrupted evidence")
        .build()
        .await;
    let user_id = scenario.user(subject).id;
    let workspace_id = scenario.workspace(workspace_name).id;
    let evidence_id = scenario
        .workspace(workspace_name)
        .evidence("Interrupted evidence")
        .id;
    let token =
        authorize_agent_connection(&app, subject, "Interrupted Upload Agent", PERMISSIONS).await;
    let connection_id =
        get_agent_connection_id_for(&app, subject, "Interrupted Upload Agent").await;
    let client = McpClient::connect(app.mcp_server(), &token).await;
    let prepared = client
        .call_tool(
            "prepare_evidence_submission_upload",
            json!({
                "evidence_id": evidence_id,
                "valid_from": VALID_FROM,
                "valid_until": VALID_UNTIL,
                "filename": "interrupted.txt",
                "content_type": CONTENT_TYPE,
                "content_length": bytes.len(),
                "checksum_sha256": sha256(bytes),
            }),
        )
        .await;
    let descriptor = machine_transfer(&prepared, CONTENT_TYPE);

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
    assert_eq!(interrupted.status, StatusCode::BAD_REQUEST);
    assert_eq!(
        interrupted.body,
        json!({
            "error": {
                "code": "bad_request",
                "message": "request validation failed",
                "details": ["request body stream failed"],
            }
        })
    );
    assert!(interrupted_logs.is_empty());
    assert_preallocated_submission_is_concealed(&client, evidence_id, descriptor.submission_id)
        .await;

    let ((created, completion_request_id), completion_logs) = app
        .capture_audit_logs(async |request_id| {
            (
                execute_transfer(&app, &descriptor, bytes, request_id).await,
                request_id,
            )
        })
        .await;
    let document_id = assert_pending_result(&created, StatusCode::CREATED, &descriptor);
    assert_eq!(completion_logs.len(), 1);
    assert_upload_audit_event(
        &completion_logs[0],
        completion_request_id,
        "agent_evidence_upload.completed",
        "rest",
        "upload_agent_evidence",
        user_id,
        connection_id,
        workspace_id,
        "agent_evidence_upload_grant",
        descriptor.upload_id,
        json!({
            "evidence_document_id": document_id,
            "evidence_id": evidence_id,
            "evidence_submission_id": descriptor.submission_id,
            "lifecycle_status": "pending",
        }),
    );
}
