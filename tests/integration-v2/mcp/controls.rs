use proofplane::domain::WorkspacePermission;
use rmcp::model::ErrorCode;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::support::{
    agent_connections::get_agent_connection_id_for,
    harness,
    json::{assert_rfc3339, object_keys},
    mcp::{assert_not_found, assert_validation_error, McpClient, McpError},
    oauth::authorize_agent_connection,
    scenario::{
        types::{TestFramework, TestFrameworkRequirement},
        ScenarioBuilder,
    },
};

#[tokio::test]
async fn framework_tools_return_the_complete_seeded_catalog_and_conceal_unavailable_reads() {
    let app = harness::app().await;
    let subject = "auth0|mcp-framework-catalog";

    let scenario = ScenarioBuilder::new(&app)
        .with_user(subject)
        .with_workspace(subject, "MCP Framework Catalog")
        .build()
        .await;
    let soc2 = scenario.framework("soc2");

    let reader_token = authorize_agent_connection(
        &app,
        subject,
        "Framework Catalog Reader",
        &[WorkspacePermission::ReadControls],
    )
    .await;
    let limited_token = authorize_agent_connection(
        &app,
        subject,
        "Framework Catalog Limited",
        &[WorkspacePermission::ReadEvidence],
    )
    .await;

    let reader = McpClient::connect(app.mcp_server(), &reader_token).await;
    let frameworks = reader.call_tool("list_frameworks", json!({})).await;
    assert_eq!(
        frameworks,
        json!({ "frameworks": [framework_projection(soc2)] })
    );

    let requirements = reader
        .call_tool(
            "list_framework_requirements",
            json!({ "framework_id": soc2.id }),
        )
        .await;
    assert_eq!(
        requirements,
        json!({
            "requirements": [
                requirement_projection(soc2.requirement("CC6.1")),
                requirement_projection(soc2.requirement("CC7.1")),
            ]
        })
    );

    let unknown = reader
        .call_tool_error(
            "list_framework_requirements",
            json!({ "framework_id": Uuid::new_v4() }),
        )
        .await;
    assert_not_found(&unknown);

    let limited = McpClient::connect(app.mcp_server(), &limited_token).await;
    let denied_frameworks = limited.call_tool_error("list_frameworks", json!({})).await;
    assert_not_found(&denied_frameworks);
    let denied_requirements = limited
        .call_tool_error(
            "list_framework_requirements",
            json!({ "framework_id": soc2.id }),
        )
        .await;
    assert_not_found(&denied_requirements);
}

