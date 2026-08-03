use super::*;

#[tokio::test]
async fn preparation_rejects_invalid_declarations_permissions_and_concealed_evidence() {
    let app = harness::app().await;
    let subject = "auth0|agent-evidence-prepare-rejections";
    let foreign = "auth0|agent-evidence-prepare-foreign";
    let workspace_name = "Agent Evidence Prepare Rejections";
    let foreign_workspace_name = "Agent Evidence Prepare Foreign";

    let scenario = ScenarioBuilder::new(&app)
        .with_user(subject)
        .with_workspace(subject, workspace_name)
        .with_evidence(workspace_name, "Owned evidence")
        .with_user(foreign)
        .with_workspace(foreign, foreign_workspace_name)
        .with_evidence(foreign_workspace_name, "Foreign evidence")
        .build()
        .await;
    let evidence_id = scenario
        .workspace(workspace_name)
        .evidence("Owned evidence")
        .id;
    let foreign_evidence_id = scenario
        .workspace(foreign_workspace_name)
        .evidence("Foreign evidence")
        .id;
    let manager_token =
        authorize_agent_connection(&app, subject, "Preparation Manager", PERMISSIONS).await;
    let reader_token = authorize_agent_connection(
        &app,
        subject,
        "Preparation Reader",
        &[WorkspacePermission::ReadEvidenceSubmissions],
    )
    .await;

    let ((missing, invalid, denied, unknown, cross_workspace, listed), logs) = app
        .capture_audit_logs(async |request_id| {
            let manager =
                McpClient::connect_with_request_id(app.mcp_server(), &manager_token, request_id)
                    .await;
            let reader =
                McpClient::connect_with_request_id(app.mcp_server(), &reader_token, request_id)
                    .await;
            let missing = manager
                .call_tool_error("prepare_evidence_submission_upload", json!({}))
                .await;
            let invalid = manager
                .call_tool_error(
                    "prepare_evidence_submission_upload",
                    json!({
                        "evidence_id": evidence_id,
                        "valid_from": "2026-04-01T00:00:00.000Z",
                        "valid_until": VALID_UNTIL,
                        "filename": "../secret.pdf",
                        "content_type": "not a media type",
                        "content_length": MAX_DOCUMENT_BYTES + 1,
                        "checksum_sha256": "A".repeat(64),
                    }),
                )
                .await;
            let denied = reader
                .call_tool_error(
                    "prepare_evidence_submission_upload",
                    json!({
                        "evidence_id": evidence_id,
                        "valid_from": VALID_FROM,
                        "valid_until": VALID_UNTIL,
                        "filename": "denied.txt",
                        "content_type": CONTENT_TYPE,
                        "content_length": 4,
                    }),
                )
                .await;
            let unknown = manager
                .call_tool_error(
                    "prepare_evidence_submission_upload",
                    json!({
                        "evidence_id": Uuid::new_v4(),
                        "valid_from": VALID_FROM,
                        "valid_until": VALID_UNTIL,
                        "filename": "unknown.txt",
                        "content_type": CONTENT_TYPE,
                        "content_length": 4,
                    }),
                )
                .await;
            let cross_workspace = manager
                .call_tool_error(
                    "prepare_evidence_submission_upload",
                    json!({
                        "evidence_id": foreign_evidence_id,
                        "valid_from": VALID_FROM,
                        "valid_until": VALID_UNTIL,
                        "filename": "foreign.txt",
                        "content_type": CONTENT_TYPE,
                        "content_length": 4,
                    }),
                )
                .await;
            let listed = manager
                .call_tool(
                    "list_evidence_submissions",
                    json!({ "evidence_id": evidence_id }),
                )
                .await;
            (missing, invalid, denied, unknown, cross_workspace, listed)
        })
        .await;

    assert_validation_error(
        &missing,
        json!([
            {"field": "evidence_id", "message": "is required"},
            {"field": "valid_from", "message": "is required"},
            {"field": "valid_until", "message": "is required"},
            {"field": "filename", "message": "is required"},
            {"field": "content_type", "message": "is required"},
            {"field": "content_length", "message": "is required"},
        ]),
    );
    assert_validation_error(
        &invalid,
        json!([
            {
                "field": "valid_until",
                "message": "valid_until must be greater than or equal to valid_from"
            },
            {
                "field": "filename",
                "message": "document filename contains unsupported characters"
            },
            {
                "field": "content_type",
                "message": "content_type must be a valid HTTP media type"
            },
            {
                "field": "content_length",
                "message": format!("content_length must be at most {MAX_DOCUMENT_BYTES} bytes")
            },
            {
                "field": "checksum_sha256",
                "message": "checksum_sha256 must be 64 lowercase hexadecimal characters"
            },
        ]),
    );
    assert_not_found(&denied);
    assert_not_found(&unknown);
    assert_not_found(&cross_workspace);
    assert!(logs.is_empty());
    assert_eq!(listed, json!({ "submissions": [] }));
}
