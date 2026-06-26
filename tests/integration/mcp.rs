use axum::http::{header, StatusCode};
use proofplane::{
    domain::WorkspacePermission,
    mcp::SESSION_ID_HEADER,
    object_storage::{FilesystemObjectStore, ObjectKey, ObjectStore},
    routes::request_context::REQUEST_ID_HEADER,
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
use std::collections::BTreeSet;
use uuid::Uuid;

use super::support::{capture_audit_logs, upload_attachment, TestApp};

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
        "list_evidence_requests",
        "get_evidence_request",
        "list_due_evidence_requests",
        "get_evidence_submission",
        "get_latest_evidence_submission",
        "create_evidence_submission",
        "create_attachment_download_grant",
        "list_controls",
        "list_evidence_request_control_mappings",
        "map_evidence_request_to_control",
        "remove_evidence_request_control_mapping",
    ]
    .into_iter()
    .collect::<BTreeSet<_>>();
    assert_eq!(tool_names, expected_tool_names);
    assert_schema_has_property(
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
        &find_tool(&tool_list, "create_attachment_download_grant")["inputSchema"],
        "attachment_id",
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
        &find_tool(&tool_list, "list_evidence_requests")["outputSchema"],
        "evidence_requests",
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
        &find_tool(&tool_list, "create_attachment_download_grant")["outputSchema"],
        "url_secret_type",
    );
    assert_schema_has_property(
        &find_tool(&tool_list, "create_attachment_download_grant")["outputSchema"],
        "expires_at",
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

    let listed = mcp_client
        .call_tool(
            "list_evidence_requests",
            json!({ "workspace_id": workspace_id }),
        )
        .await;
    assert_eq!(
        listed["evidence_requests"]
            .as_array()
            .expect("requests array")
            .len(),
        2
    );
    assert_eq!(
        listed["evidence_requests"][0]["workspace_id"],
        workspace_id.to_string()
    );

    let got = mcp_client
        .call_tool(
            "get_evidence_request",
            json!({
                "workspace_id": workspace_id,
                "evidence_request_id": due["id"],
            }),
        )
        .await;
    assert_eq!(got["evidence_request"]["title"], "Due request");

    let due_only = mcp_client
        .call_tool(
            "list_due_evidence_requests",
            json!({
                "workspace_id": workspace_id,
                "now": "2026-02-01T00:00:00Z",
            }),
        )
        .await;
    assert_eq!(due_only["evidence_requests"][0]["id"], due["id"]);
    assert_ne!(due_only["evidence_requests"][0]["id"], later["id"]);

    let concealed = mcp_client
        .call_tool_error(
            "list_evidence_requests",
            json!({ "workspace_id": other_workspace_id }),
        )
        .await;
    assert_eq!(concealed.code.0, -32002);
    assert_eq!(concealed.data["problem"]["code"], "not_found");

    let invalid = mcp_client
        .call_tool_error(
            "list_due_evidence_requests",
            json!({ "workspace_id": "nope", "now": "not-a-date" }),
        )
        .await;
    assert_eq!(invalid.code.0, -32602);
    let fields: Vec<_> = invalid.data["problem"]["field_issues"]
        .as_array()
        .expect("field issues")
        .iter()
        .map(|issue| issue["field"].as_str().expect("field"))
        .collect();
    assert_eq!(fields, ["workspace_id", "now"]);
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
            json!({ "workspace_id": workspace_id, "submission_id": submission_id }),
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
            json!({ "workspace_id": workspace_id, "evidence_request_id": evidence_request_id }),
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

    let session_id = initialize(&server, app.api_token())
        .await
        .header(SESSION_ID_HEADER)
        .to_str()
        .expect("session id is text")
        .to_owned();
    let (created, logs) = capture_audit_logs(|request_id| {
        raw_call_tool(
            &server,
            app.api_token(),
            &session_id,
            request_id,
            "create_evidence_submission",
            json!({
                "workspace_id": workspace_id,
                "evidence_request_id": evidence_request_id,
                "coverage_start_at": "2026-01-01T00:00:00Z",
                "coverage_end_at": "2026-03-31T23:59:59Z",
                "source_system": "okta",
                "collection_method": "api_export",
                "summary": "Quarterly access review",
                "description": "Reviewer decisions and exceptions."
            }),
        )
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
        "evidence_submission.created",
        "create_evidence_submission",
        "mcp",
        workspace_id,
        app.user_id(),
        app.api_token_id(),
        "evidence_submission",
        submission_id,
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
                "workspace_id": "not-a-uuid",
                "evidence_request_id": evidence_request_id,
                "coverage_start_at": "not-a-date",
                "coverage_end_at": "2026-03-31T23:59:59Z",
                "source_system": "okta",
                "collection_method": "api_export"
            }),
        )
        .await;
    assert_eq!(invalid_args.data["problem"]["code"], "validation_failed");
    assert_eq!(
        field_issue_names(&invalid_args.data),
        ["workspace_id", "coverage_start_at"]
    );

    let invalid_domain = mcp_client
        .call_tool_error(
            "create_evidence_submission",
            json!({
                "workspace_id": workspace_id,
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
async fn mcp_attachment_download_grants_use_bearer_secret_urls_and_status_mapping() {
    let app = TestApp::builder()
        .workspace("workspace", "MCP grant workspace")
        .with_default_membership()
        .build()
        .await;
    let server = app.mcp_http_server();
    let mcp_client = McpClient::connect(&server, app.api_token()).await;
    let workspace_id = app.workspace_id("workspace");
    let submission_id = create_submission(&app, workspace_id).await;
    let attachment =
        upload_attachment(&app, workspace_id, submission_id, "grant.txt", b"grant").await;
    let attachment_id = uuid_from(&attachment["id"]);

    let pending = mcp_client
        .call_tool_error(
            "create_attachment_download_grant",
            json!({
                "workspace_id": workspace_id,
                "submission_id": submission_id,
                "attachment_id": attachment_id,
            }),
        )
        .await;
    assert_eq!(pending.data["problem"]["code"], "attachment_not_ready");

    finalize_attachment(&app, workspace_id, submission_id, attachment_id).await;
    let grant = mcp_client
        .call_tool(
            "create_attachment_download_grant",
            json!({
                "workspace_id": workspace_id,
                "submission_id": submission_id,
                "attachment_id": attachment_id,
            }),
        )
        .await;
    let url = grant["url"].as_str().expect("grant URL");
    assert!(url.starts_with("https://api.proofplane.test/attachment-downloads?token="));
    assert_eq!(grant["url_secret_type"], "bearer_secret");
    assert_eq!(grant["intended_use"], "human_presentation");
    assert!(grant.get("token").is_none());
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

    let controls = mcp_client
        .call_tool("list_controls", json!({ "workspace_id": workspace_id }))
        .await;
    assert_eq!(controls["controls"][0]["code"], "PP-AC-01");

    let mappings = mcp_client
        .call_tool(
            "list_evidence_request_control_mappings",
            json!({ "workspace_id": workspace_id, "evidence_request_id": evidence_request_id }),
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
        .call_tool_error("list_controls", json!({ "workspace_id": workspace_id }))
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
                "workspace_id": workspace_id,
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
            json!({ "workspace_id": workspace_id, "evidence_request_id": evidence_request_id }),
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
                "workspace_id": workspace_id,
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
                "workspace_id": workspace_id,
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
                "workspace_id": workspace_id,
                "evidence_request_id": evidence_request_id,
                "control_id": control_id
            }),
        )
        .await;
    assert_eq!(missing.data["problem"]["code"], "not_found");

    let session_id = initialize(&server, app.api_token())
        .await
        .header(SESSION_ID_HEADER)
        .to_str()
        .expect("session id is text")
        .to_owned();
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
    let (audited, logs) = capture_audit_logs(|request_id| {
        raw_call_tool(
            &server,
            app.api_token(),
            &session_id,
            request_id,
            "map_evidence_request_to_control",
            json!({
                "workspace_id": workspace_id,
                "evidence_request_id": evidence_request_id,
                "control_id": second_control_id,
                "rationale": "Audited mapping"
            }),
        )
    })
    .await;
    assert_eq!(audited["control"]["id"], second_control_id.to_string());
    assert_eq!(logs.len(), 1);
    assert_audit_event(
        &logs[0],
        "evidence_request_control_mapping.created",
        "map_evidence_request_to_control",
        "mcp",
        workspace_id,
        app.user_id(),
        app.api_token_id(),
        "evidence_request_control_mapping",
        second_control_id,
    );

    let limited = app
        .issue_api_token(
            workspace_id,
            vec![WorkspacePermission::ReadEvidenceRequests],
        )
        .await;
    let limited_session = initialize(&server, &limited.raw_token)
        .await
        .header(SESSION_ID_HEADER)
        .to_str()
        .expect("session id is text")
        .to_owned();
    let (denied, denied_logs) = capture_audit_logs(|request_id| {
        raw_call_tool_error(
            &server,
            &limited.raw_token,
            &limited_session,
            request_id,
            "map_evidence_request_to_control",
            json!({
                "workspace_id": workspace_id,
                "evidence_request_id": evidence_request_id,
                "control_id": control_id,
                "rationale": "Denied mapping"
            }),
        )
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
        let uri = server
            .server_url(MCP)
            .expect("MCP server exposes HTTP URL")
            .to_string();
        let transport = StreamableHttpClientTransport::from_config(
            StreamableHttpClientTransportConfig::with_uri(uri).auth_header(raw_token),
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

async fn raw_call_tool(
    server: &axum_test::TestServer,
    raw_token: &str,
    session_id: &str,
    request_id: Uuid,
    name: &'static str,
    arguments: Value,
) -> Value {
    let response =
        raw_call_tool_response(server, raw_token, session_id, request_id, name, arguments).await;
    if let Some(error) = response.get("error") {
        panic!("{name} succeeds, got error: {error}");
    }

    response["result"]["structuredContent"].clone()
}

async fn raw_call_tool_error(
    server: &axum_test::TestServer,
    raw_token: &str,
    session_id: &str,
    request_id: Uuid,
    name: &'static str,
    arguments: Value,
) -> Value {
    let response =
        raw_call_tool_response(server, raw_token, session_id, request_id, name, arguments).await;
    response["error"]["data"].clone()
}

async fn raw_call_tool_response(
    server: &axum_test::TestServer,
    raw_token: &str,
    session_id: &str,
    request_id: Uuid,
    name: &'static str,
    arguments: Value,
) -> Value {
    let response = server
        .post(MCP)
        .add_header(header::AUTHORIZATION, format!("Bearer {raw_token}"))
        .add_header(header::CONTENT_TYPE, "application/json")
        .add_header(header::ACCEPT, "application/json, text/event-stream")
        .add_header(SESSION_ID_HEADER, session_id)
        .add_header(REQUEST_ID_HEADER.as_str(), request_id.to_string())
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": {
                "name": name,
                "arguments": arguments
            }
        }))
        .await;
    response.assert_status_ok();
    let body = response.text();
    body.lines()
        .filter_map(|line| line.strip_prefix("data: "))
        .filter(|data| data.starts_with('{'))
        .last()
        .map(|data| serde_json::from_str(data).expect("SSE data is JSON"))
        .unwrap_or_else(|| panic!("MCP response includes JSON data event: {body}"))
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
        "Bearer realm=\"proofplane-mcp\""
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

async fn finalize_attachment(
    app: &TestApp,
    workspace_id: Uuid,
    submission_id: Uuid,
    attachment_id: Uuid,
) {
    let client = app.postgres().get().await.expect("connection opens");
    let row = client
        .query_one(
            "SELECT object_key, filename FROM evidence_attachments WHERE id = $1",
            &[&attachment_id],
        )
        .await
        .expect("attachment loads");
    let quarantine_key =
        ObjectKey::parse(row.get::<_, String>("object_key")).expect("quarantine key parses");
    let filename: String = row.get("filename");
    let final_key = ObjectKey::parse(format!(
        "workspaces/{workspace_id}/evidence-submissions/{submission_id}/attachments/{attachment_id}/{filename}"
    ))
    .expect("final key parses");
    let store = FilesystemObjectStore::new(app.object_storage_root())
        .await
        .expect("filesystem store initializes");
    store
        .copy_object(&quarantine_key, &final_key)
        .await
        .expect("attachment copies to final storage");
    client
        .execute(
            "UPDATE evidence_attachments SET object_key = $2, upload_status = 'uploaded' WHERE id = $1",
            &[&attachment_id, &final_key.as_str()],
        )
        .await
        .expect("attachment finalizes");
}

fn field_issue_names(data: &Value) -> Vec<&str> {
    data["problem"]["field_issues"]
        .as_array()
        .expect("field issues")
        .iter()
        .map(|issue| issue["field"].as_str().expect("field"))
        .collect()
}

fn assert_audit_event(
    record: &Value,
    event_name: &str,
    operation: &str,
    client_type: &str,
    workspace_id: Uuid,
    user_id: Uuid,
    api_token_id: Uuid,
    object_type: &str,
    object_id: Uuid,
) {
    let fields = &record["fields"];

    assert_eq!(fields["type"], "audit_log");
    assert_eq!(fields["event_name"], event_name);
    assert_eq!(fields["outcome"], "success");
    assert_eq!(fields["actor_type"], "api_token");
    assert_eq!(fields["user_id"], user_id.to_string());
    assert_eq!(fields["api_token_id"], api_token_id.to_string());
    assert_eq!(fields["client_type"], client_type);
    assert_eq!(fields["operation"], operation);
    assert_eq!(fields["workspace_id"], workspace_id.to_string());
    assert_eq!(fields["object_type"], object_type);
    assert_eq!(fields["object_id"], object_id.to_string());
}

fn audit_metadata(record: &Value) -> Value {
    serde_json::from_str(
        record["fields"]["metadata"]
            .as_str()
            .expect("metadata is text"),
    )
    .expect("metadata parses")
}

fn uuid_from(value: &Value) -> Uuid {
    Uuid::parse_str(value.as_str().expect("value is a UUID string")).expect("value parses as UUID")
}
