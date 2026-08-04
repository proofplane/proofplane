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
    scenario::{types::TestControl, ScenarioBuilder},
};

#[tokio::test]
async fn policy_catalog_lifecycle_is_complete_normalized_ordered_and_safely_audited() {
    let app = harness::app().await;
    let subject = "auth0|mcp-policy-catalog-lifecycle";
    let workspace_name = "MCP Policy Catalog Lifecycle";

    let scenario = ScenarioBuilder::new(&app)
        .with_user(subject)
        .with_workspace(subject, workspace_name)
        .with_control(workspace_name, "PP-B", "Second policy safeguard")
        .with_control(workspace_name, "PP-A", "First policy safeguard")
        .with_policy(workspace_name, "beta Fixture policy")
        .build()
        .await;
    let user_id = scenario.user(subject).id;
    let workspace = scenario.workspace(workspace_name);
    let workspace_id = workspace.id;
    let control_a = workspace.control("PP-A");
    let control_b = workspace.control("PP-B");
    let fixture_policy = workspace.policy("beta Fixture policy");

    let token = authorize_agent_connection(
        &app,
        subject,
        "Policy Catalog Manager",
        &[
            WorkspacePermission::ReadControls,
            WorkspacePermission::WriteControls,
        ],
    )
    .await;
    let connection_id = get_agent_connection_id_for(&app, subject, "Policy Catalog Manager").await;

    let ((created, create_request_id), create_logs) = app
        .capture_audit_logs(async |request_id| {
            let created = McpClient::connect_with_request_id(app.mcp_server(), &token, request_id)
                .await
                .call_tool(
                    "create_policy",
                    json!({
                        "name": "  Zulu Policy  ",
                        "description": "  Original policy description.  ",
                        "control_ids": [control_b.id, control_a.id],
                    }),
                )
                .await;
            (created, request_id)
        })
        .await;
    let policy_id = Uuid::parse_str(
        created["id"]
            .as_str()
            .expect("created policy id is a string"),
    )
    .expect("created policy id is a UUID");
    assert_policy_detail(
        &created,
        policy_id,
        "Zulu Policy",
        Some("Original policy description."),
        &[control_a, control_b],
    );
    assert_eq!(created["created_at"], created["updated_at"]);
    assert_single_policy_audit_event(
        &create_logs,
        "policy.created",
        "create_policy",
        user_id,
        connection_id,
        workspace_id,
        create_request_id,
        Some(policy_id),
    );

    let ((listed, list_request_id), list_logs) = app
        .capture_audit_logs(async |request_id| {
            let listed = McpClient::connect_with_request_id(app.mcp_server(), &token, request_id)
                .await
                .call_tool("list_policies", json!({}))
                .await;
            (listed, request_id)
        })
        .await;
    assert_eq!(object_keys(&listed), ["policies"].into_iter().collect());
    let policies = listed["policies"].as_array().expect("policies is an array");
    assert_eq!(policies.len(), 2);
    assert_policy_summary(
        &policies[0],
        fixture_policy.id,
        &fixture_policy.name,
        fixture_policy.description.as_deref(),
        0,
    );
    assert_policy_summary(
        &policies[1],
        policy_id,
        "Zulu Policy",
        Some("Original policy description."),
        2,
    );
    assert_single_policy_audit_event(
        &list_logs,
        "policy.listed",
        "list_policies",
        user_id,
        connection_id,
        workspace_id,
        list_request_id,
        None,
    );

    let ((got, get_request_id), get_logs) = app
        .capture_audit_logs(async |request_id| {
            let got = McpClient::connect_with_request_id(app.mcp_server(), &token, request_id)
                .await
                .call_tool("get_policy", json!({ "policy_id": policy_id }))
                .await;
            (got, request_id)
        })
        .await;
    assert_eq!(got, created);
    assert_single_policy_audit_event(
        &get_logs,
        "policy.read",
        "get_policy",
        user_id,
        connection_id,
        workspace_id,
        get_request_id,
        Some(policy_id),
    );

    let created_at = got["created_at"].clone();
    let ((updated, update_request_id), update_logs) = app
        .capture_audit_logs(async |request_id| {
            let updated = McpClient::connect_with_request_id(app.mcp_server(), &token, request_id)
                .await
                .call_tool(
                    "update_policy",
                    json!({
                        "policy_id": policy_id,
                        "name": "  Alpha POLICY  ",
                        "description": "  Revised policy description.  ",
                    }),
                )
                .await;
            (updated, request_id)
        })
        .await;
    assert_policy_detail(
        &updated,
        policy_id,
        "Alpha POLICY",
        Some("Revised policy description."),
        &[control_a, control_b],
    );
    assert_eq!(updated["created_at"], created_at);
    assert_single_policy_audit_event(
        &update_logs,
        "policy.updated",
        "update_policy",
        user_id,
        connection_id,
        workspace_id,
        update_request_id,
        Some(policy_id),
    );

    let client = McpClient::connect(app.mcp_server(), &token).await;
    let final_listing = client.call_tool("list_policies", json!({})).await;
    let final_policies = final_listing["policies"]
        .as_array()
        .expect("policies is an array");
    assert_eq!(final_policies.len(), 2);
    assert_policy_summary(
        &final_policies[0],
        policy_id,
        "Alpha POLICY",
        Some("Revised policy description."),
        2,
    );
    assert_policy_summary(
        &final_policies[1],
        fixture_policy.id,
        &fixture_policy.name,
        fixture_policy.description.as_deref(),
        0,
    );
}

