use proofplane::domain::WorkspacePermission;
use rmcp::model::ErrorCode;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::support::{
    agent_connections::get_agent_connection_id_for,
    harness,
    json::{assert_rfc3339, object_keys},
    mcp::{assert_not_found, McpClient, McpError},
    oauth::authorize_agent_connection,
    scenario::{
        types::{TestControl, TestPolicy},
        ScenarioBuilder,
    },
};

#[tokio::test]
async fn singular_policy_control_mapping_covers_attach_duplicate_detach_and_missing_lifecycle() {
    let app = harness::app().await;
    let subject = "auth0|mcp-singular-policy-control-mapping";
    let workspace_name = "MCP Singular Policy Control Mapping";

    let scenario = ScenarioBuilder::new(&app)
        .with_user(subject)
        .with_workspace(subject, workspace_name)
        .with_control(workspace_name, "PP-SINGULAR", "Singular safeguard")
        .with_policy(workspace_name, "Singular Mapping Policy")
        .build()
        .await;
    let user_id = scenario.user(subject).id;
    let workspace = scenario.workspace(workspace_name);
    let workspace_id = workspace.id;
    let policy = workspace.policy("Singular Mapping Policy");
    let control = workspace.control("PP-SINGULAR");

    let token = authorize_agent_connection(
        &app,
        subject,
        "Singular Policy Control Manager",
        &[
            WorkspacePermission::ReadControls,
            WorkspacePermission::WriteControls,
        ],
    )
    .await;
    let connection_id =
        get_agent_connection_id_for(&app, subject, "Singular Policy Control Manager").await;
    let client = McpClient::connect(app.mcp_server(), &token).await;

    let empty = client
        .call_tool("get_policy", json!({ "policy_id": policy.id }))
        .await;
    assert_policy_read_model(&empty, policy, &[]);

    let ((attached, attach_request_id), attach_logs) = app
        .capture_audit_logs(async |request_id| {
            let attached = McpClient::connect_with_request_id(app.mcp_server(), &token, request_id)
                .await
                .call_tool(
                    "attach_policy_to_control",
                    json!({ "policy_id": policy.id, "control_id": control.id }),
                )
                .await;
            (attached, request_id)
        })
        .await;
    assert_eq!(
        attached,
        json!({ "policy_id": policy.id, "control_id": control.id })
    );
    assert_eq!(attach_logs.len(), 1);
    assert_mapping_audit_event(
        &attach_logs[0],
        "policy_control_mapping.created",
        "attach_policy_to_control",
        user_id,
        connection_id,
        workspace_id,
        attach_request_id,
        "policy_control_mapping",
        control.id,
        json!({ "policy_id": policy.id, "control_id": control.id }),
    );

    let attached_read_model = client
        .call_tool("get_policy", json!({ "policy_id": policy.id }))
        .await;
    assert_policy_read_model(&attached_read_model, policy, &[control]);

    let (duplicate, duplicate_logs) = app
        .capture_audit_logs(async |request_id| {
            McpClient::connect_with_request_id(app.mcp_server(), &token, request_id)
                .await
                .call_tool_error(
                    "attach_policy_to_control",
                    json!({ "policy_id": policy.id, "control_id": control.id }),
                )
                .await
        })
        .await;
    assert_eq!(duplicate.code, ErrorCode(-32000));
    assert_eq!(
        duplicate.data,
        json!({
            "problem": {
                "code": "policy_control_mapping_exists",
                "message": "this control is already mapped to the policy",
            }
        })
    );
    assert!(duplicate_logs.is_empty());
    assert_eq!(
        client
            .call_tool("get_policy", json!({ "policy_id": policy.id }))
            .await,
        attached_read_model
    );

    let ((detached, detach_request_id), detach_logs) = app
        .capture_audit_logs(async |request_id| {
            let detached = McpClient::connect_with_request_id(app.mcp_server(), &token, request_id)
                .await
                .call_tool(
                    "detach_policy_from_control",
                    json!({ "policy_id": policy.id, "control_id": control.id }),
                )
                .await;
            (detached, request_id)
        })
        .await;
    assert_eq!(
        detached,
        json!({ "policy_id": policy.id, "control_id": control.id })
    );
    assert_eq!(detach_logs.len(), 1);
    assert_mapping_audit_event(
        &detach_logs[0],
        "policy_control_mapping.deleted",
        "detach_policy_from_control",
        user_id,
        connection_id,
        workspace_id,
        detach_request_id,
        "policy_control_mapping",
        control.id,
        json!({ "policy_id": policy.id, "control_id": control.id }),
    );

    let detached_read_model = client
        .call_tool("get_policy", json!({ "policy_id": policy.id }))
        .await;
    assert_policy_read_model(&detached_read_model, policy, &[]);

    let (missing, missing_logs) = app
        .capture_audit_logs(async |request_id| {
            McpClient::connect_with_request_id(app.mcp_server(), &token, request_id)
                .await
                .call_tool_error(
                    "detach_policy_from_control",
                    json!({ "policy_id": policy.id, "control_id": control.id }),
                )
                .await
        })
        .await;
    assert_not_found(&missing);
    assert!(missing_logs.is_empty());
    assert_eq!(
        client
            .call_tool("get_policy", json!({ "policy_id": policy.id }))
            .await,
        detached_read_model
    );
}

