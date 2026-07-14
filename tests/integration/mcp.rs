use async_trait::async_trait;
use axum::http::{header, HeaderName, HeaderValue, Method, StatusCode};
use chrono::{Duration, Utc};
use proofplane::{
    authentication::auth0::{TokenVerifier, VerifiedMcpClaims, VerifyError},
    domain::{
        AgentAuthorizationTransactionId, AgentConnectionId, NewPendingAgentConnection, UserId,
        WorkspaceId, WorkspacePermission,
    },
    mcp::SESSION_ID_HEADER,
    routes::{
        protected_resource_metadata::PROTECTED_RESOURCE_METADATA_ENDPOINT,
        request_context::REQUEST_ID_HEADER,
    },
};
use rmcp::{
    model::{CallToolRequestParams, ClientInfo, JsonObject, ReadResourceRequestParams},
    service::{RoleClient, RunningService},
    transport::{
        streamable_http_client::StreamableHttpClientTransportConfig, StreamableHttpClientTransport,
    },
    ServiceError, ServiceExt,
};
use serde_json::{json, Value};
use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;
use uuid::Uuid;

use super::support::{capture_audit_logs, cc61_id, cc71_id, soc2_framework_id, TestApp};
use proofplane::services::agent_connections::digest_secret;

const MCP: &str = "/mcp";

struct StubAuth0Verifier {
    outcome: StubAuth0Outcome,
}

enum StubAuth0Outcome {
    Verified,
    VerifiedConnection {
        subject: String,
        client_id: String,
        connection_id: AgentConnectionId,
        workspace_id: WorkspaceId,
        scopes: Vec<WorkspacePermission>,
    },
    RejectCredentials,
    Unavailable,
}

#[async_trait]
impl TokenVerifier for StubAuth0Verifier {
    type Claims = VerifiedMcpClaims;

    async fn verify(&self, token: &str) -> Result<VerifiedMcpClaims, VerifyError> {
        match &self.outcome {
            StubAuth0Outcome::Verified => Ok(VerifiedMcpClaims {
                subject: "auth0|integration-user".to_owned(),
                client_id: "integration-mcp-client".to_owned(),
                scopes: vec![WorkspacePermission::ReadControls],
                connection_id: None,
                workspace_id: None,
            }),
            StubAuth0Outcome::VerifiedConnection {
                subject,
                client_id,
                connection_id,
                workspace_id,
                scopes,
            } => Ok(VerifiedMcpClaims {
                subject: subject.clone(),
                client_id: client_id.clone(),
                scopes: scopes.clone(),
                connection_id: Some(*connection_id),
                workspace_id: Some(*workspace_id),
            }),
            StubAuth0Outcome::RejectCredentials if token == "client-credentials-jwt" => {
                Err(VerifyError::MachineIdentity)
            }
            StubAuth0Outcome::RejectCredentials => Err(VerifyError::InvalidToken),
            StubAuth0Outcome::Unavailable => Err(VerifyError::JwksUnavailable),
        }
    }
}

#[tokio::test]
async fn auth0_connection_claims_activate_connection_and_authorize_protected_tools() {
    let app = TestApp::start_without_default_auth().await;
    let subject = "auth0|agent-mcp-user";
    let client_id = "agent-mcp-client";
    let resource = "https://mcp.proofplane.test/mcp";
    let user_id = app.login(subject).await;
    let workspace_id = Uuid::parse_str(
        app.create_workspace_as(subject, "Agent MCP Runtime Workspace")
            .await["id"]
            .as_str()
            .expect("workspace id is a string"),
    )
    .expect("workspace id is a UUID");
    let connection_id = AgentConnectionId::from(Uuid::new_v4());
    app.postgres()
        .create_pending_agent_connection(&NewPendingAgentConnection {
            id: connection_id,
            transaction_id: AgentAuthorizationTransactionId::from(Uuid::new_v4()),
            user_id: UserId::from(user_id),
            workspace_id: WorkspaceId::from(workspace_id),
            auth0_subject: subject.to_owned(),
            auth0_client_id: client_id.to_owned(),
            client_display_name: "Agent MCP Client".to_owned(),
            resource: resource.to_owned(),
            permissions: vec![WorkspacePermission::ReadControls],
            pending_expires_at: Utc::now() + Duration::minutes(5),
            continuation_digest: digest_secret("agent-continue"),
            nonce_digest: digest_secret("agent-nonce"),
        })
        .await
        .expect("pending connection creates");
    app.postgres()
        .consume_agent_connection_continuation(
            digest_secret("agent-continue"),
            digest_secret("agent-nonce"),
        )
        .await
        .expect("continuation consumes")
        .expect("connection is authorized");

    let server = app.mcp_http_server_with_auth0_verifier(Arc::new(StubAuth0Verifier {
        outcome: StubAuth0Outcome::VerifiedConnection {
            subject: subject.to_owned(),
            client_id: client_id.to_owned(),
            connection_id,
            workspace_id: WorkspaceId::from(workspace_id),
            scopes: vec![WorkspacePermission::ReadControls],
        },
    }));
    let client = McpClient::connect(&server, "auth0-access-token").await;

    let response = client.call_tool("list_controls", json!({})).await;
    assert!(response["controls"].as_array().is_some());

    let row = app
        .postgres()
        .get()
        .await
        .expect("database opens")
        .query_one(
            "SELECT status, activated_at IS NOT NULL, last_used_at IS NOT NULL FROM agent_connections WHERE id = $1",
            &[&Uuid::from(connection_id)],
        )
        .await
        .expect("connection loads");
    assert_eq!(row.get::<_, String>("status"), "active");
    assert!(row.get::<_, bool>(1));
    assert!(row.get::<_, bool>(2));
}