#[tokio::test]
async fn policy_catalog_rejections_are_exact_unaudited_atomic_and_fully_concealed() {
    let app = harness::app().await;
    let owner = "auth0|mcp-policy-catalog-rejections-owner";
    let foreign = "auth0|mcp-policy-catalog-rejections-foreign";
    let owner_workspace_name = "MCP Policy Catalog Rejections Owner";
    let foreign_workspace_name = "MCP Policy Catalog Rejections Foreign";

    let scenario = ScenarioBuilder::new(&app)
        .with_user(owner)
        .with_workspace(owner, owner_workspace_name)
        .with_control(owner_workspace_name, "PP-OWNER", "Owner policy safeguard")
        .with_policy(owner_workspace_name, "Existing Policy")
        .with_user(foreign)
        .with_workspace(foreign, foreign_workspace_name)
        .with_control(
            foreign_workspace_name,
            "PP-FOREIGN",
            "Foreign policy safeguard",
        )
        .with_policy(foreign_workspace_name, "Foreign Policy")
        .build()
        .await;
    let owner_workspace = scenario.workspace(owner_workspace_name);
    let owner_control = owner_workspace.control("PP-OWNER");
    let existing_policy = owner_workspace.policy("Existing Policy");
    let foreign_workspace = scenario.workspace(foreign_workspace_name);
    let foreign_control = foreign_workspace.control("PP-FOREIGN");
    let foreign_policy = foreign_workspace.policy("Foreign Policy");

    let manager_token = authorize_agent_connection(
        &app,
        owner,
        "Policy Rejection Manager",
        &[
            WorkspacePermission::ReadControls,
            WorkspacePermission::WriteControls,
        ],
    )
    .await;
    let reader_token = authorize_agent_connection(
        &app,
        owner,
        "Policy Rejection Reader",
        &[WorkspacePermission::ReadControls],
    )
    .await;
    let writer_token = authorize_agent_connection(
        &app,
        owner,
        "Policy Rejection Writer",
        &[WorkspacePermission::WriteControls],
    )
    .await;

    let manager = McpClient::connect(app.mcp_server(), &manager_token).await;
    let baseline = manager.call_tool("list_policies", json!({})).await;
    assert_eq!(object_keys(&baseline), ["policies"].into_iter().collect());
    let baseline_policies = baseline["policies"]
        .as_array()
        .expect("policies is an array");
    assert_eq!(baseline_policies.len(), 1);
    assert_policy_summary(
        &baseline_policies[0],
        existing_policy.id,
        &existing_policy.name,
        existing_policy.description.as_deref(),
        0,
    );

    let unknown_control_id = Uuid::new_v4();
    let unknown_policy_id = Uuid::new_v4();
    let (rejections, rejection_logs) = app
        .capture_audit_logs(async |request_id| {
            let manager =
                McpClient::connect_with_request_id(app.mcp_server(), &manager_token, request_id)
                    .await;
            let reader =
                McpClient::connect_with_request_id(app.mcp_server(), &reader_token, request_id)
                    .await;
            let writer =
                McpClient::connect_with_request_id(app.mcp_server(), &writer_token, request_id)
                    .await;

            PolicyRejections {
                duplicate_name: manager
                    .call_tool_error("create_policy", json!({ "name": "existing policy" }))
                    .await,
                blank_and_duplicate_references: manager
                    .call_tool_error(
                        "create_policy",
                        json!({
                            "name": " \t ",
                            "description": "  ",
                            "control_ids": [owner_control.id, owner_control.id],
                        }),
                    )
                    .await,
                unknown_control: manager
                    .call_tool_error(
                        "create_policy",
                        json!({
                            "name": "Unknown control policy",
                            "control_ids": [owner_control.id, unknown_control_id],
                        }),
                    )
                    .await,
                foreign_control: manager
                    .call_tool_error(
                        "create_policy",
                        json!({
                            "name": "Foreign control policy",
                            "control_ids": [owner_control.id, foreign_control.id],
                        }),
                    )
                    .await,
                unknown_get: manager
                    .call_tool_error("get_policy", json!({ "policy_id": unknown_policy_id }))
                    .await,
                foreign_get: manager
                    .call_tool_error("get_policy", json!({ "policy_id": foreign_policy.id }))
                    .await,
                unknown_update: manager
                    .call_tool_error(
                        "update_policy",
                        json!({
                            "policy_id": unknown_policy_id,
                            "name": "Unknown replacement",
                        }),
                    )
                    .await,
                foreign_update: manager
                    .call_tool_error(
                        "update_policy",
                        json!({
                            "policy_id": foreign_policy.id,
                            "name": "Foreign replacement",
                        }),
                    )
                    .await,
                denied_list: writer.call_tool_error("list_policies", json!({})).await,
                denied_create: reader
                    .call_tool_error("create_policy", json!({ "name": "Denied creation" }))
                    .await,
                denied_update: reader
                    .call_tool_error(
                        "update_policy",
                        json!({
                            "policy_id": existing_policy.id,
                            "name": "Denied replacement",
                        }),
                    )
                    .await,
                denied_archive: reader
                    .call_tool_error("archive_policy", json!({ "policy_id": existing_policy.id }))
                    .await,
            }
        })
        .await;

    assert_policy_name_taken(&rejections.duplicate_name);
    assert_validation_error(
        &rejections.blank_and_duplicate_references,
        json!([
            {"field": "name", "message": "name must not be empty"},
            {
                "field": "description",
                "message": "description must not be blank when provided"
            },
            {
                "field": "control_ids",
                "message": "control_ids contains a duplicate value"
            },
        ]),
    );
    assert_validation_error(
        &rejections.unknown_control,
        json!([{
            "field": "control_ids",
            "message": "control_ids contains unknown ids",
        }]),
    );
    assert_validation_error(
        &rejections.foreign_control,
        json!([{
            "field": "control_ids",
            "message": "control_ids contains unknown ids",
        }]),
    );
    for concealed in [
        &rejections.unknown_get,
        &rejections.foreign_get,
        &rejections.unknown_update,
        &rejections.foreign_update,
        &rejections.denied_list,
        &rejections.denied_create,
        &rejections.denied_update,
        &rejections.denied_archive,
    ] {
        assert_not_found(concealed);
    }
    assert!(rejection_logs.is_empty());

    let final_listing = manager.call_tool("list_policies", json!({})).await;
    assert_eq!(final_listing, baseline);
}