#[tokio::test]
async fn control_create_list_get_and_replace_round_trip_complete_requirement_links_and_audits() {
    let app = harness::app().await;
    let subject = "auth0|mcp-control-lifecycle";

    let scenario = ScenarioBuilder::new(&app)
        .with_user(subject)
        .with_workspace(subject, "MCP Control Lifecycle")
        .build()
        .await;
    let user_id = scenario.user(subject).id;
    let workspace_id = scenario.workspace("MCP Control Lifecycle").id;
    let soc2 = scenario.framework("soc2");
    let cc61 = soc2.requirement("CC6.1");
    let cc71 = soc2.requirement("CC7.1");

    let token = authorize_agent_connection(
        &app,
        subject,
        "Control Catalog Manager",
        &[
            WorkspacePermission::ReadControls,
            WorkspacePermission::WriteControls,
        ],
    )
    .await;

    let connection_id = get_agent_connection_id_for(&app, subject, "Control Catalog Manager").await;

    let (created, create_logs) = app
        .capture_audit_logs(async |request_id| {
            McpClient::connect_with_request_id(app.mcp_server(), &token, request_id)
                .await
                .call_tool(
                    "create_control",
                    json!({
                        "code": "PP-CC-01",
                        "title": "Identity and monitoring review",
                        "description": "Review logical access and monitoring safeguards.",
                        "framework_requirement_ids": [cc71.id, cc61.id],
                    }),
                )
                .await
        })
        .await;
    let control_id = Uuid::parse_str(created["id"].as_str().expect("control id is a string"))
        .expect("control id is a UUID");
    assert_control_projection(
        &created,
        control_id,
        workspace_id,
        "PP-CC-01",
        "Identity and monitoring review",
        "Review logical access and monitoring safeguards.",
        &[cc61, cc71],
    );
    assert_eq!(created["updated_at"], created["created_at"]);
    assert_eq!(create_logs.len(), 1);
    assert_control_audit_event(
        &create_logs[0],
        "control.created",
        "create_control",
        user_id,
        connection_id,
        workspace_id,
        control_id,
    );

    let client = McpClient::connect(app.mcp_server(), &token).await;
    let listed = client.call_tool("list_controls", json!({})).await;
    assert_eq!(listed, json!({ "controls": [created.clone()] }));
    let got = client
        .call_tool("get_control", json!({ "control_id": control_id }))
        .await;
    assert_eq!(got, created);

    let created_at = got["created_at"].clone();
    let (replaced, replace_logs) = app
        .capture_audit_logs(async |request_id| {
            McpClient::connect_with_request_id(app.mcp_server(), &token, request_id)
                .await
                .call_tool(
                    "replace_control",
                    json!({
                        "control_id": control_id,
                        "code": "PP-CC-02",
                        "title": "Monitoring review",
                        "description": "Review monitoring safeguards.",
                        "framework_requirement_ids": [cc71.id],
                    }),
                )
                .await
        })
        .await;
    assert_control_projection(
        &replaced,
        control_id,
        workspace_id,
        "PP-CC-02",
        "Monitoring review",
        "Review monitoring safeguards.",
        &[cc71],
    );
    assert_eq!(replaced["created_at"], created_at);
    assert_rfc3339(&replaced["updated_at"]);
    assert_eq!(replace_logs.len(), 1);
    assert_control_audit_event(
        &replace_logs[0],
        "control.updated",
        "replace_control",
        user_id,
        connection_id,
        workspace_id,
        control_id,
    );

    let final_listing = client.call_tool("list_controls", json!({})).await;
    assert_eq!(final_listing, json!({ "controls": [replaced] }));
}