#[tokio::test]
async fn guide_is_callable_by_a_valid_connection_with_zero_permissions() {
    let app = TestApp::start_without_default_auth().await;
    let token = app.issue_api_token(app.home_workspace_id(), vec![]).await;
    let server = app.mcp_http_server();
    let raw_token = token.raw_token.clone();

    let ((known, unknown, resources, resource, bad_resource, denied), audit_logs) =
        capture_audit_logs(|request_id| {
            let raw_token = raw_token.clone();
            async move {
                let client =
                    McpClient::connect_with_request_id(&server, &raw_token, request_id).await;
                let known = client
                    .call_tool("get_proofplane_guide", json!({"topic": " glossary "}))
                    .await;
                let unknown = client
                    .call_tool("get_proofplane_guide", json!({"topic": "unknown-topic"}))
                    .await;
                let resources = client.list_resources().await;
                let resource = client.read_resource("proofplane://docs/glossary").await;
                let bad_resource = client
                    .read_resource_error("proofplane://docs/Glossary")
                    .await;
                let denied = client.call_tool_error("list_controls", json!({})).await;
                (known, unknown, resources, resource, bad_resource, denied)
            }
        })
        .await;

    assert_eq!(known["topic"], "glossary");
    assert_eq!(known["title"], "Proofplane Glossary");
    assert!(known["markdown"]
        .as_str()
        .is_some_and(|markdown| markdown.contains("# Proofplane Glossary")));
    assert_eq!(known["topics"], json!([]));
    assert_eq!(
        known
            .as_object()
            .expect("guide response is an object")
            .keys()
            .map(String::as_str)
            .collect::<BTreeSet<_>>(),
        ["markdown", "title", "topic", "topics"]
            .into_iter()
            .collect::<BTreeSet<_>>()
    );

    assert_eq!(unknown["topic"], Value::Null);
    assert_eq!(unknown["title"], "Proofplane guide topics");
    assert_eq!(
        unknown["topics"],
        json!([
            {"topic": "glossary", "title": "Proofplane Glossary"},
            {"topic": "submitting-evidence", "title": "Submitting Evidence"},
            {"topic": "controls-and-mappings", "title": "Controls and Mappings"},
            {"topic": "attachments", "title": "Attachments"},
            {"topic": "errors-and-not-found", "title": "Errors and Not Found"}
        ])
    );
    assert!(!unknown.to_string().contains("unknown-topic"));
    assert_eq!(
        resources,
        json!({
            "resources": [
                {"uri": "proofplane://docs/glossary", "name": "glossary", "title": "Proofplane Glossary", "mimeType": "text/markdown"},
                {"uri": "proofplane://docs/submitting-evidence", "name": "submitting-evidence", "title": "Submitting Evidence", "mimeType": "text/markdown"},
                {"uri": "proofplane://docs/controls-and-mappings", "name": "controls-and-mappings", "title": "Controls and Mappings", "mimeType": "text/markdown"},
                {"uri": "proofplane://docs/attachments", "name": "attachments", "title": "Attachments", "mimeType": "text/markdown"},
                {"uri": "proofplane://docs/errors-and-not-found", "name": "errors-and-not-found", "title": "Errors and Not Found", "mimeType": "text/markdown"}
            ]
        })
    );
    assert_eq!(resource["contents"].as_array().map(Vec::len), Some(1));
    assert_eq!(resource["contents"][0]["uri"], "proofplane://docs/glossary");
    assert_eq!(resource["contents"][0]["mimeType"], "text/markdown");
    assert_eq!(resource["contents"][0]["text"], known["markdown"]);
    assert_eq!(bad_resource.code, rmcp::model::ErrorCode(-32002));
    assert_eq!(bad_resource.data["problem"]["code"], "not_found");
    assert_eq!(denied.data["problem"]["code"], "not_found");
    assert!(audit_logs.is_empty());
}