#[tokio::test]
async fn archived_policy_is_concealed_and_its_name_is_reusable_by_one_active_replacement() {
    let app = harness::app().await;
    let subject = "auth0|mcp-policy-catalog-archive";
    let workspace_name = "MCP Policy Catalog Archive";

    let scenario = ScenarioBuilder::new(&app)
        .with_user(subject)
        .with_workspace(subject, workspace_name)
        .with_control(workspace_name, "PP-ARCHIVE", "Archival policy safeguard")
        .with_policy(workspace_name, "Reusable Policy")
        .build()
        .await;
    let user_id = scenario.user(subject).id;
    let workspace = scenario.workspace(workspace_name);
    let workspace_id = workspace.id;
    let control_id = workspace.control("PP-ARCHIVE").id;
    let archived_policy = workspace.policy("Reusable Policy");
    let archived_policy_id = archived_policy.id;

    let token = authorize_agent_connection(
        &app,
        subject,
        "Policy Archive Manager",
        &[
            WorkspacePermission::ReadControls,
            WorkspacePermission::WriteControls,
        ],
    )
    .await;
    let connection_id = get_agent_connection_id_for(&app, subject, "Policy Archive Manager").await;

    let ((archived, archive_request_id), archive_logs) = app
        .capture_audit_logs(async |request_id| {
            let archived = McpClient::connect_with_request_id(app.mcp_server(), &token, request_id)
                .await
                .call_tool("archive_policy", json!({ "policy_id": archived_policy_id }))
                .await;
            (archived, request_id)
        })
        .await;
    assert_eq!(
        object_keys(&archived),
        ["archived_at", "policy_id"].into_iter().collect()
    );
    assert_eq!(archived["policy_id"], archived_policy_id.to_string());
    assert_rfc3339(&archived["archived_at"]);
    assert_single_policy_audit_event(
        &archive_logs,
        "policy.archived",
        "archive_policy",
        user_id,
        connection_id,
        workspace_id,
        archive_request_id,
        Some(archived_policy_id),
    );

    let (concealed, concealment_logs) = app
        .capture_audit_logs(async |request_id| {
            let client =
                McpClient::connect_with_request_id(app.mcp_server(), &token, request_id).await;
            [
                client
                    .call_tool_error("get_policy", json!({ "policy_id": archived_policy_id }))
                    .await,
                client
                    .call_tool_error(
                        "update_policy",
                        json!({
                            "policy_id": archived_policy_id,
                            "name": "Cannot update archived policy",
                        }),
                    )
                    .await,
                client
                    .call_tool_error(
                        "attach_policy_to_control",
                        json!({
                            "policy_id": archived_policy_id,
                            "control_id": control_id,
                        }),
                    )
                    .await,
                client
                    .call_tool_error("archive_policy", json!({ "policy_id": archived_policy_id }))
                    .await,
            ]
        })
        .await;
    for error in &concealed {
        assert_not_found(error);
    }
    assert!(concealment_logs.is_empty());

    let client = McpClient::connect(app.mcp_server(), &token).await;
    let replacement = client
        .call_tool("create_policy", json!({ "name": "reusable policy" }))
        .await;
    let replacement_id = Uuid::parse_str(
        replacement["id"]
            .as_str()
            .expect("replacement policy id is a string"),
    )
    .expect("replacement policy id is a UUID");
    assert_ne!(replacement_id, archived_policy_id);
    assert_policy_detail(&replacement, replacement_id, "reusable policy", None, &[]);

    let final_listing = client.call_tool("list_policies", json!({})).await;
    assert_eq!(
        object_keys(&final_listing),
        ["policies"].into_iter().collect()
    );
    let final_policies = final_listing["policies"]
        .as_array()
        .expect("policies is an array");
    assert_eq!(final_policies.len(), 1);
    assert_policy_summary(
        &final_policies[0],
        replacement_id,
        "reusable policy",
        None,
        0,
    );
}

