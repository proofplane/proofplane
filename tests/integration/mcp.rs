use axum::http::{header, HeaderName, HeaderValue, StatusCode};
use proofplane::{
    domain::WorkspacePermission, mcp::SESSION_ID_HEADER, routes::request_context::REQUEST_ID_HEADER,
};
use rmcp::{
    model::{CallToolRequestParams, ClientInfo, JsonObject},
    service::{RoleClient, RunningService},
    transport::{
        streamable_http_client::StreamableHttpClientTransportConfig, StreamableHttpClientTransport,
    },
    ServiceError, ServiceExt,
};
use serde_json::{json, Value};
use std::collections::{BTreeSet, HashMap};
use uuid::Uuid;

use super::support::{capture_audit_logs, cc61_id, cc71_id, soc2_framework_id, TestApp};

const MCP: &str = "/mcp";

#[tokio::test]
async fn mcp_reauthenticates_token_state_and_serves_public_operational_routes() {
    let app = TestApp::start_without_default_auth().await;
    let server = app.mcp_http_server();
    let client = app
        .postgres
        .get()
        .await
        .expect("fixture database connection opens");
    let workspace_id = app.home_workspace_id();

    let initialized = initialize(&server, app.api_token()).await;
    initialized.assert_status_ok();
    assert!(initialized.text().contains("proofplane"));
    let session_id = initialized.header(SESSION_ID_HEADER);
    let mcp_client = McpClient::connect(&server, app.api_token()).await;
    let tool_list = mcp_client.list_tools().await;
    let tool_names = tool_list
        .iter()
        .map(|tool| tool["name"].as_str().expect("tool has a name"))
        .collect::<BTreeSet<_>>();
    let expected_tool_names = [
        "create_evidence_request",
        "list_evidence_requests",
        "get_evidence_request",
        "list_due_evidence_requests",
        "get_evidence_submission",
        "get_latest_evidence_submission",
        "create_evidence_submission",
        "manage_evidence_submission_attachment",
        "list_frameworks",
        "list_framework_requirements",
        "list_controls",
        "get_control",
        "create_control",
        "replace_control",
        "list_evidence_request_control_mappings",
        "map_evidence_request_to_control",
        "remove_evidence_request_control_mapping",
    ]
    .into_iter()
    .collect::<BTreeSet<_>>();
    assert_eq!(tool_names, expected_tool_names);
    assert_schema_has_property(
        &find_tool(&tool_list, "create_evidence_request")["inputSchema"],
        "title",
    );
    assert_schema_has_property(
        &find_tool(&tool_list, "create_evidence_request")["inputSchema"],
        "due_at",
    );
    assert_schema_lacks_property(
        &find_tool(&tool_list, "create_evidence_request")["inputSchema"],
        "workspace_id",
    );
    assert_schema_lacks_property(
        &find_tool(&tool_list, "list_due_evidence_requests")["inputSchema"],
        "workspace_id",
    );
    assert_schema_has_property(
        &find_tool(&tool_list, "list_due_evidence_requests")["inputSchema"],
        "now",
    );
    assert_schema_has_property(
        &find_tool(&tool_list, "get_evidence_submission")["inputSchema"],
        "submission_id",
    );
    assert_schema_has_property(
        &find_tool(&tool_list, "create_evidence_submission")["inputSchema"],
        "coverage_start_at",
    );
    assert_schema_has_property(
        &find_tool(&tool_list, "create_evidence_submission")["inputSchema"],
        "source_system",
    );
    assert_schema_has_property(
        &find_tool(&tool_list, "manage_evidence_submission_attachment")["inputSchema"],
        "submission_id",
    );
    assert_schema_has_property(
        &find_tool(&tool_list, "map_evidence_request_to_control")["inputSchema"],
        "rationale",
    );
    assert_schema_has_property(
        &find_tool(&tool_list, "remove_evidence_request_control_mapping")["inputSchema"],
        "control_id",
    );
    assert_schema_has_property(
        &find_tool(&tool_list, "list_framework_requirements")["inputSchema"],
        "framework_id",
    );
    assert_schema_lacks_property(
        &find_tool(&tool_list, "list_framework_requirements")["inputSchema"],
        "workspace_id",
    );
    assert_schema_has_property(
        &find_tool(&tool_list, "get_control")["inputSchema"],
        "control_id",
    );
    assert_schema_lacks_property(
        &find_tool(&tool_list, "get_control")["inputSchema"],
        "workspace_id",
    );
    assert_schema_has_property(
        &find_tool(&tool_list, "create_control")["inputSchema"],
        "framework_requirement_ids",
    );
    assert_schema_has_property(
        &find_tool(&tool_list, "replace_control")["inputSchema"],
        "framework_requirement_ids",
    );
    assert_schema_has_property(
        &find_tool(&tool_list, "list_evidence_requests")["outputSchema"],
        "evidence_requests",
    );
    assert_schema_has_property(
        &find_tool(&tool_list, "create_evidence_request")["outputSchema"],
        "evidence_request",
    );
    assert_schema_has_property(
        &find_tool(&tool_list, "get_evidence_submission")["outputSchema"],
        "submission",
    );
    assert_schema_has_property(
        &find_tool(&tool_list, "get_evidence_submission")["outputSchema"],
        "attachments",
    );
    assert_schema_has_property(
        &find_tool(&tool_list, "create_evidence_submission")["outputSchema"],
        "attachment_upload",
    );
    assert_schema_has_property(
        &find_tool(&tool_list, "list_controls")["outputSchema"],
        "controls",
    );
    assert_schema_has_property(
        &find_tool(&tool_list, "list_frameworks")["outputSchema"],
        "frameworks",
    );
    assert_schema_has_property(
        &find_tool(&tool_list, "list_framework_requirements")["outputSchema"],
        "requirements",
    );
    assert_schema_has_property(&find_tool(&tool_list, "get_control")["outputSchema"], "id");
    assert_schema_has_property(
        &find_tool(&tool_list, "create_control")["outputSchema"],
        "framework_requirements",
    );
    assert_schema_has_property(
        &find_tool(&tool_list, "replace_control")["outputSchema"],
        "framework_requirements",
    );
    assert_schema_has_property(
        &find_tool(&tool_list, "list_evidence_request_control_mappings")["outputSchema"],
        "mappings",
    );
    assert_schema_has_property(
        &find_tool(&tool_list, "map_evidence_request_to_control")["outputSchema"],
        "control",
    );
    assert_schema_has_property(
        &find_tool(&tool_list, "remove_evidence_request_control_mapping")["outputSchema"],
        "removed",
    );
    assert_schema_has_property(
        &find_tool(&tool_list, "manage_evidence_submission_attachment")["outputSchema"],
        "url_secret_type",
    );
    assert_schema_has_property(
        &find_tool(&tool_list, "manage_evidence_submission_attachment")["outputSchema"],
        "expires_at",
    );
    assert_schema_has_property(
        &find_tool(&tool_list, "manage_evidence_submission_attachment")["outputSchema"],
        "intended_use",
    );

    client
        .execute(
            "UPDATE api_tokens SET revoked_at = now() WHERE id = $1",
            &[&app.api_token_id()],
        )
        .await
        .expect("token revokes");
    let revoked = server
        .delete(MCP)
        .add_header(header::AUTHORIZATION, app.bearer_token())
        .add_header(SESSION_ID_HEADER, session_id)
        .await;
    assert_unauthorized(&revoked);

    let expired = app
        .issue_api_token(workspace_id, WorkspacePermission::ALL.to_vec())
        .await;
    client
        .execute(
            "UPDATE api_tokens SET expires_at = now() - interval '1 second' WHERE id = $1",
            &[&Uuid::from(expired.token_id)],
        )
        .await
        .expect("token expires");
    assert_unauthorized(&initialize(&server, &expired.raw_token).await);

    let removed_member = app
        .issue_api_token(workspace_id, WorkspacePermission::ALL.to_vec())
        .await;
    client
        .execute(
            "DELETE FROM workspace_memberships WHERE workspace_id = $1 AND user_id = $2",
            &[&workspace_id, &Uuid::from(removed_member.user_id)],
        )
        .await
        .expect("membership removes");
    assert_unauthorized(&initialize(&server, &removed_member.raw_token).await);

    server.get("/livez").await.assert_status_ok();
    server.get("/readyz").await.assert_status_ok();
    let metrics = server.get("/metrics").await;
    metrics.assert_status_ok();
    assert!(metrics
        .header(header::CONTENT_TYPE)
        .to_str()
        .expect("content type is text")
        .starts_with("text/plain"));
}