#[tokio::test]
async fn attach_policy_to_controls_attaches_two_in_request_order_and_audits_once() {
    let app = harness::app().await;
    let subject = "auth0|mcp-attach-policy-controls";
    let workspace_name = "MCP Attach Policy Controls";

    let scenario = ScenarioBuilder::new(&app)
        .with_user(subject)
        .with_workspace(subject, workspace_name)
        .with_control(workspace_name, "PP-ATTACH-A", "Alpha safeguard")
        .with_control(workspace_name, "PP-ATTACH-B", "Bravo safeguard")
        .with_policy(workspace_name, "Governing Policy")
        .build()
        .await;
    let user_id = scenario.user(subject).id;
    let workspace = scenario.workspace(workspace_name);
    let workspace_id = workspace.id;
    let policy = workspace.policy("Governing Policy");
    let first_control = workspace.control("PP-ATTACH-A");
    let second_control = workspace.control("PP-ATTACH-B");

    let token = authorize_agent_connection(
        &app,
        subject,
        "Attach Policy Controls Manager",
        &[
            WorkspacePermission::ReadControls,
            WorkspacePermission::WriteControls,
        ],
    )
    .await;
    let connection_id =
        get_agent_connection_id_for(&app, subject, "Attach Policy Controls Manager").await;
    let client = McpClient::connect(app.mcp_server(), &token).await;

    let before = client
        .call_tool("get_policy", json!({ "policy_id": policy.id }))
        .await;
    assert_policy_read_model(&before, policy, &[]);

    let ((attached, request_id), logs) = app
        .capture_audit_logs(async |request_id| {
            let attached = McpClient::connect_with_request_id(app.mcp_server(), &token, request_id)
                .await
                .call_tool(
                    "attach_policy_to_controls",
                    json!({
                        "policy_id": policy.id,
                        "control_ids": [second_control.id, first_control.id],
                    }),
                )
                .await;
            (attached, request_id)
        })
        .await;
    assert_eq!(
        attached,
        json!({
            "policy_id": policy.id,
            "count": 2,
            "control_ids": [second_control.id, first_control.id],
        })
    );
    assert_eq!(logs.len(), 1);
    assert_mapping_audit_event(
        &logs[0],
        "policy_control_mappings.created",
        "attach_policy_to_controls",
        user_id,
        connection_id,
        workspace_id,
        request_id,
        "policy",
        policy.id,
        batch_metadata(
            "policy_id",
            policy.id,
            "control_ids",
            &[second_control.id, first_control.id],
        ),
    );

    let after = client
        .call_tool("get_policy", json!({ "policy_id": policy.id }))
        .await;
    assert_policy_read_model(&after, policy, &[first_control, second_control]);
}