#[tokio::test]
async fn control_rejections_are_exact_unaudited_and_leave_the_complete_listing_unchanged() {
    let app = harness::app().await;
    let subject = "auth0|mcp-control-rejections";

    let scenario = ScenarioBuilder::new(&app)
        .with_user(subject)
        .with_workspace(subject, "MCP Control Rejections")
        .build()
        .await;
    let workspace_id = scenario.workspace("MCP Control Rejections").id;
    let soc2 = scenario.framework("soc2");
    let cc61 = soc2.requirement("CC6.1");
    let cc71 = soc2.requirement("CC7.1");

    let manager_token = authorize_agent_connection(
        &app,
        subject,
        "Control Rejection Manager",
        &[
            WorkspacePermission::ReadControls,
            WorkspacePermission::WriteControls,
        ],
    )
    .await;
    let read_only_token = authorize_agent_connection(
        &app,
        subject,
        "Control Rejection Reader",
        &[WorkspacePermission::ReadControls],
    )
    .await;

    let manager = McpClient::connect(app.mcp_server(), &manager_token).await;
    let baseline = manager
        .call_tool(
            "create_control",
            json!({
                "code": "PP-BASELINE",
                "title": "Baseline control",
                "description": "The unchanged baseline control.",
                "framework_requirement_ids": [cc61.id, cc71.id],
            }),
        )
        .await;
    let control_id = Uuid::parse_str(baseline["id"].as_str().expect("control id is a string"))
        .expect("control id is a UUID");
    assert_control_projection(
        &baseline,
        control_id,
        workspace_id,
        "PP-BASELINE",
        "Baseline control",
        "The unchanged baseline control.",
        &[cc61, cc71],
    );
    let unknown_requirement_id = Uuid::new_v4();
    let unknown_get_id = Uuid::new_v4();
    let unknown_replace_id = Uuid::new_v4();

    let (
        (
            duplicate_code,
            duplicate_requirements,
            unknown_requirement,
            unknown_get,
            unknown_replace,
            denied_create,
            denied_replace,
            final_listing,
        ),
        logs,
    ) = app
        .capture_audit_logs(async |request_id| {
            let manager =
                McpClient::connect_with_request_id(app.mcp_server(), &manager_token, request_id)
                    .await;
            let reader =
                McpClient::connect_with_request_id(app.mcp_server(), &read_only_token, request_id)
                    .await;

            let duplicate_code = manager
                .call_tool_error(
                    "create_control",
                    json!({
                        "code": "PP-BASELINE",
                        "title": "Duplicate code",
                        "description": "This code is already used.",
                        "framework_requirement_ids": [cc61.id],
                    }),
                )
                .await;
            let duplicate_requirements = manager
                .call_tool_error(
                    "replace_control",
                    json!({
                        "control_id": control_id,
                        "code": "PP-DUPLICATE-REQUIREMENTS",
                        "title": "Duplicate requirements",
                        "description": "This request repeats one requirement.",
                        "framework_requirement_ids": [cc71.id, cc71.id],
                    }),
                )
                .await;
            let unknown_requirement = manager
                .call_tool_error(
                    "replace_control",
                    json!({
                        "control_id": control_id,
                        "code": "PP-UNKNOWN-REQUIREMENT",
                        "title": "Unknown requirement",
                        "description": "This request references an unknown requirement.",
                        "framework_requirement_ids": [unknown_requirement_id],
                    }),
                )
                .await;
            let unknown_get = manager
                .call_tool_error("get_control", json!({ "control_id": unknown_get_id }))
                .await;
            let unknown_replace = manager
                .call_tool_error(
                    "replace_control",
                    json!({
                        "control_id": unknown_replace_id,
                        "code": "PP-UNKNOWN-CONTROL",
                        "title": "Unknown control",
                        "description": "This control does not exist.",
                        "framework_requirement_ids": [cc61.id],
                    }),
                )
                .await;
            let denied_create = reader
                .call_tool_error(
                    "create_control",
                    json!({
                        "code": "PP-DENIED",
                        "title": "Denied control",
                        "description": "The reader cannot create controls.",
                        "framework_requirement_ids": [cc61.id],
                    }),
                )
                .await;
            let denied_replace = reader
                .call_tool_error(
                    "replace_control",
                    json!({
                        "control_id": control_id,
                        "code": "PP-DENIED",
                        "title": "Denied replacement",
                        "description": "The reader cannot replace controls.",
                        "framework_requirement_ids": [cc71.id],
                    }),
                )
                .await;
            let final_listing = manager.call_tool("list_controls", json!({})).await;

            (
                duplicate_code,
                duplicate_requirements,
                unknown_requirement,
                unknown_get,
                unknown_replace,
                denied_create,
                denied_replace,
                final_listing,
            )
        })
        .await;

    assert_control_code_taken(&duplicate_code);
    assert_validation_error(
        &duplicate_requirements,
        json!([{
            "field": "framework_requirement_ids",
            "message": format!("must not contain duplicate ids: {}", cc71.id),
        }]),
    );
    assert_validation_error(
        &unknown_requirement,
        json!([{
            "field": "framework_requirement_ids",
            "message": "framework_requirement_ids contains unknown ids",
        }]),
    );
    assert_not_found(&unknown_get);
    assert_not_found(&unknown_replace);
    assert_not_found(&denied_create);
    assert_not_found(&denied_replace);
    assert!(logs.is_empty());
    assert_eq!(final_listing, json!({ "controls": [baseline] }));
}

fn framework_projection(framework: &TestFramework) -> Value {
    json!({
        "id": framework.id,
        "code": framework.code,
        "name": framework.name,
        "description": framework.description,
    })
}