#[tokio::test]
async fn mcp_evidence_request_tools_match_rest_scope_and_validate_all_fields() {
    let app = TestApp::builder()
        .workspace("workspace", "MCP request workspace")
        .with_default_membership()
        .workspace("other", "MCP other workspace")
        .without_membership()
        .build()
        .await;
    let server = app.mcp_http_server();
    let mcp_client = McpClient::connect(&server, app.api_token()).await;
    let workspace_id = app.workspace_id("workspace");
    let other_workspace_id = app.workspace_id("other");
    let due = app
        .create_evidence_request(
            workspace_id,
            &evidence_request("Due request", "2026-01-01T00:00:00Z"),
        )
        .await;
    let later = app
        .create_evidence_request(
            workspace_id,
            &evidence_request("Later request", "2026-03-01T00:00:00Z"),
        )
        .await;
    app.insert_evidence_request_row(other_workspace_id, "Hidden request")
        .await;

    let token = app.api_token().to_owned();
    let (created, create_logs) = capture_audit_logs(|request_id| {
        let server = &server;
        let token = token.clone();
        async move {
            let mcp_client = McpClient::connect_with_request_id(server, &token, request_id).await;
            mcp_client
                .call_tool(
                    "create_evidence_request",
                    evidence_request("Created by MCP", "2026-02-15T00:00:00Z"),
                )
                .await
        }
    })
    .await;
    let created_id = uuid_from(&created["evidence_request"]["id"]);
    assert_eq!(
        created["evidence_request"]["workspace_id"],
        workspace_id.to_string()
    );
    assert_eq!(created["evidence_request"]["title"], "Created by MCP");
    assert_eq!(created["evidence_request"]["cadence"], "quarterly");
    assert_eq!(create_logs.len(), 1);
    assert_audit_event(
        &create_logs[0],
        ExpectedAuditEvent {
            event_name: "evidence_request.created",
            operation: "create_evidence_request",
            client_type: "mcp",
            workspace_id,
            user_id: app.user_id(),
            api_token_id: app.api_token_id(),
            object_type: "evidence_request",
            object_id: created_id,
        },
    );

    let listed = mcp_client
        .call_tool("list_evidence_requests", json!({}))
        .await;
    assert_eq!(
        listed["evidence_requests"]
            .as_array()
            .expect("requests array")
            .len(),
        3
    );
    assert_eq!(
        listed["evidence_requests"][0]["workspace_id"],
        workspace_id.to_string()
    );
    assert!(!listed["evidence_requests"]
        .as_array()
        .expect("requests array")
        .iter()
        .any(|request| request["title"] == "Hidden request"));
    assert!(listed["evidence_requests"]
        .as_array()
        .expect("requests array")
        .iter()
        .any(|request| request["id"] == created_id.to_string()));

    let got = mcp_client
        .call_tool(
            "get_evidence_request",
            json!({
                "evidence_request_id": due["id"],
            }),
        )
        .await;
    assert_eq!(got["evidence_request"]["title"], "Due request");

    let due_only = mcp_client
        .call_tool(
            "list_due_evidence_requests",
            json!({
                "now": "2026-02-01T00:00:00Z",
            }),
        )
        .await;
    assert_eq!(due_only["evidence_requests"][0]["id"], due["id"]);
    assert_ne!(due_only["evidence_requests"][0]["id"], later["id"]);

    let invalid = mcp_client
        .call_tool_error("list_due_evidence_requests", json!({ "now": "not-a-date" }))
        .await;
    assert_eq!(invalid.code.0, -32602);
    let fields: Vec<_> = invalid.data["problem"]["field_issues"]
        .as_array()
        .expect("field issues")
        .iter()
        .map(|issue| issue["field"].as_str().expect("field"))
        .collect();
    assert_eq!(fields, ["now"]);

    let invalid_create = mcp_client
        .call_tool_error(
            "create_evidence_request",
            json!({
                "title": "",
                "description": " ",
                "collection_instructions": "\t",
                "cadence": "weekly",
                "due_at": "not-a-date",
                "schedule_anchor_at": "also-not-a-date",
                "freshness_window_days": 0,
                "status": "draft"
            }),
        )
        .await;
    assert_eq!(invalid_create.code.0, -32602);
    assert_eq!(
        field_issue_names(&invalid_create.data),
        ["due_at", "schedule_anchor_at",]
    );

    let invalid_domain = mcp_client
        .call_tool_error(
            "create_evidence_request",
            json!({
                "title": "",
                "description": " ",
                "collection_instructions": "\t",
                "cadence": "weekly",
                "due_at": "2026-02-15T00:00:00Z",
                "schedule_anchor_at": "2026-01-01T00:00:00Z",
                "freshness_window_days": 0,
                "status": "draft"
            }),
        )
        .await;
    assert_eq!(invalid_domain.code.0, -32602);
    assert_eq!(
        field_issue_names(&invalid_domain.data),
        [
            "title",
            "description",
            "collection_instructions",
            "cadence",
            "freshness_window_days",
            "status"
        ]
    );

    let read_only = app
        .issue_api_token(
            workspace_id,
            vec![WorkspacePermission::ReadEvidenceRequests],
        )
        .await;
    let read_only_token = read_only.raw_token.clone();
    let (denied_create, denied_create_logs) = capture_audit_logs(|request_id| {
        let server = &server;
        let read_only_token = read_only_token.clone();
        async move {
            let read_only_client =
                McpClient::connect_with_request_id(server, &read_only_token, request_id).await;
            read_only_client
                .call_tool_error(
                    "create_evidence_request",
                    evidence_request("Denied request", "2026-02-15T00:00:00Z"),
                )
                .await
                .data
        }
    })
    .await;
    assert_eq!(denied_create["problem"]["code"], "not_found");
    assert!(denied_create_logs.is_empty());
}