#[tokio::test]
async fn attach_policy_to_controls_rejects_valid_plus_unknown_atomically() {
    let app = harness::app().await;
    let subject = "auth0|mcp-attach-policy-controls-reject";
    let workspace_name = "MCP Attach Policy Controls Reject";

    let scenario = ScenarioBuilder::new(&app)
        .with_user(subject)
        .with_workspace(subject, workspace_name)
        .with_control(workspace_name, "PP-ATTACH-VALID", "Valid safeguard")
        .with_policy(workspace_name, "Atomic Attachment Policy")
        .build()
        .await;
    let workspace = scenario.workspace(workspace_name);
    let policy = workspace.policy("Atomic Attachment Policy");
    let valid_control = workspace.control("PP-ATTACH-VALID");

    let token = authorize_agent_connection(
        &app,
        subject,
        "Attach Policy Controls Rejection Manager",
        &[
            WorkspacePermission::ReadControls,
            WorkspacePermission::WriteControls,
        ],
    )
    .await;
    let client = McpClient::connect(app.mcp_server(), &token).await;
    let before = client
        .call_tool("get_policy", json!({ "policy_id": policy.id }))
        .await;
    assert_policy_read_model(&before, policy, &[]);
    let unknown_control_id = Uuid::new_v4();

    let (rejected, logs) = app
        .capture_audit_logs(async |request_id| {
            McpClient::connect_with_request_id(app.mcp_server(), &token, request_id)
                .await
                .call_tool_error(
                    "attach_policy_to_controls",
                    json!({
                        "policy_id": policy.id,
                        "control_ids": [valid_control.id, unknown_control_id],
                    }),
                )
                .await
        })
        .await;
    assert_batch_rejected(
        &rejected,
        "control_ids",
        "control_ids contains unknown or already-attached ids",
        &[
            ("unknown_ids", &[unknown_control_id]),
            ("already_mapped_ids", &[]),
        ],
    );
    assert!(logs.is_empty());
    let after = client
        .call_tool("get_policy", json!({ "policy_id": policy.id }))
        .await;
    assert_eq!(after, before);
}

#[tokio::test]
async fn attach_control_to_policies_attaches_two_in_request_order_and_audits_once() {
    let app = harness::app().await;
    let subject = "auth0|mcp-attach-control-policies";
    let workspace_name = "MCP Attach Control Policies";

    let scenario = ScenarioBuilder::new(&app)
        .with_user(subject)
        .with_workspace(subject, workspace_name)
        .with_control(workspace_name, "PP-ATTACH-ONE", "Shared safeguard")
        .with_policy(workspace_name, "Alpha Policy")
        .with_policy(workspace_name, "Bravo Policy")
        .build()
        .await;
    let user_id = scenario.user(subject).id;
    let workspace = scenario.workspace(workspace_name);
    let workspace_id = workspace.id;
    let control = workspace.control("PP-ATTACH-ONE");
    let first_policy = workspace.policy("Alpha Policy");
    let second_policy = workspace.policy("Bravo Policy");

    let token = authorize_agent_connection(
        &app,
        subject,
        "Attach Control Policies Manager",
        &[
            WorkspacePermission::ReadControls,
            WorkspacePermission::WriteControls,
        ],
    )
    .await;
    let connection_id =
        get_agent_connection_id_for(&app, subject, "Attach Control Policies Manager").await;
    let client = McpClient::connect(app.mcp_server(), &token).await;

    let first_before = client
        .call_tool("get_policy", json!({ "policy_id": first_policy.id }))
        .await;
    let second_before = client
        .call_tool("get_policy", json!({ "policy_id": second_policy.id }))
        .await;
    assert_policy_read_model(&first_before, first_policy, &[]);
    assert_policy_read_model(&second_before, second_policy, &[]);

    let ((attached, request_id), logs) = app
        .capture_audit_logs(async |request_id| {
            let attached = McpClient::connect_with_request_id(app.mcp_server(), &token, request_id)
                .await
                .call_tool(
                    "attach_control_to_policies",
                    json!({
                        "control_id": control.id,
                        "policy_ids": [second_policy.id, first_policy.id],
                    }),
                )
                .await;
            (attached, request_id)
        })
        .await;
    assert_eq!(
        attached,
        json!({
            "control_id": control.id,
            "count": 2,
            "policy_ids": [second_policy.id, first_policy.id],
        })
    );
    assert_eq!(logs.len(), 1);
    assert_mapping_audit_event(
        &logs[0],
        "policy_control_mappings.created",
        "attach_control_to_policies",
        user_id,
        connection_id,
        workspace_id,
        request_id,
        "control",
        control.id,
        batch_metadata(
            "control_id",
            control.id,
            "policy_ids",
            &[second_policy.id, first_policy.id],
        ),
    );

    let first_after = client
        .call_tool("get_policy", json!({ "policy_id": first_policy.id }))
        .await;
    let second_after = client
        .call_tool("get_policy", json!({ "policy_id": second_policy.id }))
        .await;
    assert_policy_read_model(&first_after, first_policy, &[control]);
    assert_policy_read_model(&second_after, second_policy, &[control]);
}

