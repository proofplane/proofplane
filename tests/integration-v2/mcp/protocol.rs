use http::header;
use proofplane::{
    domain::WorkspacePermission,
    mcp::{ENDPOINT, SESSION_ID_HEADER},
};
use rmcp::model::ErrorCode;
use serde_json::{json, Value};

use crate::support::{
    harness, mcp::McpClient, oauth::authorize_agent_connection, scenario::ScenarioBuilder,
};

use super::initialize_body;

#[tokio::test]
async fn initialize_advertises_server_identity_and_capabilities() {
    let app = harness::app().await;
    let subject = "auth0|mcp-initialize";

    ScenarioBuilder::new(&app)
        .with_user(subject)
        .with_workspace(subject, "Initialize")
        .build()
        .await;

    let token =
        authorize_agent_connection(&app, subject, "Claude", &WorkspacePermission::ALL).await;

    let response = app
        .mcp_server()
        .post(ENDPOINT)
        .add_header(header::AUTHORIZATION, format!("Bearer {token}"))
        .add_header(header::ACCEPT, "application/json, text/event-stream")
        .json(&initialize_body())
        .await;
    response.assert_status_ok();
    assert_eq!(response.header(header::CONTENT_TYPE), "application/json");
    assert_eq!(
        response.headers().get_all(SESSION_ID_HEADER).iter().count(),
        0
    );

    let body = response.json::<Value>();

    let result = &body["result"];
    assert_eq!(result["serverInfo"]["name"], "proofplane");
    assert!(
        result["instructions"]
            .as_str()
            .expect("instructions are a string")
            .starts_with("Proofplane manages SOC 2 and compliance evidence."),
        "initialization returns the real server instructions"
    );
    assert_eq!(result["capabilities"]["tools"], json!({}));
    assert_eq!(result["capabilities"]["resources"], json!({}));
}

#[tokio::test]
async fn guide_and_docs_resources_are_available_to_any_connection() {
    let app = harness::app().await;
    let subject = "auth0|mcp-guide-reader";

    ScenarioBuilder::new(&app)
        .with_user(subject)
        .with_workspace(subject, "Guide Reader")
        .build()
        .await;

    // Scoped so narrowly that every tool except the guide is out of reach.
    let token = authorize_agent_connection(
        &app,
        subject,
        "Claude",
        &[WorkspacePermission::ReadEvidence],
    )
    .await;

    let ((known, unknown, resources, resource, wrong_case, denied), audit_logs) = app
        .capture_audit_logs(async |request_id| {
            let client =
                McpClient::connect_with_request_id(app.mcp_server(), &token, request_id).await;

            let known = client
                .call_tool("get_proofplane_guide", json!({ "topic": " glossary " }))
                .await;
            let unknown = client
                .call_tool("get_proofplane_guide", json!({ "topic": "unknown-topic" }))
                .await;
            let resources = client.list_resources().await;
            let resource = client.read_resource("proofplane://docs/glossary").await;
            let wrong_case = client
                .read_resource_error("proofplane://docs/Glossary")
                .await;
            let denied = client.call_tool_error("list_controls", json!({})).await;

            (known, unknown, resources, resource, wrong_case, denied)
        })
        .await;

    // A padded topic still resolves, and a resolved topic returns no index.
    assert_eq!(known["topic"], "glossary");
    assert_eq!(known["title"], "Proofplane Glossary");
    assert!(known["markdown"]
        .as_str()
        .is_some_and(|markdown| markdown.contains("# Proofplane Glossary")));
    assert_eq!(known["topics"], json!([]));

    assert_eq!(
        unknown,
        json!({
            "topic": null,
            "title": "Proofplane guide topics",
            "markdown": "Call `get_proofplane_guide` again with one of the topics listed in `topics`.",
            "topics": [
                {"topic": "glossary", "title": "Proofplane Glossary"},
                {"topic": "submitting-evidence", "title": "Submitting Evidence"},
                {"topic": "controls-and-mappings", "title": "Controls and Mappings"},
                {"topic": "errors-and-not-found", "title": "Errors and Not Found"},
                {"topic": "policies", "title": "Policies"}
            ]
        })
    );

    assert_eq!(
        resources,
        json!({
            "resources": [
                {"uri": "proofplane://docs/glossary", "name": "glossary", "title": "Proofplane Glossary", "mimeType": "text/markdown"},
                {"uri": "proofplane://docs/submitting-evidence", "name": "submitting-evidence", "title": "Submitting Evidence", "mimeType": "text/markdown"},
                {"uri": "proofplane://docs/controls-and-mappings", "name": "controls-and-mappings", "title": "Controls and Mappings", "mimeType": "text/markdown"},
                {"uri": "proofplane://docs/errors-and-not-found", "name": "errors-and-not-found", "title": "Errors and Not Found", "mimeType": "text/markdown"},
                {"uri": "proofplane://docs/policies", "name": "policies", "title": "Policies", "mimeType": "text/markdown"}
            ]
        })
    );

    // The resource and the tool serve the same document.
    assert_eq!(resource["contents"].as_array().map(Vec::len), Some(1));
    assert_eq!(resource["contents"][0]["uri"], "proofplane://docs/glossary");
    assert_eq!(resource["contents"][0]["mimeType"], "text/markdown");
    assert_eq!(resource["contents"][0]["text"], known["markdown"]);

    assert_eq!(wrong_case.code, ErrorCode(-32002));
    assert_eq!(wrong_case.data["problem"]["code"], "not_found");

    // A missing permission is concealed as a missing resource.
    assert_eq!(denied.data["problem"]["code"], "not_found");

    assert!(audit_logs.is_empty());
}