#[tokio::test]
async fn mcp_submission_tools_preserve_selective_context() {
    let app = TestApp::builder()
        .workspace("workspace", "MCP submission workspace")
        .with_default_membership()
        .build()
        .await;
    let server = app.mcp_http_server();
    let mcp_client = McpClient::connect(&server, app.api_token()).await;
    let workspace_id = app.workspace_id("workspace");
    let request = app
        .create_evidence_request(
            workspace_id,
            &evidence_request("Submission request", "2026-04-01T00:00:00Z"),
        )
        .await;
    let evidence_request_id = uuid_from(&request["id"]);
    let created = app
        .post(&format!(
            "/workspaces/{workspace_id}/evidence-requests/{evidence_request_id}/submissions"
        ))
        .json(&json!({
            "coverage_start_at": "2026-01-01T00:00:00Z",
            "coverage_end_at": "2026-03-31T23:59:59Z",
            "source_system": "okta",
            "collection_method": "api_export",
            "summary": "Quarterly access review",
            "description": "Reviewer decisions and exceptions."
        }))
        .await
        .json::<Value>();
    let submission_id = uuid_from(&created["id"]);

    let direct = mcp_client
        .call_tool(
            "get_evidence_submission",
            json!({ "submission_id": submission_id }),
        )
        .await;
    assert_eq!(direct["submission"]["summary"], "Quarterly access review");
    assert_eq!(
        direct["submission"]["description"],
        "Reviewer decisions and exceptions."
    );
    assert_eq!(direct["submission"]["source_system"], "okta");

    let latest = mcp_client
        .call_tool(
            "get_latest_evidence_submission",
            json!({ "evidence_request_id": evidence_request_id }),
        )
        .await;
    assert_eq!(latest["submission"]["summary"], "Quarterly access review");
    assert!(latest["submission"].get("description").is_none());
    assert!(latest["submission"].get("source_system").is_none());
}

#[tokio::test]
async fn mcp_create_evidence_submission_persists_and_returns_upload_next_step() {
    let app = TestApp::builder()
        .workspace("workspace", "MCP submission create workspace")
        .with_default_membership()
        .build()
        .await;
    let server = app.mcp_http_server();
    let workspace_id = app.workspace_id("workspace");
    let request = app
        .create_evidence_request(
            workspace_id,
            &evidence_request("Create submission request", "2026-04-01T00:00:00Z"),
        )
        .await;
    let evidence_request_id = uuid_from(&request["id"]);

    let token = app.api_token().to_owned();
    let (created, logs) = capture_audit_logs(|request_id| {
        let server = &server;
        let token = token.clone();
        async move {
            let mcp_client = McpClient::connect_with_request_id(server, &token, request_id).await;
            mcp_client
                .call_tool(
                    "create_evidence_submission",
                    json!({
                        "evidence_request_id": evidence_request_id,
                        "coverage_start_at": "2026-01-01T00:00:00Z",
                        "coverage_end_at": "2026-03-31T23:59:59Z",
                        "source_system": "okta",
                        "collection_method": "api_export",
                        "summary": "Quarterly access review",
                        "description": "Reviewer decisions and exceptions."
                    }),
                )
                .await
        }
    })
    .await;
    let submission_id = uuid_from(&created["submission_id"]);

    assert_eq!(
        created["evidence_request_id"],
        evidence_request_id.to_string()
    );
    assert_eq!(created["attachment_upload"]["method"], "POST");
    assert_eq!(
        created["attachment_upload"]["path"],
        format!("/workspaces/{workspace_id}/evidence-submissions/{submission_id}/attachments")
    );
    assert_eq!(created["attachment_upload"]["multipart_file_field"], "file");
    assert_eq!(
        created["attachment_upload"]["required_file_part_header"],
        "Content-Digest"
    );
    assert_eq!(created["attachment_upload"]["transfer_mode"], "rest_only");
    assert!(created.get("summary").is_none());
    assert!(created.get("description").is_none());
    assert!(created.get("token").is_none());

    let persisted = app
        .get(&format!(
            "/workspaces/{workspace_id}/evidence-submissions/{submission_id}"
        ))
        .await
        .json::<Value>();
    assert_eq!(persisted["submission"]["source_system"], "okta");
    assert_eq!(persisted["submission"]["collection_method"], "api_export");
    assert_eq!(
        persisted["submission"]["summary"],
        "Quarterly access review"
    );
    assert_eq!(
        persisted["submission"]["description"],
        "Reviewer decisions and exceptions."
    );

    assert_eq!(logs.len(), 1);
    assert_audit_event(
        &logs[0],
        ExpectedAuditEvent {
            event_name: "evidence_submission.created",
            operation: "create_evidence_submission",
            client_type: "mcp",
            workspace_id,
            user_id: app.user_id(),
            api_token_id: app.api_token_id(),
            object_type: "evidence_submission",
            object_id: submission_id,
        },
    );
    let metadata = audit_metadata(&logs[0]);
    assert_eq!(
        metadata["evidence_request_id"],
        evidence_request_id.to_string()
    );
    assert_eq!(
        metadata["evidence_submission_id"],
        submission_id.to_string()
    );
    let serialized = serde_json::to_string(&logs).expect("logs serialize");
    assert!(!serialized.contains(app.api_token()));
    assert!(!serialized.contains("Quarterly access review"));
    assert!(!serialized.contains("Reviewer decisions and exceptions."));
}