#[tokio::test]
async fn attach_control_to_policies_rejects_valid_plus_already_attached_atomically() {
    let app = harness::app().await;
    let subject = "auth0|mcp-attach-control-policies-reject";
    let workspace_name = "MCP Attach Control Policies Reject";

    let scenario = ScenarioBuilder::new(&app)
        .with_user(subject)
        .with_workspace(subject, workspace_name)
        .with_control(workspace_name, "PP-ATTACH-SHARED", "Shared safeguard")
        .with_policy(workspace_name, "Already Attached Policy")
        .with_policy(workspace_name, "Valid New Policy")
        .with_policy_control_mapping(
            workspace_name,
            "Already Attached Policy",
            "PP-ATTACH-SHARED",
        )
        .build()
        .await;
    let workspace = scenario.workspace(workspace_name);
    let control = workspace.control("PP-ATTACH-SHARED");
    let attached_policy = workspace.policy("Already Attached Policy");
    let valid_policy = workspace.policy("Valid New Policy");

    let token = authorize_agent_connection(
        &app,
        subject,
        "Attach Control Policies Rejection Manager",
        &[
            WorkspacePermission::ReadControls,
            WorkspacePermission::WriteControls,
        ],
    )
    .await;
    let client = McpClient::connect(app.mcp_server(), &token).await;
    let attached_before = client
        .call_tool("get_policy", json!({ "policy_id": attached_policy.id }))
        .await;
    let valid_before = client
        .call_tool("get_policy", json!({ "policy_id": valid_policy.id }))
        .await;
    assert_policy_read_model(&attached_before, attached_policy, &[control]);
    assert_policy_read_model(&valid_before, valid_policy, &[]);

    let (rejected, logs) = app
        .capture_audit_logs(async |request_id| {
            McpClient::connect_with_request_id(app.mcp_server(), &token, request_id)
                .await
                .call_tool_error(
                    "attach_control_to_policies",
                    json!({
                        "control_id": control.id,
                        "policy_ids": [valid_policy.id, attached_policy.id],
                    }),
                )
                .await
        })
        .await;
    assert_batch_rejected(
        &rejected,
        "policy_ids",
        "policy_ids contains unknown, archived, or already-attached ids",
        &[
            ("unknown_ids", &[]),
            ("archived_ids", &[]),
            ("already_mapped_ids", &[attached_policy.id]),
        ],
    );
    assert!(logs.is_empty());
    let attached_after = client
        .call_tool("get_policy", json!({ "policy_id": attached_policy.id }))
        .await;
    let valid_after = client
        .call_tool("get_policy", json!({ "policy_id": valid_policy.id }))
        .await;
    assert_eq!(attached_after, attached_before);
    assert_eq!(valid_after, valid_before);
}

