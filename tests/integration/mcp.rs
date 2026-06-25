use axum::http::{header, StatusCode};
use proofplane::{
    domain::WorkspacePermission,
    mcp::SESSION_ID_HEADER,
    object_storage::{FilesystemObjectStore, ObjectKey, ObjectStore},
};
use serde_json::{json, Value};
use uuid::Uuid;

use super::support::{upload_attachment, TestApp};

const MCP: &str = "/mcp";

#[tokio::test]
async fn mcp_reauthenticates_token_state_and_serves_public_operational_routes() {
    let app = TestApp::start_without_default_auth().await;
    let server = app.mcp_server();
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
    let tools = rpc_body(
        &tools_list(
            &server,
            app.api_token(),
            session_id.to_str().expect("session id is text"),
        )
        .await,
    );
    let tool_names = tools["result"]["tools"]
        .as_array()
        .expect("tools list is an array")
        .iter()
        .map(|tool| tool["name"].as_str().expect("tool has a name"))
        .collect::<Vec<_>>();
    for expected in [
        "list_evidence_requests",
        "get_evidence_request",
        "list_due_evidence_requests",
        "get_evidence_submission",
        "get_latest_evidence_submission",
        "create_attachment_download_grant",
        "list_controls",
        "list_evidence_request_control_mappings",
    ] {
        assert!(tool_names.contains(&expected), "{expected} is registered");
    }

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
    let server = app.mcp_server();
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
    let session = session_id(&initialize(&server, app.api_token()).await);

    let listed = tool_result(
        &tools_call(
            &server,
            app.api_token(),
            &session,
            2,
            "list_evidence_requests",
            json!({ "workspace_id": workspace_id }),
        )
        .await,
    );
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

    let got = tool_result(
        &tools_call(
            &server,
            app.api_token(),
            &session,
            3,
            "get_evidence_request",
            json!({
                "workspace_id": workspace_id,
                "evidence_request_id": due["id"],
            }),
        )
        .await,
    );
    assert_eq!(got["evidence_request"]["title"], "Due request");

    let due_only = tool_result(
        &tools_call(
            &server,
            app.api_token(),
            &session,
            4,
            "list_due_evidence_requests",
            json!({
                "workspace_id": workspace_id,
                "now": "2026-02-01T00:00:00Z",
            }),
        )
        .await,
    );
    assert_eq!(due_only["evidence_requests"][0]["id"], due["id"]);
    assert_ne!(due_only["evidence_requests"][0]["id"], later["id"]);

    let concealed = rpc_error(
        &tools_call(
            &server,
            app.api_token(),
            &session,
            5,
            "list_evidence_requests",
            json!({ "workspace_id": other_workspace_id }),
        )
        .await,
    );
    assert_eq!(concealed["code"], -32002);
    assert_eq!(concealed["data"]["problem"]["code"], "not_found");

    let invalid = rpc_error(
        &tools_call(
            &server,
            app.api_token(),
            &session,
            6,
            "list_due_evidence_requests",
            json!({ "workspace_id": "nope", "now": "not-a-date" }),
        )
        .await,
    );
    assert_eq!(invalid["code"], -32602);
    let fields: Vec<_> = invalid["data"]["problem"]["field_issues"]
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
    let server = app.mcp_server();
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
    let session = session_id(&initialize(&server, app.api_token()).await);

    let direct = tool_result(
        &tools_call(
            &server,
            app.api_token(),
            &session,
            2,
            "get_evidence_submission",
            json!({ "workspace_id": workspace_id, "submission_id": submission_id }),
        )
        .await,
    );
    assert_eq!(direct["submission"]["summary"], "Quarterly access review");
    assert_eq!(
        direct["submission"]["description"],
        "Reviewer decisions and exceptions."
    );
    assert_eq!(direct["submission"]["source_system"], "okta");

    let latest = tool_result(
        &tools_call(
            &server,
            app.api_token(),
            &session,
            3,
            "get_latest_evidence_submission",
            json!({ "workspace_id": workspace_id, "evidence_request_id": evidence_request_id }),
        )
        .await,
    );
    assert_eq!(latest["submission"]["summary"], "Quarterly access review");
    assert!(latest["submission"].get("description").is_none());
    assert!(latest["submission"].get("source_system").is_none());
}