#[tokio::test]
async fn mcp_create_evidence_submission_reports_structured_validation_errors() {
    let app = TestApp::builder()
        .workspace("workspace", "MCP submission validation workspace")
        .with_default_membership()
        .build()
        .await;
    let server = app.mcp_http_server();
    let mcp_client = McpClient::connect(&server, app.api_token()).await;
    let workspace_id = app.workspace_id("workspace");
    let request = app
        .create_evidence_request(
            workspace_id,
            &evidence_request("Validation request", "2026-04-01T00:00:00Z"),
        )
        .await;
    let evidence_request_id = uuid_from(&request["id"]);

    let invalid_args = mcp_client
        .call_tool_error(
            "create_evidence_submission",
            json!({
                "evidence_request_id": evidence_request_id,
                "coverage_start_at": "not-a-date",
                "coverage_end_at": "2026-03-31T23:59:59Z",
                "source_system": "okta",
                "collection_method": "api_export"
            }),
        )
        .await;
    assert_eq!(invalid_args.data["problem"]["code"], "validation_failed");
    assert_eq!(field_issue_names(&invalid_args.data), ["coverage_start_at"]);

    let invalid_domain = mcp_client
        .call_tool_error(
            "create_evidence_submission",
            json!({
                "evidence_request_id": evidence_request_id,
                "coverage_start_at": "2026-04-01T00:00:00Z",
                "coverage_end_at": "2026-03-31T23:59:59Z",
                "source_system": " ",
                "collection_method": "\t",
                "summary": " ",
                "description": "x".repeat(4_001)
            }),
        )
        .await;
    assert_eq!(invalid_domain.data["problem"]["code"], "validation_failed");
    assert_eq!(
        field_issue_names(&invalid_domain.data),
        [
            "source_system",
            "collection_method",
            "summary",
            "description",
            "coverage_end_at"
        ]
    );
}

#[tokio::test]
async fn mcp_attachment_management_issues_bearer_secret_urls_and_audit_success_only() {
    let app = TestApp::builder()
        .workspace("workspace", "MCP upload grant workspace")
        .with_default_membership()
        .workspace("other", "MCP upload grant hidden workspace")
        .without_membership()
        .build()
        .await;
    let server = app.mcp_http_server();
    let workspace_id = app.workspace_id("workspace");
    let submission_id = create_submission(&app, workspace_id).await;
    let token = app.api_token().to_owned();

    let (grant, logs) = capture_audit_logs(|request_id| {
        let server = &server;
        let token = token.clone();
        async move {
            let mcp_client = McpClient::connect_with_request_id(server, &token, request_id).await;
            mcp_client
                .call_tool(
                    "manage_evidence_submission_attachment",
                    json!({ "submission_id": submission_id }),
                )
                .await
        }
    })
    .await;

    let url = grant["url"].as_str().expect("upload grant URL");
    assert!(url.starts_with("https://api.proofplane.test/evidence-attachment-uploads?token="));
    assert_eq!(grant["submission_id"], submission_id.to_string());
    assert_eq!(grant["url_secret_type"], "bearer_secret");
    assert_eq!(grant["intended_use"], "human_browser_attachment_management");
    assert!(grant["expires_at"].as_str().is_some());
    assert!(grant.get("token").is_none());
    assert!(grant.get("api_token").is_none());
    assert!(grant.get("upload_session_cookie").is_none());
    assert!(grant.get("file").is_none());
    assert!(grant.get("bytes").is_none());

    assert_eq!(logs.len(), 1);
    assert_audit_event(
        &logs[0],
        ExpectedAuditEvent {
            event_name: "evidence_attachment_upload_grant.issued",
            operation: "manage_evidence_submission_attachment",
            client_type: "mcp",
            workspace_id,
            user_id: app.user_id(),
            api_token_id: app.api_token_id(),
            object_type: "evidence_submission",
            object_id: submission_id,
        },
    );
    let metadata = audit_metadata(&logs[0]);
    assert_eq!(
        metadata["evidence_submission_id"],
        submission_id.to_string()
    );
    assert!(!metadata.to_string().contains("token"));
    assert!(!metadata.to_string().contains("url"));

    let mcp_client = McpClient::connect(&server, app.api_token()).await;
    let invalid = mcp_client
        .call_tool_error(
            "manage_evidence_submission_attachment",
            json!({ "submission_id": "not-a-uuid" }),
        )
        .await;
    assert_eq!(invalid.data["problem"]["code"], "validation_failed");
    assert_eq!(field_issue_names(&invalid.data), ["submission_id"]);

    let missing = mcp_client
        .call_tool_error(
            "manage_evidence_submission_attachment",
            json!({ "submission_id": Uuid::new_v4() }),
        )
        .await;
    assert_eq!(missing.data["problem"]["code"], "not_found");

    let other_submission_id = insert_submission_row(
        &app,
        app.workspace_id("other"),
        "Hidden upload grant submission",
    )
    .await;
    let cross_workspace = mcp_client
        .call_tool_error(
            "manage_evidence_submission_attachment",
            json!({ "submission_id": other_submission_id }),
        )
        .await;
    assert_eq!(cross_workspace.data["problem"]["code"], "not_found");

    let read_only = app
        .issue_api_token(
            workspace_id,
            vec![WorkspacePermission::ReadEvidenceSubmissions],
        )
        .await;
    let (denied, denied_logs) = capture_audit_logs(|request_id| {
        let server = &server;
        let token = read_only.raw_token.clone();
        async move {
            let read_only_client =
                McpClient::connect_with_request_id(server, &token, request_id).await;
            read_only_client
                .call_tool_error(
                    "manage_evidence_submission_attachment",
                    json!({ "submission_id": submission_id }),
                )
                .await
                .data
        }
    })
    .await;
    assert_eq!(denied["problem"]["code"], "not_found");
    assert!(denied_logs.is_empty());
}