#[tokio::test]
async fn auth0_connection_claims_authorize_write_tools_with_agent_provenance() {
    let app = TestApp::start_without_default_auth().await;
    let subject = "auth0|agent-mcp-writer";
    let client_id = "agent-mcp-writer-client";
    let resource = "https://mcp.proofplane.test/mcp";
    let user_id = app.login(subject).await;
    let workspace_id = Uuid::parse_str(
        app.create_workspace_as(subject, "Agent MCP Write Workspace")
            .await["id"]
            .as_str()
            .expect("workspace id is a string"),
    )
    .expect("workspace id is a UUID");
    let evidence_request_id = Uuid::new_v4();
    app.postgres()
        .get()
        .await
        .expect("database opens")
        .execute(
            r#"
INSERT INTO evidence_requests (
    id, workspace_id, title, description, collection_instructions,
    cadence, due_at, schedule_anchor_at, freshness_window_days, status
)
VALUES ($1, $2, 'Agent request', 'Description', 'Instructions', 'quarterly', now(), now(), 90, 'active')
"#,
            &[&evidence_request_id, &workspace_id],
        )
        .await
        .expect("evidence request inserts");
    let connection_id = AgentConnectionId::from(Uuid::new_v4());
    app.postgres()
        .create_pending_agent_connection(&NewPendingAgentConnection {
            id: connection_id,
            transaction_id: AgentAuthorizationTransactionId::from(Uuid::new_v4()),
            user_id: UserId::from(user_id),
            workspace_id: WorkspaceId::from(workspace_id),
            auth0_subject: subject.to_owned(),
            auth0_client_id: client_id.to_owned(),
            client_display_name: "Agent MCP Writer".to_owned(),
            resource: resource.to_owned(),
            permissions: vec![WorkspacePermission::WriteEvidenceSubmissions],
            pending_expires_at: Utc::now() + Duration::minutes(5),
            continuation_digest: digest_secret("agent-write-continue"),
            nonce_digest: digest_secret("agent-write-nonce"),
        })
        .await
        .expect("pending connection creates");
    app.postgres()
        .consume_agent_connection_continuation(
            digest_secret("agent-write-continue"),
            digest_secret("agent-write-nonce"),
        )
        .await
        .expect("continuation consumes")
        .expect("connection is authorized");

    let server = app.mcp_http_server_with_auth0_verifier(Arc::new(StubAuth0Verifier {
        outcome: StubAuth0Outcome::VerifiedConnection {
            subject: subject.to_owned(),
            client_id: client_id.to_owned(),
            connection_id,
            workspace_id: WorkspaceId::from(workspace_id),
            scopes: vec![WorkspacePermission::WriteEvidenceSubmissions],
        },
    }));
    let client = McpClient::connect(&server, "auth0-agent-write-token").await;

    let created = client
        .call_tool(
            "create_evidence_submission",
            json!({
                "evidence_request_id": evidence_request_id,
                "coverage_start_at": "2026-01-01T00:00:00Z",
                "coverage_end_at": "2026-03-31T23:59:59Z",
                "source_system": "agent",
                "collection_method": "api_export",
                "summary": "Agent-created submission"
            }),
        )
        .await;
    let submission_id = uuid_from(&created["submission_id"]);

    let submission = app
        .postgres()
        .get()
        .await
        .expect("database opens")
        .query_one(
            r#"
SELECT submitted_by_agent_connection_id
FROM evidence_submissions
WHERE id = $1
"#,
            &[&submission_id],
        )
        .await
        .expect("submission loads");
    assert_eq!(
        submission.get::<_, Option<Uuid>>("submitted_by_agent_connection_id"),
        Some(Uuid::from(connection_id))
    );

    let grant = client
        .call_tool(
            "manage_evidence_submission_attachment",
            json!({ "submission_id": submission_id }),
        )
        .await;
    assert_eq!(grant["submission_id"], submission_id.to_string());

    let grant_row = app
        .postgres()
        .get()
        .await
        .expect("database opens")
        .query_one(
            r#"
SELECT issued_via_agent_connection_id
FROM attachment_upload_grants
WHERE evidence_submission_id = $1
"#,
            &[&submission_id],
        )
        .await
        .expect("upload grant loads");
    assert_eq!(
        grant_row.get::<_, Option<Uuid>>("issued_via_agent_connection_id"),
        Some(Uuid::from(connection_id))
    );
}

#[tokio::test]
async fn auth0_principals_without_agent_connection_are_unauthorized() {
    let app = TestApp::start_without_default_auth().await;
    let server = app.mcp_http_server_with_auth0_verifier(Arc::new(StubAuth0Verifier {
        outcome: StubAuth0Outcome::Verified,
    }));

    assert_unauthorized(&initialize(&server, "auth0-access-token").await);
}