fn requirement_projection(requirement: &TestFrameworkRequirement) -> Value {
    json!({
        "id": requirement.id,
        "framework_id": requirement.framework_id,
        "code": requirement.code,
        "title": requirement.title,
        "description": requirement.description,
    })
}

#[allow(clippy::too_many_arguments)]
#[track_caller]
fn assert_control_projection(
    control: &Value,
    control_id: Uuid,
    workspace_id: Uuid,
    code: &str,
    title: &str,
    description: &str,
    requirements: &[&TestFrameworkRequirement],
) {
    assert_eq!(
        object_keys(control),
        [
            "code",
            "created_at",
            "description",
            "framework_requirements",
            "id",
            "title",
            "updated_at",
            "workspace_id",
        ]
        .into_iter()
        .collect()
    );
    assert_eq!(control["id"], control_id.to_string());
    assert_eq!(control["workspace_id"], workspace_id.to_string());
    assert_eq!(control["code"], code);
    assert_eq!(control["title"], title);
    assert_eq!(control["description"], description);
    assert_rfc3339(&control["created_at"]);
    assert_rfc3339(&control["updated_at"]);

    let actual_requirements = control["framework_requirements"]
        .as_array()
        .expect("framework_requirements is an array");
    assert_eq!(actual_requirements.len(), requirements.len());
    for (actual, expected) in actual_requirements.iter().zip(requirements) {
        assert_eq!(actual, &requirement_projection(expected));
    }
}

#[allow(clippy::too_many_arguments)]
#[track_caller]
fn assert_control_audit_event(
    record: &Value,
    event_name: &str,
    operation: &str,
    user_id: Uuid,
    connection_id: Uuid,
    workspace_id: Uuid,
    control_id: Uuid,
) {
    assert_eq!(
        object_keys(record),
        ["fields", "level", "target", "timestamp"]
            .into_iter()
            .collect()
    );
    assert_eq!(record["level"], "INFO");
    assert_eq!(record["target"], "proofplane::audit");
    assert_rfc3339(&record["timestamp"]);

    let fields = &record["fields"];
    assert_eq!(
        object_keys(fields),
        [
            "actor_type",
            "agent_connection_id",
            "client_type",
            "event_id",
            "event_name",
            "metadata",
            "object_id",
            "object_type",
            "operation",
            "outcome",
            "request_id",
            "type",
            "user_id",
            "workspace_id",
        ]
        .into_iter()
        .collect()
    );
    assert_eq!(fields["type"], "audit_log");
    Uuid::parse_str(fields["event_id"].as_str().expect("event id is a string"))
        .expect("event id is a UUID");
    assert_eq!(fields["event_name"], event_name);
    assert_eq!(fields["outcome"], "success");
    assert_eq!(fields["actor_type"], "agent_connection");
    assert_eq!(fields["user_id"], user_id.to_string());
    assert_eq!(fields["agent_connection_id"], connection_id.to_string());
    assert_eq!(fields["client_type"], "mcp");
    assert_eq!(fields["operation"], operation);
    assert_eq!(fields["workspace_id"], workspace_id.to_string());
    Uuid::parse_str(
        fields["request_id"]
            .as_str()
            .expect("request id is a string"),
    )
    .expect("request id is a UUID");
    assert_eq!(fields["object_type"], "control");
    assert_eq!(fields["object_id"], control_id.to_string());
    assert_eq!(
        serde_json::from_str::<Value>(
            fields["metadata"]
                .as_str()
                .expect("audit metadata is serialized JSON"),
        )
        .expect("audit metadata parses"),
        json!({ "control_id": control_id })
    );
}

#[track_caller]
fn assert_control_code_taken(error: &McpError) {
    assert_eq!(error.code, ErrorCode(-32000));
    assert_eq!(
        error.data,
        json!({
            "problem": {
                "code": "control_code_taken",
                "message": "a control with this code already exists in the workspace",
            }
        })
    );
}