#[tokio::test]
async fn mcp_framework_tools_list_global_reference_data_without_workspace_argument() {
    let app = TestApp::builder()
        .with_soc2_reference_data()
        .workspace("workspace", "MCP framework workspace")
        .with_default_membership()
        .build()
        .await;
    let server = app.mcp_http_server();
    let mcp_client = McpClient::connect(&server, app.api_token()).await;

    let frameworks = mcp_client.call_tool("list_frameworks", json!({})).await;
    assert_eq!(
        frameworks["frameworks"][0]["id"],
        soc2_framework_id().to_string()
    );
    assert_eq!(frameworks["frameworks"][0]["code"], "soc2");

    let requirements = mcp_client
        .call_tool(
            "list_framework_requirements",
            json!({ "framework_id": soc2_framework_id() }),
        )
        .await;
    assert_eq!(
        requirement_codes(&requirements["requirements"]),
        ["CC6.1", "CC7.1"]
    );

    let missing = mcp_client
        .call_tool_error(
            "list_framework_requirements",
            json!({ "framework_id": Uuid::new_v4() }),
        )
        .await;
    assert_eq!(missing.data["problem"]["code"], "not_found");

    let limited = app
        .issue_api_token(
            app.workspace_id("workspace"),
            vec![WorkspacePermission::ReadEvidenceRequests],
        )
        .await;
    let limited_client = McpClient::connect(&server, &limited.raw_token).await;
    let denied = limited_client
        .call_tool_error("list_frameworks", json!({}))
        .await;
    assert_eq!(denied.data["problem"]["code"], "not_found");
}

#[tokio::test]
async fn mcp_control_crud_tools_create_get_replace_validate_and_audit_success_only() {
    let app = TestApp::builder()
        .with_soc2_reference_data()
        .workspace("workspace", "MCP control lifecycle workspace")
        .with_default_membership()
        .workspace("other", "MCP hidden control workspace")
        .without_membership()
        .build()
        .await;
    let server = app.mcp_http_server();
    let workspace_id = app.workspace_id("workspace");
    let token = app.api_token().to_owned();

    let (created, create_logs) = capture_audit_logs(|request_id| {
        let server = &server;
        let token = token.clone();
        async move {
            let mcp_client = McpClient::connect_with_request_id(server, &token, request_id).await;
            mcp_client
                .call_tool(
                    "create_control",
                    json!({
                        "code": "PP-MCP-01",
                        "title": "MCP access review",
                        "description": "Control description for MCP access review.",
                        "framework_requirement_ids": [cc71_id(), cc61_id()]
                    }),
                )
                .await
        }
    })
    .await;
    let control_id = uuid_from(&created["id"]);
    assert_eq!(created["workspace_id"], workspace_id.to_string());
    assert_eq!(
        requirement_codes(&created["framework_requirements"]),
        ["CC6.1", "CC7.1"]
    );
    assert_eq!(create_logs.len(), 1);
    assert_audit_event(
        &create_logs[0],
        ExpectedAuditEvent {
            event_name: "control.created",
            operation: "create_control",
            client_type: "mcp",
            workspace_id,
            user_id: app.user_id(),
            api_token_id: app.api_token_id(),
            object_type: "control",
            object_id: control_id,
        },
    );

    let mcp_client = McpClient::connect(&server, app.api_token()).await;
    let listed = mcp_client.call_tool("list_controls", json!({})).await;
    assert_eq!(listed["controls"][0]["id"], control_id.to_string());

    let got = mcp_client
        .call_tool("get_control", json!({ "control_id": control_id }))
        .await;
    assert_eq!(got["code"], "PP-MCP-01");

    let (updated, update_logs) = capture_audit_logs(|request_id| {
        let server = &server;
        let token = token.clone();
        async move {
            let mcp_client = McpClient::connect_with_request_id(server, &token, request_id).await;
            mcp_client
                .call_tool(
                    "replace_control",
                    json!({
                        "control_id": control_id,
                        "code": "PP-MCP-02",
                        "title": "Updated MCP access review",
                        "description": "Control description for updated MCP access review.",
                        "framework_requirement_ids": [cc71_id()]
                    }),
                )
                .await
        }
    })
    .await;
    assert_eq!(updated["id"], control_id.to_string());
    assert_eq!(updated["code"], "PP-MCP-02");
    assert_eq!(
        requirement_codes(&updated["framework_requirements"]),
        ["CC7.1"]
    );
    assert_eq!(update_logs.len(), 1);
    assert_audit_event(
        &update_logs[0],
        ExpectedAuditEvent {
            event_name: "control.updated",
            operation: "replace_control",
            client_type: "mcp",
            workspace_id,
            user_id: app.user_id(),
            api_token_id: app.api_token_id(),
            object_type: "control",
            object_id: control_id,
        },
    );

    let (duplicate, duplicate_logs) = capture_audit_logs(|request_id| {
        let server = &server;
        let token = token.clone();
        async move {
            let mcp_client = McpClient::connect_with_request_id(server, &token, request_id).await;
            mcp_client
                .call_tool_error(
                    "create_control",
                    json!({
                        "code": "PP-MCP-02",
                        "title": "Duplicate MCP access review",
                        "description": "Duplicate control description.",
                        "framework_requirement_ids": []
                    }),
                )
                .await
                .data
        }
    })
    .await;
    assert_eq!(duplicate["problem"]["code"], "control_code_taken");
    assert!(duplicate_logs.is_empty());

    let (unknown_requirement, unknown_requirement_logs) = capture_audit_logs(|request_id| {
        let server = &server;
        let token = token.clone();
        async move {
            let mcp_client = McpClient::connect_with_request_id(server, &token, request_id).await;
            mcp_client
                .call_tool_error(
                    "replace_control",
                    json!({
                        "control_id": control_id,
                        "code": "PP-MCP-03",
                        "title": "Unknown requirement mapping",
                        "description": "Unknown requirement control description.",
                        "framework_requirement_ids": [Uuid::new_v4()]
                    }),
                )
                .await
                .data
        }
    })
    .await;
    assert_eq!(unknown_requirement["problem"]["code"], "validation_failed");
    assert_eq!(
        field_issue_names(&unknown_requirement),
        ["framework_requirement_ids"]
    );
    assert!(unknown_requirement_logs.is_empty());

    let missing = mcp_client
        .call_tool_error("get_control", json!({ "control_id": Uuid::new_v4() }))
        .await;
    assert_eq!(missing.data["problem"]["code"], "not_found");

    let missing_replace = mcp_client
        .call_tool_error(
            "replace_control",
            json!({
                "control_id": Uuid::new_v4(),
                "code": "PP-MISSING",
                "title": "Missing control",
                "description": "Missing control description.",
                "framework_requirement_ids": []
            }),
        )
        .await;
    assert_eq!(missing_replace.data["problem"]["code"], "not_found");

    let read_only = app
        .issue_api_token(workspace_id, vec![WorkspacePermission::ReadControls])
        .await;
    let read_only_token = read_only.raw_token.clone();
    let (denied_create, denied_create_logs) = capture_audit_logs(|request_id| {
        let server = &server;
        let read_only_token = read_only_token.clone();
        async move {
            let read_only_client =
                McpClient::connect_with_request_id(server, &read_only_token, request_id).await;
            read_only_client
                .call_tool_error(
                    "create_control",
                    json!({
                        "code": "PP-DENIED",
                        "title": "Denied",
                        "description": "Denied control description.",
                        "framework_requirement_ids": []
                    }),
                )
                .await
                .data
        }
    })
    .await;
    assert_eq!(denied_create["problem"]["code"], "not_found");
    assert!(denied_create_logs.is_empty());

    let read_only_client = McpClient::connect(&server, &read_only.raw_token).await;
    let denied_replace = read_only_client
        .call_tool_error(
            "replace_control",
            json!({
                "control_id": control_id,
                "code": "PP-DENIED",
                "title": "Denied",
                "description": "Denied control description.",
                "framework_requirement_ids": []
            }),
        )
        .await;
    assert_eq!(denied_replace.data["problem"]["code"], "not_found");
}