struct PolicyRejections {
    duplicate_name: McpError,
    blank_and_duplicate_references: McpError,
    unknown_control: McpError,
    foreign_control: McpError,
    unknown_get: McpError,
    foreign_get: McpError,
    unknown_update: McpError,
    foreign_update: McpError,
    denied_list: McpError,
    denied_create: McpError,
    denied_update: McpError,
    denied_archive: McpError,
}

#[track_caller]
fn assert_policy_detail(
    policy: &Value,
    policy_id: Uuid,
    name: &str,
    description: Option<&str>,
    controls: &[&TestControl],
) {
    assert_eq!(
        object_keys(policy),
        [
            "controls",
            "created_at",
            "description",
            "document",
            "id",
            "name",
            "updated_at",
        ]
        .into_iter()
        .collect()
    );
    assert_eq!(policy["id"], policy_id.to_string());
    assert_eq!(policy["name"], name);
    assert_eq!(policy["description"], json!(description));
    assert_eq!(policy["document"], Value::Null);
    assert_rfc3339(&policy["created_at"]);
    assert_rfc3339(&policy["updated_at"]);

    let actual_controls = policy["controls"].as_array().expect("controls is an array");
    assert_eq!(actual_controls.len(), controls.len());
    for (actual, expected) in actual_controls.iter().zip(controls) {
        assert_eq!(
            object_keys(actual),
            ["code", "description", "id", "title"].into_iter().collect()
        );
        assert_eq!(
            actual,
            &json!({
                "id": expected.id,
                "code": expected.code,
                "title": expected.title,
                "description": expected.description,
            })
        );
    }
}

