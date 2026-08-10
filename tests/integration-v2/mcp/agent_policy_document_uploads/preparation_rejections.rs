use super::{helpers::*, *};

#[tokio::test]
async fn preparation_rejects_invalid_declarations_permissions_and_unavailable_policies() {
    let app = harness::app().await;
    let subject = "auth0|agent-policy-prepare-rejections";
    let foreign = "auth0|agent-policy-prepare-foreign";
    let workspace_name = "Agent Policy Prepare Rejections";
    let foreign_workspace_name = "Agent Policy Prepare Foreign";
    let existing_bytes = b"existing policy document";

    let scenario = ScenarioBuilder::new(&app)
        .with_user(subject)
        .with_workspace(subject, workspace_name)
        .with_policy(workspace_name, "Active Policy")
        .with_policy(workspace_name, "Archived Policy")
        .with_policy(workspace_name, "Documented Policy")
        .with_policy_document(
            workspace_name,
            "Documented Policy",
            "existing-policy.txt",
            existing_bytes,
        )
        .with_user(foreign)
        .with_workspace(foreign, foreign_workspace_name)
        .with_policy(foreign_workspace_name, "Foreign Policy")
        .build()
        .await;
    let workspace = scenario.workspace(workspace_name);
    let user_id = scenario.user(subject).id;
    let active_policy = workspace.policy("Active Policy");
    let archived_policy = workspace.policy("Archived Policy");
    let documented_policy = workspace.policy("Documented Policy");
    let foreign_policy_id = scenario
        .workspace(foreign_workspace_name)
        .policy("Foreign Policy")
        .id;
    let manager_token =
        authorize_agent_connection(&app, subject, "Policy Preparation Manager", PERMISSIONS).await;
    let reader_token = authorize_agent_connection(
        &app,
        subject,
        "Policy Preparation Reader",
        &[WorkspacePermission::ReadControls],
    )
    .await;
    let manager = McpClient::connect(app.mcp_server(), &manager_token).await;
    manager
        .call_tool("archive_policy", json!({ "policy_id": archived_policy.id }))
        .await;
    let active_before = manager
        .call_tool("get_policy", json!({ "policy_id": active_policy.id }))
        .await;
    let documented_before = manager
        .call_tool("get_policy", json!({ "policy_id": documented_policy.id }))
        .await;

    let ((missing, invalid, denied, unknown, cross, archived, current), logs) = app
        .capture_audit_logs(async |request_id| {
            let manager =
                McpClient::connect_with_request_id(app.mcp_server(), &manager_token, request_id)
                    .await;
            let reader =
                McpClient::connect_with_request_id(app.mcp_server(), &reader_token, request_id)
                    .await;
            let missing = manager
                .call_tool_error("prepare_policy_document_upload", json!({}))
                .await;
            let invalid = manager
                .call_tool_error(
                    "prepare_policy_document_upload",
                    json!({
                        "policy_id": active_policy.id,
                        "filename": "../secret.pdf",
                        "content_type": "not a media type",
                        "content_length": MAX_DOCUMENT_BYTES + 1,
                        "checksum_sha256": "A".repeat(64),
                    }),
                )
                .await;
            assert_eq!(
                assert_policy_read_model(&active_before, active_policy, None),
                None
            );
            assert_eq!(
                assert_policy_read_model(
                    &documented_before,
                    documented_policy,
                    Some(ExpectedPolicyDocument {
                        user_id,
                        filename: "existing-policy.txt",
                        bytes: existing_bytes,
                        upload_status: "uploaded",
                    }),
                ),
                Some(documented_policy.document().document_id)
            );
            let declaration = |policy_id: Uuid, filename: &str| {
                json!({
                    "policy_id": policy_id,
                    "filename": filename,
                    "content_type": CONTENT_TYPE,
                    "content_length": 4,
                })
            };
            let denied = reader
                .call_tool_error(
                    "prepare_policy_document_upload",
                    declaration(active_policy.id, "denied.txt"),
                )
                .await;
            let unknown = manager
                .call_tool_error(
                    "prepare_policy_document_upload",
                    declaration(Uuid::new_v4(), "unknown.txt"),
                )
                .await;
            let cross = manager
                .call_tool_error(
                    "prepare_policy_document_upload",
                    declaration(foreign_policy_id, "foreign.txt"),
                )
                .await;
            let archived = manager
                .call_tool_error(
                    "prepare_policy_document_upload",
                    declaration(archived_policy.id, "archived.txt"),
                )
                .await;
            let current = manager
                .call_tool_error(
                    "prepare_policy_document_upload",
                    declaration(documented_policy.id, "current.txt"),
                )
                .await;
            (missing, invalid, denied, unknown, cross, archived, current)
        })
        .await;

    assert_validation_error(
        &missing,
        json!([
            {"field": "policy_id", "message": "is required"},
            {"field": "filename", "message": "is required"},
            {"field": "content_type", "message": "is required"},
            {"field": "content_length", "message": "is required"},
        ]),
    );
    assert_validation_error(
        &invalid,
        json!([
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
    for concealed in [&denied, &unknown, &cross, &archived] {
        assert_not_found(concealed);
    }
    assert_policy_document_exists(&current);
    assert!(logs.is_empty());

    let active_after = manager
        .call_tool("get_policy", json!({ "policy_id": active_policy.id }))
        .await;
    let documented_after = manager
        .call_tool("get_policy", json!({ "policy_id": documented_policy.id }))
        .await;
    assert_eq!(active_after, active_before);
    assert_eq!(documented_after, documented_before);
}