#[tokio::test]
async fn mcp_control_tools_match_rest_visible_mappings_and_permissions() {
    let app = TestApp::builder()
        .workspace("workspace", "MCP controls workspace")
        .with_control("PP-AC-01", "Access review", vec![])
        .with_default_membership()
        .build()
        .await;
    let server = app.mcp_http_server();
    let mcp_client = McpClient::connect(&server, app.api_token()).await;
    let workspace_id = app.workspace_id("workspace");
    let control_id = app.control_id("workspace", "PP-AC-01");
    let request = app
        .create_evidence_request(
            workspace_id,
            &evidence_request("Mapped request", "2026-05-01T00:00:00Z"),
        )
        .await;
    let evidence_request_id = uuid_from(&request["id"]);
    app.post(&format!(
        "/workspaces/{workspace_id}/evidence-requests/{evidence_request_id}/control-mappings"
    ))
    .json(&json!({
        "control_id": control_id,
        "rationale": "Maps access evidence to the access review control."
    }))
    .await
    .assert_status_ok();

    let controls = mcp_client.call_tool("list_controls", json!({})).await;
    assert_eq!(controls["controls"][0]["code"], "PP-AC-01");

    let mappings = mcp_client
        .call_tool(
            "list_evidence_request_control_mappings",
            json!({ "evidence_request_id": evidence_request_id }),
        )
        .await;
    assert_eq!(
        mappings["mappings"][0]["control"]["id"],
        control_id.to_string()
    );

    let limited = app
        .issue_api_token(
            workspace_id,
            vec![WorkspacePermission::ReadEvidenceRequests],
        )
        .await;
    let limited_client = McpClient::connect(&server, &limited.raw_token).await;
    let denied = limited_client
        .call_tool_error("list_controls", json!({}))
        .await;
    assert_eq!(denied.data["problem"]["code"], "not_found");
}