#[tokio::test]
async fn detach_policy_from_controls_removes_two_in_request_order_and_audits_once() {
    let app = harness::app().await;
    let subject = "auth0|mcp-detach-policy-controls";
    let workspace_name = "MCP Detach Policy Controls";

    let scenario = ScenarioBuilder::new(&app)
        .with_user(subject)
        .with_workspace(subject, workspace_name)
        .with_control(workspace_name, "PP-DETACH-A", "Alpha safeguard")
        .with_control(workspace_name, "PP-DETACH-B", "Bravo safeguard")
        .with_policy(workspace_name, "Detaching Policy")
        .with_policy_control_mapping(workspace_name, "Detaching Policy", "PP-DETACH-B")
        .with_policy_control_mapping(workspace_name, "Detaching Policy", "PP-DETACH-A")
        .build()
        .await;
    let user_id = scenario.user(subject).id;
    let workspace = scenario.workspace(workspace_name);
    let workspace_id = workspace.id;
    let policy = workspace.policy("Detaching Policy");
    let first_control = workspace.control("PP-DETACH-A");
    let second_control = workspace.control("PP-DETACH-B");

    let token = authorize_agent_connection(
        &app,
        subject,
        "Detach Policy Controls Manager",
        &[
            WorkspacePermission::ReadControls,
            WorkspacePermission::WriteControls,
        ],
    )
    .await;
    let connection_id =
        get_agent_connection_id_for(&app, subject, "Detach Policy Controls Manager").await;
    let client = McpClient::connect(app.mcp_server(), &token).await;

    let before = client
        .call_tool("get_policy", json!({ "policy_id": policy.id }))
        .await;
    assert_policy_read_model(&before, policy, &[first_control, second_control]);

    let ((detached, request_id), logs) = app
        .capture_audit_logs(async |request_id| {
            let detached = McpClient::connect_with_request_id(app.mcp_server(), &token, request_id)
                .await
                .call_tool(
                    "detach_policy_from_controls",
                    json!({
                        "policy_id": policy.id,
                        "control_ids": [second_control.id, first_control.id],
                    }),
                )
                .await;
            (detached, request_id)
        })
        .await;
    assert_eq!(
        detached,
        json!({
            "policy_id": policy.id,
            "count": 2,
            "control_ids": [second_control.id, first_control.id],
        })
    );
    assert_eq!(logs.len(), 1);
    assert_mapping_audit_event(
        &logs[0],
        "policy_control_mappings.deleted",
        "detach_policy_from_controls",
        user_id,
        connection_id,
        workspace_id,
        request_id,
        "policy",
        policy.id,
        batch_metadata(
            "policy_id",
            policy.id,
            "control_ids",
            &[second_control.id, first_control.id],
        ),
    );

    let after = client
        .call_tool("get_policy", json!({ "policy_id": policy.id }))
        .await;
    assert_policy_read_model(&after, policy, &[]);
}

#[tokio::test]
async fn detach_policy_from_controls_rejects_mapped_plus_not_mapped_atomically() {
    let app = harness::app().await;
    let subject = "auth0|mcp-detach-policy-controls-reject";
    let workspace_name = "MCP Detach Policy Controls Reject";

    let scenario = ScenarioBuilder::new(&app)
        .with_user(subject)
        .with_workspace(subject, workspace_name)
        .with_control(workspace_name, "PP-DETACH-MAPPED", "Mapped safeguard")
        .with_control(workspace_name, "PP-DETACH-UNMAPPED", "Unmapped safeguard")
        .with_policy(workspace_name, "Atomic Detachment Policy")
        .with_policy_control_mapping(
            workspace_name,
            "Atomic Detachment Policy",
            "PP-DETACH-MAPPED",
        )
        .build()
        .await;
    let workspace = scenario.workspace(workspace_name);
    let policy = workspace.policy("Atomic Detachment Policy");
    let mapped_control = workspace.control("PP-DETACH-MAPPED");
    let unmapped_control = workspace.control("PP-DETACH-UNMAPPED");

    let token = authorize_agent_connection(
        &app,
        subject,
        "Detach Policy Controls Rejection Manager",
        &[
            WorkspacePermission::ReadControls,
            WorkspacePermission::WriteControls,
        ],
    )
    .await;
    let client = McpClient::connect(app.mcp_server(), &token).await;
    let before = client
        .call_tool("get_policy", json!({ "policy_id": policy.id }))
        .await;
    assert_policy_read_model(&before, policy, &[mapped_control]);

    let (rejected, logs) = app
        .capture_audit_logs(async |request_id| {
            McpClient::connect_with_request_id(app.mcp_server(), &token, request_id)
                .await
                .call_tool_error(
                    "detach_policy_from_controls",
                    json!({
                        "policy_id": policy.id,
                        "control_ids": [mapped_control.id, unmapped_control.id],
                    }),
                )
                .await
        })
        .await;
    assert_batch_rejected(
        &rejected,
        "control_ids",
        "control_ids contains unknown or not-mapped ids",
        &[
            ("unknown_ids", &[]),
            ("not_mapped_ids", &[unmapped_control.id]),
        ],
    );
    assert!(logs.is_empty());
    let after = client
        .call_tool("get_policy", json!({ "policy_id": policy.id }))
        .await;
    assert_eq!(after, before);
}