#[tokio::test]
async fn mcp_attachment_download_grants_use_bearer_secret_urls_and_status_mapping() {
    let app = TestApp::builder()
        .workspace("workspace", "MCP grant workspace")
        .with_default_membership()
        .build()
        .await;
    let server = app.mcp_server();
    let workspace_id = app.workspace_id("workspace");
    let submission_id = create_submission(&app, workspace_id).await;
    let attachment =
        upload_attachment(&app, workspace_id, submission_id, "grant.txt", b"grant").await;
    let attachment_id = uuid_from(&attachment["id"]);
    let session = session_id(&initialize(&server, app.api_token()).await);

    let pending = rpc_error(
        &tools_call(
            &server,
            app.api_token(),
            &session,
            2,
            "create_attachment_download_grant",
            json!({
                "workspace_id": workspace_id,
                "submission_id": submission_id,
                "attachment_id": attachment_id,
            }),
        )
        .await,
    );
    assert_eq!(pending["data"]["problem"]["code"], "attachment_not_ready");

    finalize_attachment(&app, workspace_id, submission_id, attachment_id).await;
    let grant = tool_result(
        &tools_call(
            &server,
            app.api_token(),
            &session,
            3,
            "create_attachment_download_grant",
            json!({
                "workspace_id": workspace_id,
                "submission_id": submission_id,
                "attachment_id": attachment_id,
            }),
        )
        .await,
    );
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
    let server = app.mcp_server();
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
    let session = session_id(&initialize(&server, app.api_token()).await);

    let controls = tool_result(
        &tools_call(
            &server,
            app.api_token(),
            &session,
            2,
            "list_controls",
            json!({ "workspace_id": workspace_id }),
        )
        .await,
    );
    assert_eq!(controls["controls"][0]["code"], "PP-AC-01");

    let mappings = tool_result(
        &tools_call(
            &server,
            app.api_token(),
            &session,
            3,
            "list_evidence_request_control_mappings",
            json!({ "workspace_id": workspace_id, "evidence_request_id": evidence_request_id }),
        )
        .await,
    );
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
    let limited_session = session_id(&initialize(&server, &limited.raw_token).await);
    let denied = rpc_error(
        &tools_call(
            &server,
            &limited.raw_token,
            &limited_session,
            4,
            "list_controls",
            json!({ "workspace_id": workspace_id }),
        )
        .await,
    );
    assert_eq!(denied["data"]["problem"]["code"], "not_found");
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

async fn tools_call(
    server: &axum_test::TestServer,
    raw_token: &str,
    session_id: &str,
    id: u64,
    name: &str,
    arguments: Value,
) -> axum_test::TestResponse {
    server
        .post(MCP)
        .add_header(header::AUTHORIZATION, format!("Bearer {raw_token}"))
        .add_header(SESSION_ID_HEADER, session_id)
        .add_header(header::CONTENT_TYPE, "application/json")
        .add_header(header::ACCEPT, "application/json, text/event-stream")
        .json(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "tools/call",
            "params": {
                "name": name,
                "arguments": arguments,
            }
        }))
        .await
}

async fn tools_list(
    server: &axum_test::TestServer,
    raw_token: &str,
    session_id: &str,
) -> axum_test::TestResponse {
    server
        .post(MCP)
        .add_header(header::AUTHORIZATION, format!("Bearer {raw_token}"))
        .add_header(SESSION_ID_HEADER, session_id)
        .add_header(header::CONTENT_TYPE, "application/json")
        .add_header(header::ACCEPT, "application/json, text/event-stream")
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/list",
            "params": {}
        }))
        .await
}

fn session_id(response: &axum_test::TestResponse) -> String {
    response.assert_status_ok();
    response
        .header(SESSION_ID_HEADER)
        .to_str()
        .expect("session header is text")
        .to_owned()
}

fn tool_result(response: &axum_test::TestResponse) -> Value {
    response.assert_status_ok();
    let body = rpc_body(response);
    body["result"]["structuredContent"].clone()
}

fn rpc_error(response: &axum_test::TestResponse) -> Value {
    response.assert_status_ok();
    rpc_body(response)["error"].clone()
}

fn rpc_body(response: &axum_test::TestResponse) -> Value {
    let text = response.text();
    if let Ok(value) = serde_json::from_str(&text) {
        return value;
    }

    let data = text
        .lines()
        .filter_map(|line| {
            line.strip_prefix("data:")
                .map(str::trim_start)
                .filter(|data| !data.is_empty())
        })
        .next()
        .unwrap_or_else(|| panic!("SSE response has non-empty data line: {text:?}"));
    serde_json::from_str(data).expect("SSE data is JSON")
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

fn uuid_from(value: &Value) -> Uuid {
    Uuid::parse_str(value.as_str().expect("value is a UUID string")).expect("value parses as UUID")
}