#[tokio::test]
async fn mcp_mapping_write_tools_create_list_delete_and_audit_success_only() {
    let app = TestApp::builder()
        .workspace("workspace", "MCP mapping write workspace")
        .with_control("PP-AC-04", "Access review", vec![])
        .with_default_membership()
        .build()
        .await;
    let server = app.mcp_http_server();
    let mcp_client = McpClient::connect(&server, app.api_token()).await;
    let workspace_id = app.workspace_id("workspace");
    let control_id = app.control_id("workspace", "PP-AC-04");
    let request = app
        .create_evidence_request(
            workspace_id,
            &evidence_request("Mapping write request", "2026-05-01T00:00:00Z"),
        )
        .await;
    let evidence_request_id = uuid_from(&request["id"]);

    let created = mcp_client
        .call_tool(
            "map_evidence_request_to_control",
            json!({
                "evidence_request_id": evidence_request_id,
                "control_id": control_id,
                "rationale": "Maps access evidence to the access review control."
            }),
        )
        .await;
    assert_eq!(
        created["evidence_request_id"],
        evidence_request_id.to_string()
    );
    assert_eq!(created["control"]["id"], control_id.to_string());
    assert_eq!(
        created["rationale"],
        "Maps access evidence to the access review control."
    );

    let listed = mcp_client
        .call_tool(
            "list_evidence_request_control_mappings",
            json!({ "evidence_request_id": evidence_request_id }),
        )
        .await;
    assert_eq!(
        listed["mappings"][0]["control"]["id"],
        control_id.to_string()
    );

    let duplicate = mcp_client
        .call_tool_error(
            "map_evidence_request_to_control",
            json!({
                "evidence_request_id": evidence_request_id,
                "control_id": control_id,
                "rationale": "Duplicate mapping"
            }),
        )
        .await;
    assert_eq!(
        duplicate.data["problem"]["code"],
        "evidence_request_control_mapping_exists"
    );

    let removed = mcp_client
        .call_tool(
            "remove_evidence_request_control_mapping",
            json!({
                "evidence_request_id": evidence_request_id,
                "control_id": control_id
            }),
        )
        .await;
    assert_eq!(removed["removed"], true);
    assert_eq!(
        removed["evidence_request_id"],
        evidence_request_id.to_string()
    );
    assert_eq!(removed["control_id"], control_id.to_string());

    let missing = mcp_client
        .call_tool_error(
            "remove_evidence_request_control_mapping",
            json!({
                "evidence_request_id": evidence_request_id,
                "control_id": control_id
            }),
        )
        .await;
    assert_eq!(missing.data["problem"]["code"], "not_found");

    let second_control_id = app
        .post(&format!("/workspaces/{workspace_id}/controls"))
        .json(&json!({
            "code": "PP-AC-05",
            "title": "Second access review",
            "description": "Control description for second access review.",
            "framework_requirement_ids": []
        }))
        .await
        .json::<Value>()["id"]
        .as_str()
        .expect("control id")
        .parse::<Uuid>()
        .expect("control id parses");
    let token = app.api_token().to_owned();
    let (audited, logs) = capture_audit_logs(|request_id| {
        let server = &server;
        let token = token.clone();
        async move {
            let mcp_client = McpClient::connect_with_request_id(server, &token, request_id).await;
            mcp_client
                .call_tool(
                    "map_evidence_request_to_control",
                    json!({
                        "evidence_request_id": evidence_request_id,
                        "control_id": second_control_id,
                        "rationale": "Audited mapping"
                    }),
                )
                .await
        }
    })
    .await;
    assert_eq!(audited["control"]["id"], second_control_id.to_string());
    assert_eq!(logs.len(), 1);
    assert_audit_event(
        &logs[0],
        ExpectedAuditEvent {
            event_name: "evidence_request_control_mapping.created",
            operation: "map_evidence_request_to_control",
            client_type: "mcp",
            workspace_id,
            user_id: app.user_id(),
            api_token_id: app.api_token_id(),
            object_type: "evidence_request_control_mapping",
            object_id: second_control_id,
        },
    );

    let limited = app
        .issue_api_token(
            workspace_id,
            vec![WorkspacePermission::ReadEvidenceRequests],
        )
        .await;
    let limited_token = limited.raw_token.clone();
    let (denied, denied_logs) = capture_audit_logs(|request_id| {
        let server = &server;
        let limited_token = limited_token.clone();
        async move {
            let limited_client =
                McpClient::connect_with_request_id(server, &limited_token, request_id).await;
            limited_client
                .call_tool_error(
                    "map_evidence_request_to_control",
                    json!({
                        "evidence_request_id": evidence_request_id,
                        "control_id": control_id,
                        "rationale": "Denied mapping"
                    }),
                )
                .await
                .data
        }
    })
    .await;
    assert_eq!(denied["problem"]["code"], "not_found");
    assert!(denied_logs.is_empty());
}

struct McpClient {
    service: RunningService<RoleClient, ClientInfo>,
}

impl McpClient {
    async fn connect(server: &axum_test::TestServer, raw_token: &str) -> Self {
        Self::connect_with_headers(server, raw_token, HashMap::new()).await
    }

    async fn connect_with_request_id(
        server: &axum_test::TestServer,
        raw_token: &str,
        request_id: Uuid,
    ) -> Self {
        let mut headers = HashMap::new();
        headers.insert(
            REQUEST_ID_HEADER,
            HeaderValue::from_str(&request_id.to_string()).expect("request id is a header value"),
        );
        Self::connect_with_headers(server, raw_token, headers).await
    }

    async fn connect_with_headers(
        server: &axum_test::TestServer,
        raw_token: &str,
        headers: HashMap<HeaderName, HeaderValue>,
    ) -> Self {
        let uri = server
            .server_url(MCP)
            .expect("MCP server exposes HTTP URL")
            .to_string();
        let transport = StreamableHttpClientTransport::from_config(
            StreamableHttpClientTransportConfig::with_uri(uri)
                .auth_header(raw_token)
                .custom_headers(headers),
        );
        let service = ClientInfo::default()
            .serve(transport)
            .await
            .expect("MCP client initializes");
        Self { service }
    }

    async fn list_tools(&self) -> Vec<Value> {
        self.service
            .list_tools(None)
            .await
            .expect("tools list succeeds")
            .tools
            .into_iter()
            .map(|tool| serde_json::to_value(tool).expect("tool serializes"))
            .collect()
    }

    async fn call_tool(&self, name: &'static str, arguments: Value) -> Value {
        self.service
            .call_tool(call_tool_params(name, arguments))
            .await
            .unwrap_or_else(|error| panic!("{name} succeeds: {error:?}"))
            .structured_content
            .expect("tool returns structured content")
    }

    async fn call_tool_error(&self, name: &'static str, arguments: Value) -> McpError {
        match self
            .service
            .call_tool(call_tool_params(name, arguments))
            .await
        {
            Ok(result) => panic!("{name} fails, got success: {result:?}"),
            Err(ServiceError::McpError(error)) => McpError {
                code: error.code,
                data: error.data.expect("MCP error has problem data"),
            },
            Err(error) => panic!("{name} fails with MCP error, got: {error:?}"),
        }
    }
}

struct McpError {
    code: rmcp::model::ErrorCode,
    data: Value,
}

fn call_tool_params(name: &'static str, arguments: Value) -> CallToolRequestParams {
    CallToolRequestParams::new(name).with_arguments(arguments_object(arguments))
}

fn arguments_object(arguments: Value) -> JsonObject {
    match arguments {
        Value::Object(arguments) => arguments,
        other => panic!("tool arguments must be a JSON object: {other}"),
    }
}