#[tokio::test]
async fn detach_control_from_policies_removes_two_in_request_order_and_audits_once() {
    let app = harness::app().await;
    let subject = "auth0|mcp-detach-control-policies";
    let workspace_name = "MCP Detach Control Policies";

    let scenario = ScenarioBuilder::new(&app)
        .with_user(subject)
        .with_workspace(subject, workspace_name)
        .with_control(workspace_name, "PP-DETACH-ONE", "Shared safeguard")
        .with_policy(workspace_name, "Alpha Detaching Policy")
        .with_policy(workspace_name, "Bravo Detaching Policy")
        .with_policy_control_mapping(workspace_name, "Alpha Detaching Policy", "PP-DETACH-ONE")
        .with_policy_control_mapping(workspace_name, "Bravo Detaching Policy", "PP-DETACH-ONE")
        .build()
        .await;
    let user_id = scenario.user(subject).id;
    let workspace = scenario.workspace(workspace_name);
    let workspace_id = workspace.id;
    let control = workspace.control("PP-DETACH-ONE");
    let first_policy = workspace.policy("Alpha Detaching Policy");
    let second_policy = workspace.policy("Bravo Detaching Policy");

    let token = authorize_agent_connection(
        &app,
        subject,
        "Detach Control Policies Manager",
        &[
            WorkspacePermission::ReadControls,
            WorkspacePermission::WriteControls,
        ],
    )
    .await;
    let connection_id =
        get_agent_connection_id_for(&app, subject, "Detach Control Policies Manager").await;
    let client = McpClient::connect(app.mcp_server(), &token).await;

    let first_before = client
        .call_tool("get_policy", json!({ "policy_id": first_policy.id }))
        .await;
    let second_before = client
        .call_tool("get_policy", json!({ "policy_id": second_policy.id }))
        .await;
    assert_policy_read_model(&first_before, first_policy, &[control]);
    assert_policy_read_model(&second_before, second_policy, &[control]);

    let ((detached, request_id), logs) = app
        .capture_audit_logs(async |request_id| {
            let detached = McpClient::connect_with_request_id(app.mcp_server(), &token, request_id)
                .await
                .call_tool(
                    "detach_control_from_policies",
                    json!({
                        "control_id": control.id,
                        "policy_ids": [second_policy.id, first_policy.id],
                    }),
                )
                .await;
            (detached, request_id)
        })
        .await;
    assert_eq!(
        detached,
        json!({
            "control_id": control.id,
            "count": 2,
            "policy_ids": [second_policy.id, first_policy.id],
        })
    );
    assert_eq!(logs.len(), 1);
    assert_mapping_audit_event(
        &logs[0],
        "policy_control_mappings.deleted",
        "detach_control_from_policies",
        user_id,
        connection_id,
        workspace_id,
        request_id,
        "control",
        control.id,
        batch_metadata(
            "control_id",
            control.id,
            "policy_ids",
            &[second_policy.id, first_policy.id],
        ),
    );

    let first_after = client
        .call_tool("get_policy", json!({ "policy_id": first_policy.id }))
        .await;
    let second_after = client
        .call_tool("get_policy", json!({ "policy_id": second_policy.id }))
        .await;
    assert_policy_read_model(&first_after, first_policy, &[]);
    assert_policy_read_model(&second_after, second_policy, &[]);
}