#[tokio::test]
async fn malformed_and_client_credentials_tokens_are_unauthorized() {
    let app = TestApp::start_without_default_auth().await;
    let rejected = app.mcp_http_server_with_auth0_verifier(Arc::new(StubAuth0Verifier {
        outcome: StubAuth0Outcome::RejectCredentials,
    }));
    assert_unauthorized(&initialize(&rejected, "invalid-jwt").await);
    assert_unauthorized(&initialize(&rejected, "client-credentials-jwt").await);
}

#[tokio::test]
async fn jwks_dependency_failure_is_a_server_error() {
    let app = TestApp::start_without_default_auth().await;
    let unavailable = app.mcp_http_server_with_auth0_verifier(Arc::new(StubAuth0Verifier {
        outcome: StubAuth0Outcome::Unavailable,
    }));
    initialize(&unavailable, "auth0-jwt")
        .await
        .assert_status(StatusCode::INTERNAL_SERVER_ERROR);
}

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

    let metadata = server.get(PROTECTED_RESOURCE_METADATA_ENDPOINT).await;
    metadata.assert_status_ok();
    metadata.assert_json(&json!({
        "resource": "https://mcp.proofplane.test/mcp",
        "authorization_servers": ["https://api.proofplane.test/"],
        "bearer_methods_supported": ["header"],
        "scopes_supported": [
            "read_evidence_requests",
            "write_evidence_requests",
            "read_evidence_submissions",
            "write_evidence_submissions",
            "read_controls",
            "write_controls",
            "manage_auditor_access"
        ]
    }));

    let initialized = initialize(&server, app.api_token()).await;
    initialized.assert_status_ok();
    let initialized_body = initialized
        .text()
        .lines()
        .filter_map(|line| line.strip_prefix("data: "))
        .find_map(|data| serde_json::from_str::<Value>(data).ok())
        .expect("initialize event contains a JSON-RPC response");
    assert_eq!(
        initialized_body["result"]["serverInfo"]["name"],
        "proofplane"
    );
    assert!(
        initialized_body["result"]["instructions"]
            .as_str()
            .is_some_and(|instructions| !instructions.trim().is_empty()),
        "authenticated initialization returns server instructions"
    );
    assert_eq!(
        initialized_body["result"]["capabilities"]["tools"],
        json!({})
    );
    assert_eq!(
        initialized_body["result"]["capabilities"]["resources"],
        json!({})
    );
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
        "create_auditor_access_link",
        "list_auditor_access_links",
        "revoke_auditor_access_link",
        "list_frameworks",
        "list_framework_requirements",
        "list_controls",
        "get_control",
        "create_control",
        "replace_control",
        "list_evidence_request_control_mappings",
        "map_evidence_request_to_control",
        "remove_evidence_request_control_mapping",
        "get_proofplane_guide",
    ]
    .into_iter()
    .collect::<BTreeSet<_>>();
    assert_eq!(tool_names, expected_tool_names);
    let expected_descriptions = [
        (
            "create_evidence_request",
            "Create an evidence request that states what proof to collect, how to collect it, and when it is due; for guidance, call get_proofplane_guide with topic submitting-evidence.",
        ),
        (
            "list_evidence_requests",
            "List evidence requests with their collection instructions, due dates, cadence, and status; for guidance, call get_proofplane_guide with topic submitting-evidence.",
        ),
        (
            "get_evidence_request",
            "Get one evidence request with its collection instructions, due date, cadence, and status by evidence request ID; for guidance, call get_proofplane_guide with topic submitting-evidence.",
        ),
        (
            "list_due_evidence_requests",
            "List evidence requests due at or before `now`, using the current time when `now` is omitted; for guidance, call get_proofplane_guide with topic submitting-evidence.",
        ),
        (
            "create_evidence_submission",
            "Create a submission that records proof for an evidence request; call manage_evidence_submission_attachment afterward to obtain a human-browser attachment flow; for guidance, call get_proofplane_guide with topic submitting-evidence.",
        ),
        (
            "get_evidence_submission",
            "Get one evidence submission with detailed provenance, coverage, collection, and attachment metadata by submission ID; for guidance, call get_proofplane_guide with topic submitting-evidence.",
        ),
        (
            "get_latest_evidence_submission",
            "Get the latest submission for an evidence request with compact provenance, coverage, summary, and attachment metadata; for guidance, call get_proofplane_guide with topic submitting-evidence.",
        ),
        (
            "manage_evidence_submission_attachment",
            "Create a short-lived bearer-secret browser URL for a human to upload or download an evidence submission’s attachments; file bytes never pass through MCP; for guidance, call get_proofplane_guide with topic attachments.",
        ),
        (
            "create_auditor_access_link",
            "Create a bearer-secret browser link that lets the named auditor review compliance evidence until the grant expires.",
        ),
        (
            "list_auditor_access_links",
            "List auditor access grants with email, creation, expiry, and revocation metadata without returning bearer-secret URLs.",
        ),
        (
            "revoke_auditor_access_link",
            "Revoke an auditor access grant by grant ID and return its updated metadata.",
        ),
        (
            "list_frameworks",
            "List the supported compliance frameworks that organize requirements used by controls; for guidance, call get_proofplane_guide with topic controls-and-mappings.",
        ),
        (
            "list_framework_requirements",
            "List a compliance framework’s requirements so their IDs can be assigned to controls; for guidance, call get_proofplane_guide with topic controls-and-mappings.",
        ),
        (
            "list_controls",
            "List controls that define what must be proven for compliance; for guidance, call get_proofplane_guide with topic controls-and-mappings.",
        ),
        (
            "get_control",
            "Get one control and its linked framework requirements by control ID; for guidance, call get_proofplane_guide with topic controls-and-mappings.",
        ),
        (
            "create_control",
            "Create a control that defines what must be proven and link it to the supplied framework requirement IDs; for guidance, call get_proofplane_guide with topic controls-and-mappings.",
        ),
        (
            "replace_control",
            "Replace a control’s code, title, description, and complete framework-requirement links by control ID; for guidance, call get_proofplane_guide with topic controls-and-mappings.",
        ),
        (
            "list_evidence_request_control_mappings",
            "List the controls mapped to an evidence request, including each mapping rationale; for guidance, call get_proofplane_guide with topic controls-and-mappings.",
        ),
        (
            "map_evidence_request_to_control",
            "Map an evidence request to a control with a rationale explaining how the requested proof supports it; for guidance, call get_proofplane_guide with topic controls-and-mappings.",
        ),
        (
            "remove_evidence_request_control_mapping",
            "Remove the mapping between an evidence request and a control by their IDs; for guidance, call get_proofplane_guide with topic controls-and-mappings.",
        ),
        (
            "get_proofplane_guide",
            "Return embedded Proofplane guidance for a topic, or the ordered topic index when the topic is omitted or unknown.",
        ),
    ];
    for (name, expected_description) in expected_descriptions {
        assert_eq!(
            find_tool(&tool_list, name)["description"],
            expected_description,
            "{name} exposes its expected description"
        );
    }
    let submission_description = find_tool(&tool_list, "create_evidence_submission")["description"]
        .as_str()
        .expect("submission tool has a description");
    assert!(submission_description.contains("manage_evidence_submission_attachment afterward"));
    assert!(submission_description.contains("human-browser attachment flow"));
    assert_schema_has_property(
        &find_tool(&tool_list, "get_proofplane_guide")["inputSchema"],
        "topic",
    );
    assert_schema_lacks_property(
        &find_tool(&tool_list, "get_proofplane_guide")["inputSchema"],
        "workspace_id",
    );
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
        &find_tool(&tool_list, "create_auditor_access_link")["inputSchema"],
        "email",
    );
    assert_schema_has_property(
        &find_tool(&tool_list, "create_auditor_access_link")["inputSchema"],
        "expires_at",
    );
    assert_schema_has_property(
        &find_tool(&tool_list, "revoke_auditor_access_link")["inputSchema"],
        "grant_id",
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
    assert_schema_has_property(
        &find_tool(&tool_list, "create_auditor_access_link")["outputSchema"],
        "url",
    );
    assert_schema_has_property(
        &find_tool(&tool_list, "create_auditor_access_link")["outputSchema"],
        "grant",
    );
    for property in [
        "id",
        "auditor_email",
        "created_at",
        "expires_at",
        "revoked_at",
    ] {
        assert_schema_has_property(
            &find_tool(&tool_list, "create_auditor_access_link")["outputSchema"],
            property,
        );
        assert_schema_has_property(
            &find_tool(&tool_list, "list_auditor_access_links")["outputSchema"],
            property,
        );
        assert_schema_has_property(
            &find_tool(&tool_list, "revoke_auditor_access_link")["outputSchema"],
            property,
        );
    }
    assert_schema_has_property(
        &find_tool(&tool_list, "create_auditor_access_link")["outputSchema"],
        "url_secret_type",
    );
    assert_schema_has_property(
        &find_tool(&tool_list, "create_auditor_access_link")["outputSchema"],
        "intended_use",
    );
    assert_schema_has_property(
        &find_tool(&tool_list, "list_auditor_access_links")["outputSchema"],
        "grants",
    );
    assert_schema_has_property(
        &find_tool(&tool_list, "revoke_auditor_access_link")["outputSchema"],
        "grant",
    );
    for property in ["topic", "title", "markdown", "topics"] {
        assert_schema_has_property(
            &find_tool(&tool_list, "get_proofplane_guide")["outputSchema"],
            property,
        );
    }

    client
        .execute(
            "UPDATE agent_connections SET status = 'revoked', revoked_at = now() WHERE id = $1",
            &[&app.api_token_id()],
        )
        .await
        .expect("agent connection revokes");
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
            "UPDATE agent_connections SET status = 'revoked', revoked_at = now() WHERE id = $1",
            &[&Uuid::from(expired.token_id)],
        )
        .await
        .expect("agent connection revokes");
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
async fn mcp_cors_is_available_for_browser_based_clients() {
    let app = TestApp::builder().without_default_auth().build().await;
    let server = app.mcp_http_server();

    let metadata = server
        .get(PROTECTED_RESOURCE_METADATA_ENDPOINT)
        .add_header(header::ORIGIN.as_str(), "http://localhost:6274")
        .await;
    metadata.assert_status_ok();
    metadata.assert_header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*");

    let preflight = server
        .method(Method::OPTIONS, PROTECTED_RESOURCE_METADATA_ENDPOINT)
        .add_header(header::ORIGIN.as_str(), "http://localhost:6274")
        .add_header(header::ACCESS_CONTROL_REQUEST_METHOD.as_str(), "GET")
        .await;
    preflight.assert_status_ok();
    preflight.assert_header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*");
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
                    mcp_evidence_request("Created by MCP", "2026-02-15T00:00:00Z"),
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
    assert_eq!(created["evidence_request"]["status"], "active");
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
                "freshness_window_days": 0
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
                "freshness_window_days": 0
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
            "freshness_window_days"
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
                    mcp_evidence_request("Denied request", "2026-02-15T00:00:00Z"),
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
        .create_evidence_submission(
            workspace_id,
            evidence_request_id,
            &json!({
            "coverage_start_at": "2026-01-01T00:00:00Z",
            "coverage_end_at": "2026-03-31T23:59:59Z",
            "source_system": "okta",
            "collection_method": "api_export",
            "summary": "Quarterly access review",
            "description": "Reviewer decisions and exceptions."
            }),
        )
        .await;
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
    assert!(created.get("summary").is_none());
    assert!(created.get("description").is_none());
    assert!(created.get("token").is_none());

    let persisted = app
        .postgres()
        .get()
        .await
        .expect("database opens")
        .query_one(
            "SELECT source_system, collection_method, summary, description FROM evidence_submissions WHERE id = $1",
            &[&submission_id],
        )
        .await
        .expect("submission loads");
    assert_eq!(persisted.get::<_, String>("source_system"), "okta");
    assert_eq!(
        persisted.get::<_, String>("collection_method"),
        "api_export"
    );
    assert_eq!(
        persisted.get::<_, Option<String>>("summary").as_deref(),
        Some("Quarterly access review")
    );
    assert_eq!(
        persisted.get::<_, Option<String>>("description").as_deref(),
        Some("Reviewer decisions and exceptions.")
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
async fn mcp_auditor_link_tools_create_list_revoke_and_audit_without_secrets() {
    let app = TestApp::start().await;
    let server = app.mcp_http_server();
    let workspace_id = app.home_workspace_id();
    let token = app.api_token().to_owned();

    let (created, create_logs) = capture_audit_logs(|request_id| {
        let server = &server;
        let token = token.clone();
        async move {
            let mcp_client = McpClient::connect_with_request_id(server, &token, request_id).await;
            mcp_client
                .call_tool(
                    "create_auditor_access_link",
                    json!({
                        "email": " Auditor@Example.COM ",
                        "expires_at": "2099-01-01T00:00:00Z"
                    }),
                )
                .await
        }
    })
    .await;

    let url = created["url"].as_str().expect("auditor link URL");
    assert!(url.starts_with(&format!(
        "https://api.proofplane.test/auditor-access/{workspace_id}?token="
    )));
    let invite_token = url
        .split("token=")
        .nth(1)
        .expect("auditor URL contains token");
    let grant = &created["grant"];
    let grant_id = uuid_from(&grant["id"]);
    assert_eq!(grant["auditor_email"], "auditor@example.com");
    assert!(grant["created_at"].as_str().is_some());
    assert_eq!(grant["expires_at"], "2099-01-01T00:00:00.000Z");
    assert!(grant["revoked_at"].is_null());
    assert_eq!(created["url_secret_type"], "bearer_secret");
    assert_eq!(created["intended_use"], "auditor_browser_access");
    assert_secret_fields_absent(&created);

    assert_eq!(create_logs.len(), 1);
    assert_audit_event(
        &create_logs[0],
        ExpectedAuditEvent {
            event_name: "auditor_access_grant.created",
            operation: "create_auditor_access_link",
            client_type: "mcp",
            workspace_id,
            user_id: app.user_id(),
            api_token_id: app.api_token_id(),
            object_type: "auditor_access_grant",
            object_id: grant_id,
        },
    );
    let create_metadata = audit_metadata(&create_logs[0]);
    assert_eq!(create_metadata["auditor_email"], "auditor@example.com");
    assert_eq!(create_metadata["expires_at"], "2099-01-01T00:00:00.000Z");
    assert!(!create_logs[0].to_string().contains(url));
    assert!(!create_logs[0].to_string().contains(invite_token));

    let mcp_client = McpClient::connect(&server, app.api_token()).await;
    let listed = mcp_client
        .call_tool("list_auditor_access_links", json!({}))
        .await;
    assert_eq!(listed["grants"][0]["id"], grant_id.to_string());
    assert_eq!(listed["grants"][0]["auditor_email"], "auditor@example.com");
    assert_secret_fields_absent(&listed);

    let (revoked, revoke_logs) = capture_audit_logs(|request_id| {
        let server = &server;
        let token = token.clone();
        async move {
            let mcp_client = McpClient::connect_with_request_id(server, &token, request_id).await;
            mcp_client
                .call_tool(
                    "revoke_auditor_access_link",
                    json!({ "grant_id": grant_id }),
                )
                .await
        }
    })
    .await;
    assert_eq!(revoked["grant"]["id"], grant_id.to_string());
    assert!(revoked["grant"]["revoked_at"].as_str().is_some());
    assert_secret_fields_absent(&revoked);

    assert_eq!(revoke_logs.len(), 1);
    assert_audit_event(
        &revoke_logs[0],
        ExpectedAuditEvent {
            event_name: "auditor_access_grant.revoked",
            operation: "revoke_auditor_access_link",
            client_type: "mcp",
            workspace_id,
            user_id: app.user_id(),
            api_token_id: app.api_token_id(),
            object_type: "auditor_access_grant",
            object_id: grant_id,
        },
    );
    assert!(!revoke_logs[0].to_string().contains(url));
    assert!(!revoke_logs[0].to_string().contains(invite_token));
}

#[tokio::test]
async fn mcp_auditor_link_tools_validate_and_conceal_denied_access() {
    let app = TestApp::builder()
        .workspace("workspace", "MCP auditor link workspace")
        .with_default_membership()
        .workspace("other", "MCP auditor link hidden workspace")
        .without_membership()
        .build()
        .await;
    let server = app.mcp_http_server();
    let workspace_id = app.workspace_id("workspace");
    let mcp_client = McpClient::connect(&server, app.api_token()).await;

    let invalid_email = mcp_client
        .call_tool_error(
            "create_auditor_access_link",
            json!({ "email": "not-an-email" }),
        )
        .await;
    assert_eq!(invalid_email.data["problem"]["code"], "validation_failed");
    assert_eq!(field_issue_names(&invalid_email.data), ["email"]);

    let invalid_expiry_timestamp = mcp_client
        .call_tool_error(
            "create_auditor_access_link",
            json!({ "email": "auditor@example.com", "expires_at": "tomorrow" }),
        )
        .await;
    assert_eq!(
        invalid_expiry_timestamp.data["problem"]["code"],
        "validation_failed"
    );
    assert_eq!(
        field_issue_names(&invalid_expiry_timestamp.data),
        ["expires_at"]
    );

    let past_expiry = mcp_client
        .call_tool_error(
            "create_auditor_access_link",
            json!({ "email": "auditor@example.com", "expires_at": "2000-01-01T00:00:00Z" }),
        )
        .await;
    assert_eq!(past_expiry.data["problem"]["code"], "validation_failed");
    assert_eq!(field_issue_names(&past_expiry.data), ["expires_at"]);

    let invalid_grant_id = mcp_client
        .call_tool_error(
            "revoke_auditor_access_link",
            json!({ "grant_id": "not-a-uuid" }),
        )
        .await;
    assert_eq!(
        invalid_grant_id.data["problem"]["code"],
        "validation_failed"
    );
    assert_eq!(field_issue_names(&invalid_grant_id.data), ["grant_id"]);

    let created = mcp_client
        .call_tool(
            "create_auditor_access_link",
            json!({ "email": "auditor@example.com" }),
        )
        .await;
    let grant_id = uuid_from(&created["grant"]["id"]);
    let other_workspace = app
        .issue_api_token(
            app.workspace_id("other"),
            vec![WorkspacePermission::ManageAuditorAccess],
        )
        .await;
    let (cross_workspace, cross_workspace_logs) = capture_audit_logs(|request_id| {
        let server = &server;
        let token = other_workspace.raw_token.clone();
        async move {
            let other_client = McpClient::connect_with_request_id(server, &token, request_id).await;
            other_client
                .call_tool_error(
                    "revoke_auditor_access_link",
                    json!({ "grant_id": grant_id }),
                )
                .await
                .data
        }
    })
    .await;
    assert_eq!(cross_workspace["problem"]["code"], "not_found");
    assert!(cross_workspace_logs.is_empty());

    let read_only = app
        .issue_api_token(workspace_id, vec![WorkspacePermission::ReadControls])
        .await;
    let (denied, denied_logs) = capture_audit_logs(|request_id| {
        let server = &server;
        let token = read_only.raw_token.clone();
        async move {
            let read_only_client =
                McpClient::connect_with_request_id(server, &token, request_id).await;
            read_only_client
                .call_tool_error(
                    "create_auditor_access_link",
                    json!({ "email": "auditor@example.com" }),
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
    insert_control_mapping_row(
        &app,
        evidence_request_id,
        control_id,
        "Maps access evidence to the access review control.",
    )
    .await;

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

    let second_control_id =
        insert_control_row(&app, workspace_id, "PP-AC-05", "Second access review").await;
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

    async fn list_resources(&self) -> Value {
        let result = self
            .service
            .list_resources(None)
            .await
            .expect("resources list succeeds");
        serde_json::to_value(result).expect("resource list serializes")
    }

    async fn read_resource(&self, uri: &str) -> Value {
        let result = self
            .service
            .read_resource(ReadResourceRequestParams::new(uri))
            .await
            .unwrap_or_else(|error| panic!("resource {uri} reads: {error:?}"));
        serde_json::to_value(result).expect("resource result serializes")
    }

    async fn read_resource_error(&self, uri: &str) -> McpError {
        match self
            .service
            .read_resource(ReadResourceRequestParams::new(uri))
            .await
        {
            Ok(result) => panic!("resource {uri} fails, got success: {result:?}"),
            Err(ServiceError::McpError(error)) => McpError {
                code: error.code,
                data: error.data.expect("MCP error has problem data"),
            },
            Err(error) => panic!("resource {uri} fails with MCP error, got: {error:?}"),
        }
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
        "Bearer realm=\"proofplane-mcp\", resource_metadata=\"https://mcp.proofplane.test/.well-known/oauth-protected-resource/mcp\", scope=\"read_evidence_requests write_evidence_requests read_evidence_submissions write_evidence_submissions read_controls write_controls manage_auditor_access\""
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

fn mcp_evidence_request(title: &str, due_at: &str) -> Value {
    json!({
        "title": title,
        "description": format!("Collect evidence for {title}."),
        "collection_instructions": format!("Upload the artifact for {title}."),
        "cadence": "quarterly",
        "due_at": due_at,
        "schedule_anchor_at": "2026-01-01T00:00:00Z",
        "freshness_window_days": 90
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
        .create_evidence_submission(
            workspace_id,
            evidence_request_id,
            &json!({
            "coverage_start_at": "2026-01-01T00:00:00Z",
            "coverage_end_at": "2026-01-31T23:59:59Z",
            "source_system": "integration",
            "collection_method": "manual_upload",
            }),
        )
        .await;
    uuid_from(&response["id"])
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
    id, evidence_request_id, submitted_by_agent_connection_id,
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

async fn insert_control_row(app: &TestApp, workspace_id: Uuid, code: &str, title: &str) -> Uuid {
    let control_id = Uuid::new_v4();
    app.postgres()
        .get()
        .await
        .expect("control fixture connection opens")
        .execute(
            r#"
INSERT INTO controls (id, workspace_id, code, title, description)
VALUES ($1, $2, $3, $4, $5)
"#,
            &[
                &control_id,
                &workspace_id,
                &code,
                &title,
                &format!("Control description for {title}."),
            ],
        )
        .await
        .expect("control fixture inserts");

    control_id
}

async fn insert_control_mapping_row(
    app: &TestApp,
    evidence_request_id: Uuid,
    control_id: Uuid,
    rationale: &str,
) {
    app.postgres()
        .get()
        .await
        .expect("control mapping fixture connection opens")
        .execute(
            r#"
INSERT INTO evidence_request_control_mappings (evidence_request_id, control_id, rationale)
VALUES ($1, $2, $3)
"#,
            &[&evidence_request_id, &control_id, &rationale],
        )
        .await
        .expect("control mapping fixture inserts");
}

fn field_issue_names(data: &Value) -> Vec<&str> {
    data["problem"]["field_issues"]
        .as_array()
        .expect("field issues")
        .iter()
        .map(|issue| issue["field"].as_str().expect("field"))
        .collect()
}

fn assert_secret_fields_absent(value: &Value) {
    match value {
        Value::Object(object) => {
            for (key, nested) in object {
                assert!(
                    !["token", "secret", "raw_secret", "secret_digest"].contains(&key.as_str()),
                    "secret-bearing field {key} is absent from {value}"
                );
                assert_secret_fields_absent(nested);
            }
        }
        Value::Array(values) => {
            for nested in values {
                assert_secret_fields_absent(nested);
            }
        }
        _ => {}
    }
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
    assert_eq!(fields["actor_type"], "agent_connection");
    assert_eq!(fields["user_id"], expected.user_id.to_string());
    assert_eq!(
        fields["agent_connection_id"],
        expected.api_token_id.to_string()
    );
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