async fn initialize(server: &axum_test::TestServer, raw_token: &str) -> axum_test::TestResponse {
    server
        .post(MCP)
        .add_header(header::AUTHORIZATION, format!("Bearer {raw_token}"))
        .add_header(header::CONTENT_TYPE, "application/json")
        .add_header(header::ACCEPT, "application/json, text/event-stream")
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-03-26",
                "capabilities": {},
                "clientInfo": {"name": "integration-test", "version": "1.0"}
            }
        }))
        .await
}

fn find_tool<'a>(tools: &'a [Value], name: &str) -> &'a Value {
    tools
        .iter()
        .find(|tool| tool["name"] == name)
        .unwrap_or_else(|| panic!("{name} tool is registered"))
}

fn assert_schema_has_property(schema: &Value, property: &str) {
    assert!(
        schema_has_property(schema, property),
        "schema exposes {property}: {schema}"
    );
}

fn assert_schema_lacks_property(schema: &Value, property: &str) {
    assert!(
        !schema_has_property(schema, property),
        "schema omits {property}: {schema}"
    );
}

fn schema_has_property(value: &Value, property: &str) -> bool {
    match value {
        Value::Object(object) => {
            object
                .get("properties")
                .and_then(Value::as_object)
                .is_some_and(|properties| properties.contains_key(property))
                || object
                    .values()
                    .any(|nested| schema_has_property(nested, property))
        }
        Value::Array(values) => values
            .iter()
            .any(|nested| schema_has_property(nested, property)),
        _ => false,
    }
}

fn assert_unauthorized(response: &axum_test::TestResponse) {
    response.assert_status(StatusCode::UNAUTHORIZED);
    assert_eq!(
        response.header(header::WWW_AUTHENTICATE),
        "Bearer realm=\"proofplane-mcp\", resource_metadata=\"https://mcp.proofplane.test/.well-known/oauth-protected-resource/mcp\""
    );
}

fn evidence_request(title: &str, due_at: &str) -> Value {
    json!({
        "title": title,
        "description": format!("Collect evidence for {title}."),
        "collection_instructions": format!("Upload the artifact for {title}."),
        "cadence": "quarterly",
        "due_at": due_at,
        "schedule_anchor_at": "2026-01-01T00:00:00Z",
        "freshness_window_days": 90,
        "status": "active"
    })
}

async fn create_submission(app: &TestApp, workspace_id: Uuid) -> Uuid {
    let request = app
        .create_evidence_request(
            workspace_id,
            &evidence_request("Download evidence", "2099-01-01T00:00:00Z"),
        )
        .await;
    let evidence_request_id = uuid_from(&request["id"]);
    let response = app
        .post(&format!(
            "/workspaces/{workspace_id}/evidence-requests/{evidence_request_id}/submissions"
        ))
        .json(&json!({
            "coverage_start_at": "2026-01-01T00:00:00Z",
            "coverage_end_at": "2026-01-31T23:59:59Z",
            "source_system": "integration",
            "collection_method": "manual_upload",
        }))
        .await;
    response.assert_status_ok();
    uuid_from(&response.json::<Value>()["id"])
}

async fn insert_submission_row(app: &TestApp, workspace_id: Uuid, title: &str) -> Uuid {
    let client = app
        .postgres()
        .get()
        .await
        .expect("submission fixture connection opens");
    let evidence_request_id = Uuid::new_v4();
    let submission_id = Uuid::new_v4();
    client
        .execute(
            r#"
INSERT INTO evidence_requests (
    id, workspace_id, title, description, collection_instructions,
    cadence, due_at, schedule_anchor_at, freshness_window_days, status
)
VALUES ($1, $2, $3, 'Seeded description', 'Seeded instructions', 'quarterly', now(), now(), 90, 'active')
"#,
            &[&evidence_request_id, &workspace_id, &title],
        )
        .await
        .expect("evidence request fixture inserts");
    client
        .execute(
            r#"
INSERT INTO evidence_submissions (
    id, evidence_request_id, submitted_by_api_token_id,
    coverage_start_at, coverage_end_at, source_system, collection_method
)
VALUES ($1, $2, $3, now(), now(), 'integration', 'manual_upload')
"#,
            &[&submission_id, &evidence_request_id, &app.api_token_id()],
        )
        .await
        .expect("evidence submission fixture inserts");

    submission_id
}

fn field_issue_names(data: &Value) -> Vec<&str> {
    data["problem"]["field_issues"]
        .as_array()
        .expect("field issues")
        .iter()
        .map(|issue| issue["field"].as_str().expect("field"))
        .collect()
}

struct ExpectedAuditEvent {
    event_name: &'static str,
    operation: &'static str,
    client_type: &'static str,
    workspace_id: Uuid,
    user_id: Uuid,
    api_token_id: Uuid,
    object_type: &'static str,
    object_id: Uuid,
}

fn assert_audit_event(record: &Value, expected: ExpectedAuditEvent) {
    let fields = &record["fields"];

    assert_eq!(fields["type"], "audit_log");
    assert_eq!(fields["event_name"], expected.event_name);
    assert_eq!(fields["outcome"], "success");
    assert_eq!(fields["actor_type"], "api_token");
    assert_eq!(fields["user_id"], expected.user_id.to_string());
    assert_eq!(fields["api_token_id"], expected.api_token_id.to_string());
    assert_eq!(fields["client_type"], expected.client_type);
    assert_eq!(fields["operation"], expected.operation);
    assert_eq!(fields["workspace_id"], expected.workspace_id.to_string());
    assert_eq!(fields["object_type"], expected.object_type);
    assert_eq!(fields["object_id"], expected.object_id.to_string());
}

fn audit_metadata(record: &Value) -> Value {
    serde_json::from_str(
        record["fields"]["metadata"]
            .as_str()
            .expect("metadata is text"),
    )
    .expect("metadata parses")
}

fn requirement_codes(list: &Value) -> Vec<&str> {
    list.as_array()
        .expect("requirements array")
        .iter()
        .map(|item| item["code"].as_str().expect("requirement code"))
        .collect()
}

fn uuid_from(value: &Value) -> Uuid {
    Uuid::parse_str(value.as_str().expect("value is a UUID string")).expect("value parses as UUID")
}