#[tokio::test]
async fn detach_control_from_policies_rejects_mapped_plus_unknown_atomically() {
    let app = harness::app().await;
    let subject = "auth0|mcp-detach-control-policies-reject";
    let workspace_name = "MCP Detach Control Policies Reject";

    let scenario = ScenarioBuilder::new(&app)
        .with_user(subject)
        .with_workspace(subject, workspace_name)
        .with_control(workspace_name, "PP-DETACH-SHARED", "Shared safeguard")
        .with_policy(workspace_name, "Mapped Detachment Policy")
        .with_policy_control_mapping(
            workspace_name,
            "Mapped Detachment Policy",
            "PP-DETACH-SHARED",
        )
        .build()
        .await;
    let workspace = scenario.workspace(workspace_name);
    let control = workspace.control("PP-DETACH-SHARED");
    let policy = workspace.policy("Mapped Detachment Policy");

    let token = authorize_agent_connection(
        &app,
        subject,
        "Detach Control Policies Rejection Manager",
        &[
            WorkspacePermission::ReadControls,
            WorkspacePermission::WriteControls,
        ],
    )
    .await;
    let client = McpClient::connect(app.mcp_server(), &token).await;
    let before = client
        .call_tool("get_policy", json!({ "policy_id": policy.id }))
        .await;
    assert_policy_read_model(&before, policy, &[control]);
    let unknown_policy_id = Uuid::new_v4();

    let (rejected, logs) = app
        .capture_audit_logs(async |request_id| {
            McpClient::connect_with_request_id(app.mcp_server(), &token, request_id)
                .await
                .call_tool_error(
                    "detach_control_from_policies",
                    json!({
                        "control_id": control.id,
                        "policy_ids": [policy.id, unknown_policy_id],
                    }),
                )
                .await
        })
        .await;
    assert_batch_rejected(
        &rejected,
        "policy_ids",
        "policy_ids contains unknown, archived, or not-mapped ids",
        &[
            ("unknown_ids", &[unknown_policy_id]),
            ("archived_ids", &[]),
            ("not_mapped_ids", &[]),
        ],
    );
    assert!(logs.is_empty());
    let after = client
        .call_tool("get_policy", json!({ "policy_id": policy.id }))
        .await;
    assert_eq!(after, before);
}

#[track_caller]
fn assert_policy_read_model(policy: &Value, expected: &TestPolicy, controls: &[&TestControl]) {
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
    assert_eq!(policy["id"], expected.id.to_string());
    assert_eq!(policy["name"], expected.name);
    assert_eq!(policy["description"], json!(expected.description));
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
fn assert_batch_rejected(
    error: &McpError,
    field: &str,
    message: &str,
    buckets: &[(&str, &[Uuid])],
) {
    assert_eq!(error.code, ErrorCode::INVALID_PARAMS);
    let mut problem = serde_json::Map::new();
    problem.insert("code".to_owned(), json!("batch_rejected"));
    problem.insert("message".to_owned(), json!(message));
    problem.insert("field".to_owned(), json!(field));
    for (name, ids) in buckets {
        problem.insert((*name).to_owned(), json!(ids));
    }
    assert_eq!(error.data, json!({ "problem": problem }));
}

fn batch_metadata(
    object_id_key: &'static str,
    object_id: Uuid,
    ids_key: &'static str,
    ids: &[Uuid],
) -> Value {
    let mut metadata = serde_json::Map::new();
    metadata.insert("count".to_owned(), Value::String(ids.len().to_string()));
    metadata.insert(
        object_id_key.to_owned(),
        Value::String(object_id.to_string()),
    );
    metadata.insert(
        ids_key.to_owned(),
        Value::String(serde_json::to_string(ids).expect("batch ids serialize")),
    );
    Value::Object(metadata)
}

#[allow(clippy::too_many_arguments)]
#[track_caller]
fn assert_mapping_audit_event(
    record: &Value,
    event_name: &str,
    operation: &str,
    user_id: Uuid,
    connection_id: Uuid,
    workspace_id: Uuid,
    request_id: Uuid,
    object_type: &str,
    object_id: Uuid,
    metadata: Value,
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
    assert_eq!(fields["request_id"], request_id.to_string());
    assert_eq!(fields["object_type"], object_type);
    assert_eq!(fields["object_id"], object_id.to_string());
    assert_eq!(
        serde_json::from_str::<Value>(
            fields["metadata"]
                .as_str()
                .expect("audit metadata is serialized JSON"),
        )
        .expect("audit metadata parses"),
        metadata
    );
}