#[track_caller]
fn assert_policy_summary(
    policy: &Value,
    policy_id: Uuid,
    name: &str,
    description: Option<&str>,
    mapped_control_count: i64,
) {
    assert_eq!(
        object_keys(policy),
        [
            "description",
            "document",
            "id",
            "mapped_control_count",
            "name",
        ]
        .into_iter()
        .collect()
    );
    assert_eq!(policy["id"], policy_id.to_string());
    assert_eq!(policy["name"], name);
    assert_eq!(policy["description"], json!(description));
    assert_eq!(policy["mapped_control_count"], mapped_control_count);
    assert_eq!(policy["document"], Value::Null);
}

#[allow(clippy::too_many_arguments)]
#[track_caller]
fn assert_single_policy_audit_event(
    records: &[Value],
    event_name: &str,
    operation: &str,
    user_id: Uuid,
    connection_id: Uuid,
    workspace_id: Uuid,
    request_id: Uuid,
    policy_id: Option<Uuid>,
) {
    assert_eq!(records.len(), 1);
    let record = &records[0];
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
    let expected_keys = if policy_id.is_some() {
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
    } else {
        [
            "actor_type",
            "agent_connection_id",
            "client_type",
            "event_id",
            "event_name",
            "metadata",
            "operation",
            "outcome",
            "request_id",
            "type",
            "user_id",
            "workspace_id",
        ]
        .into_iter()
        .collect()
    };
    assert_eq!(object_keys(fields), expected_keys);
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
    assert_eq!(fields["request_id"], request_id.to_string());

    let metadata = serde_json::from_str::<Value>(
        fields["metadata"]
            .as_str()
            .expect("audit metadata is serialized JSON"),
    )
    .expect("audit metadata parses");
    if let Some(policy_id) = policy_id {
        assert_eq!(fields["object_type"], "policy");
        assert_eq!(fields["object_id"], policy_id.to_string());
        assert_eq!(metadata, json!({ "policy_id": policy_id }));
    } else {
        assert_eq!(metadata, json!({}));
    }
}

#[track_caller]
fn assert_policy_name_taken(error: &McpError) {
    assert_eq!(error.code, ErrorCode(-32000));
    assert_eq!(
        error.data,
        json!({
            "problem": {
                "code": "policy_name_taken",
                "message": "an active policy with this name already exists in the workspace",
            }
        })
    );
}
