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
    scenario::{types::TestControl, ScenarioBuilder},
};

#[tokio::test]
async fn single_mapping_round_trips_conflicts_conceals_writes_and_removes_once() {
    let app = harness::app().await;
    let subject = "auth0|mcp-evidence-control-single";
    let workspace_name = "MCP Evidence Control Single";

    let scenario = ScenarioBuilder::new(&app)
        .with_user(subject)
        .with_workspace(subject, workspace_name)
        .with_evidence(workspace_name, "Quarterly access evidence")
        .with_control(workspace_name, "PP-AC-01", "Quarterly access review")
        .build()
        .await;
    let user_id = scenario.user(subject).id;
    let workspace = scenario.workspace(workspace_name);
    let workspace_id = workspace.id;
    let evidence_id = workspace.evidence("Quarterly access evidence").id;
    let control = workspace.control("PP-AC-01");

    let manager_token = authorize_agent_connection(
        &app,
        subject,
        "Evidence Control Single Manager",
        &[
            WorkspacePermission::ReadControls,
            WorkspacePermission::WriteControls,
        ],
    )
    .await;
    let read_only_token = authorize_agent_connection(
        &app,
        subject,
        "Evidence Control Single Reader",
        &[WorkspacePermission::ReadControls],
    )
    .await;
    let manager_connection_id =
        get_agent_connection_id_for(&app, subject, "Evidence Control Single Manager").await;

    let manager = McpClient::connect(app.mcp_server(), &manager_token).await;
    assert_eq!(
        manager
            .call_tool(
                "list_evidence_control_mappings",
                json!({ "evidence_id": evidence_id }),
            )
            .await,
        json!({ "mappings": [] })
    );

    let ((created, create_request_id), create_logs) = app
        .capture_audit_logs(async |request_id| {
            let created =
                McpClient::connect_with_request_id(app.mcp_server(), &manager_token, request_id)
                    .await
                    .call_tool(
                        "map_evidence_to_control",
                        json!({
                            "evidence_id": evidence_id,
                            "control_id": control.id,
                            "rationale": "Demonstrates the quarterly access review.",
                        }),
                    )
                    .await;
            (created, request_id)
        })
        .await;
    assert_mapping_projection(
        &created,
        evidence_id,
        control,
        "Demonstrates the quarterly access review.",
    );
    assert_eq!(create_logs.len(), 1);
    assert_mapping_audit_event(
        &create_logs[0],
        "evidence_control_mapping.created",
        "map_evidence_to_control",
        user_id,
        manager_connection_id,
        workspace_id,
        create_request_id,
        "evidence_control_mapping",
        control.id,
        json!({
            "control_id": control.id.to_string(),
            "evidence_id": evidence_id.to_string(),
        }),
    );

    let complete_listing = manager
        .call_tool(
            "list_evidence_control_mappings",
            json!({ "evidence_id": evidence_id }),
        )
        .await;
    assert_eq!(complete_listing, json!({ "mappings": [created.clone()] }));

    let ((duplicate, denied), rejection_logs) = app
        .capture_audit_logs(async |request_id| {
            let manager =
                McpClient::connect_with_request_id(app.mcp_server(), &manager_token, request_id)
                    .await;
            let reader =
                McpClient::connect_with_request_id(app.mcp_server(), &read_only_token, request_id)
                    .await;
            let duplicate = manager
                .call_tool_error(
                    "map_evidence_to_control",
                    json!({
                        "evidence_id": evidence_id,
                        "control_id": control.id,
                        "rationale": "Duplicate mapping.",
                    }),
                )
                .await;
            let denied = reader
                .call_tool_error(
                    "remove_evidence_control_mapping",
                    json!({
                        "evidence_id": evidence_id,
                        "control_id": control.id,
                    }),
                )
                .await;
            (duplicate, denied)
        })
        .await;
    assert_mapping_exists(&duplicate);
    assert_not_found(&denied);
    assert!(rejection_logs.is_empty());
    assert_eq!(
        manager
            .call_tool(
                "list_evidence_control_mappings",
                json!({ "evidence_id": evidence_id }),
            )
            .await,
        complete_listing
    );

    let ((removed, remove_request_id), remove_logs) = app
        .capture_audit_logs(async |request_id| {
            let removed =
                McpClient::connect_with_request_id(app.mcp_server(), &manager_token, request_id)
                    .await
                    .call_tool(
                        "remove_evidence_control_mapping",
                        json!({
                            "evidence_id": evidence_id,
                            "control_id": control.id,
                        }),
                    )
                    .await;
            (removed, request_id)
        })
        .await;
    assert_eq!(
        removed,
        json!({
            "removed": true,
            "evidence_id": evidence_id,
            "control_id": control.id,
        })
    );
    assert_eq!(remove_logs.len(), 1);
    assert_mapping_audit_event(
        &remove_logs[0],
        "evidence_control_mapping.deleted",
        "remove_evidence_control_mapping",
        user_id,
        manager_connection_id,
        workspace_id,
        remove_request_id,
        "evidence_control_mapping",
        control.id,
        json!({
            "control_id": control.id.to_string(),
            "evidence_id": evidence_id.to_string(),
        }),
    );

    let ((missing, final_listing), missing_logs) = app
        .capture_audit_logs(async |request_id| {
            let manager =
                McpClient::connect_with_request_id(app.mcp_server(), &manager_token, request_id)
                    .await;
            let missing = manager
                .call_tool_error(
                    "remove_evidence_control_mapping",
                    json!({
                        "evidence_id": evidence_id,
                        "control_id": control.id,
                    }),
                )
                .await;
            let final_listing = manager
                .call_tool(
                    "list_evidence_control_mappings",
                    json!({ "evidence_id": evidence_id }),
                )
                .await;
            (missing, final_listing)
        })
        .await;
    assert_not_found(&missing);
    assert!(missing_logs.is_empty());
    assert_eq!(final_listing, json!({ "mappings": [] }));
}

#[tokio::test]
async fn map_evidence_to_controls_creates_two_in_request_order_and_rejects_atomically() {
    let app = harness::app().await;
    let subject = "auth0|mcp-map-evidence-controls";
    let workspace_name = "MCP Map Evidence Controls";

    let scenario = ScenarioBuilder::new(&app)
        .with_user(subject)
        .with_workspace(subject, workspace_name)
        .with_evidence(workspace_name, "Access review exports")
        .with_control(workspace_name, "PP-MAP-A", "Access review")
        .with_control(workspace_name, "PP-MAP-B", "Change review")
        .with_control(workspace_name, "PP-MAP-C", "Incident review")
        .build()
        .await;
    let user_id = scenario.user(subject).id;
    let workspace = scenario.workspace(workspace_name);
    let workspace_id = workspace.id;
    let evidence_id = workspace.evidence("Access review exports").id;
    let first_control = workspace.control("PP-MAP-A");
    let second_control = workspace.control("PP-MAP-B");
    let unmapped_control = workspace.control("PP-MAP-C");

    let token = authorize_agent_connection(
        &app,
        subject,
        "Map Evidence Controls Manager",
        &[
            WorkspacePermission::ReadControls,
            WorkspacePermission::WriteControls,
        ],
    )
    .await;
    let connection_id =
        get_agent_connection_id_for(&app, subject, "Map Evidence Controls Manager").await;
    let client = McpClient::connect(app.mcp_server(), &token).await;

    let empty_listing = client
        .call_tool(
            "list_evidence_control_mappings",
            json!({ "evidence_id": evidence_id }),
        )
        .await;
    assert_eq!(empty_listing, json!({ "mappings": [] }));

    let ((created, request_id), logs) = app
        .capture_audit_logs(async |request_id| {
            let created = McpClient::connect_with_request_id(app.mcp_server(), &token, request_id)
                .await
                .call_tool(
                    "map_evidence_to_controls",
                    json!({
                        "evidence_id": evidence_id,
                        "items": [
                            {
                                "control_id": second_control.id,
                                "rationale": "Covers change review.",
                            },
                            {
                                "control_id": first_control.id,
                                "rationale": "Covers access review.",
                            },
                        ],
                    }),
                )
                .await;
            (created, request_id)
        })
        .await;
    assert_eq!(
        created,
        json!({
            "evidence_id": evidence_id,
            "count": 2,
            "control_ids": [second_control.id, first_control.id],
        })
    );
    assert_eq!(logs.len(), 1);
    assert_mapping_audit_event(
        &logs[0],
        "evidence_control_mappings.created",
        "map_evidence_to_controls",
        user_id,
        connection_id,
        workspace_id,
        request_id,
        "evidence",
        evidence_id,
        batch_metadata(
            "evidence_id",
            evidence_id,
            "control_ids",
            &[second_control.id, first_control.id],
        ),
    );

    let complete_listing = client
        .call_tool(
            "list_evidence_control_mappings",
            json!({ "evidence_id": evidence_id }),
        )
        .await;
    let mappings = complete_listing["mappings"]
        .as_array()
        .expect("mappings is an array");
    assert_eq!(mappings.len(), 2);
    assert_mapping_projection(
        &mappings[0],
        evidence_id,
        first_control,
        "Covers access review.",
    );
    assert_mapping_projection(
        &mappings[1],
        evidence_id,
        second_control,
        "Covers change review.",
    );

    let unknown_control_id = Uuid::new_v4();
    let ((rejected, after_rejection), rejection_logs) = app
        .capture_audit_logs(async |request_id| {
            let client =
                McpClient::connect_with_request_id(app.mcp_server(), &token, request_id).await;
            let rejected = client
                .call_tool_error(
                    "map_evidence_to_controls",
                    json!({
                        "evidence_id": evidence_id,
                        "items": [
                            {
                                "control_id": unmapped_control.id,
                                "rationale": "Would cover incident review.",
                            },
                            {
                                "control_id": unknown_control_id,
                                "rationale": "Unknown control.",
                            },
                        ],
                    }),
                )
                .await;
            let after_rejection = client
                .call_tool(
                    "list_evidence_control_mappings",
                    json!({ "evidence_id": evidence_id }),
                )
                .await;
            (rejected, after_rejection)
        })
        .await;
    assert_batch_rejected(
        &rejected,
        "control_ids",
        "control_ids contains unknown or already-mapped ids",
        &[unknown_control_id],
        "already_mapped_ids",
        &[],
    );
    assert!(rejection_logs.is_empty());
    assert_eq!(after_rejection, complete_listing);
}

#[tokio::test]
async fn map_control_to_evidence_creates_two_in_request_order_and_rejects_atomically() {
    let app = harness::app().await;
    let subject = "auth0|mcp-map-control-evidence";
    let workspace_name = "MCP Map Control Evidence";

    let scenario = ScenarioBuilder::new(&app)
        .with_user(subject)
        .with_workspace(subject, workspace_name)
        .with_evidence(workspace_name, "Alpha evidence")
        .with_evidence(workspace_name, "Bravo evidence")
        .with_evidence(workspace_name, "Charlie evidence")
        .with_control(workspace_name, "PP-MAP-ONE", "Evidence review")
        .build()
        .await;
    let user_id = scenario.user(subject).id;
    let workspace = scenario.workspace(workspace_name);
    let workspace_id = workspace.id;
    let first_evidence_id = workspace.evidence("Alpha evidence").id;
    let second_evidence_id = workspace.evidence("Bravo evidence").id;
    let new_evidence_id = workspace.evidence("Charlie evidence").id;
    let control = workspace.control("PP-MAP-ONE");

    let token = authorize_agent_connection(
        &app,
        subject,
        "Map Control Evidence Manager",
        &[
            WorkspacePermission::ReadControls,
            WorkspacePermission::WriteControls,
        ],
    )
    .await;
    let connection_id =
        get_agent_connection_id_for(&app, subject, "Map Control Evidence Manager").await;
    let client = McpClient::connect(app.mcp_server(), &token).await;
    for evidence_id in [first_evidence_id, second_evidence_id, new_evidence_id] {
        assert_eq!(
            client
                .call_tool(
                    "list_evidence_control_mappings",
                    json!({ "evidence_id": evidence_id }),
                )
                .await,
            json!({ "mappings": [] })
        );
    }

    let ((created, request_id), logs) = app
        .capture_audit_logs(async |request_id| {
            let created = McpClient::connect_with_request_id(app.mcp_server(), &token, request_id)
                .await
                .call_tool(
                    "map_control_to_evidence",
                    json!({
                        "control_id": control.id,
                        "items": [
                            {
                                "evidence_id": second_evidence_id,
                                "rationale": "Bravo proof.",
                            },
                            {
                                "evidence_id": first_evidence_id,
                                "rationale": "Alpha proof.",
                            },
                        ],
                    }),
                )
                .await;
            (created, request_id)
        })
        .await;
    assert_eq!(
        created,
        json!({
            "control_id": control.id,
            "count": 2,
            "evidence_ids": [second_evidence_id, first_evidence_id],
        })
    );
    assert_eq!(logs.len(), 1);
    assert_mapping_audit_event(
        &logs[0],
        "evidence_control_mappings.created",
        "map_control_to_evidence",
        user_id,
        connection_id,
        workspace_id,
        request_id,
        "control",
        control.id,
        batch_metadata(
            "control_id",
            control.id,
            "evidence_ids",
            &[second_evidence_id, first_evidence_id],
        ),
    );

    let first_before = client
        .call_tool(
            "list_evidence_control_mappings",
            json!({ "evidence_id": first_evidence_id }),
        )
        .await;
    let second_before = client
        .call_tool(
            "list_evidence_control_mappings",
            json!({ "evidence_id": second_evidence_id }),
        )
        .await;
    let new_before = client
        .call_tool(
            "list_evidence_control_mappings",
            json!({ "evidence_id": new_evidence_id }),
        )
        .await;
    assert_single_mapping_listing(&first_before, first_evidence_id, control, "Alpha proof.");
    assert_single_mapping_listing(&second_before, second_evidence_id, control, "Bravo proof.");
    assert_eq!(new_before, json!({ "mappings": [] }));

    let ((rejected, first_after, second_after, new_after), rejection_logs) = app
        .capture_audit_logs(async |request_id| {
            let client =
                McpClient::connect_with_request_id(app.mcp_server(), &token, request_id).await;
            let rejected = client
                .call_tool_error(
                    "map_control_to_evidence",
                    json!({
                        "control_id": control.id,
                        "items": [
                            {
                                "evidence_id": new_evidence_id,
                                "rationale": "Would add Charlie proof.",
                            },
                            {
                                "evidence_id": first_evidence_id,
                                "rationale": "Already mapped Alpha proof.",
                            },
                        ],
                    }),
                )
                .await;
            let first_after = client
                .call_tool(
                    "list_evidence_control_mappings",
                    json!({ "evidence_id": first_evidence_id }),
                )
                .await;
            let second_after = client
                .call_tool(
                    "list_evidence_control_mappings",
                    json!({ "evidence_id": second_evidence_id }),
                )
                .await;
            let new_after = client
                .call_tool(
                    "list_evidence_control_mappings",
                    json!({ "evidence_id": new_evidence_id }),
                )
                .await;
            (rejected, first_after, second_after, new_after)
        })
        .await;
    assert_batch_rejected(
        &rejected,
        "evidence_ids",
        "evidence_ids contains unknown or already-mapped ids",
        &[],
        "already_mapped_ids",
        &[first_evidence_id],
    );
    assert!(rejection_logs.is_empty());
    assert_eq!(first_after, first_before);
    assert_eq!(second_after, second_before);
    assert_eq!(new_after, new_before);
}

#[tokio::test]
async fn unmap_evidence_from_controls_removes_two_in_request_order_and_rejects_atomically() {
    let app = harness::app().await;
    let subject = "auth0|mcp-unmap-evidence-controls";
    let workspace_name = "MCP Unmap Evidence Controls";

    let scenario = ScenarioBuilder::new(&app)
        .with_user(subject)
        .with_workspace(subject, workspace_name)
        .with_evidence(workspace_name, "Unmapping evidence")
        .with_control(workspace_name, "PP-UNMAP-A", "Alpha review")
        .with_control(workspace_name, "PP-UNMAP-B", "Bravo review")
        .with_control(workspace_name, "PP-UNMAP-C", "Charlie review")
        .with_control(workspace_name, "PP-UNMAP-D", "Delta review")
        .with_evidence_control_mapping(
            workspace_name,
            "Unmapping evidence",
            "PP-UNMAP-B",
            "Bravo proof.",
        )
        .with_evidence_control_mapping(
            workspace_name,
            "Unmapping evidence",
            "PP-UNMAP-C",
            "Charlie proof.",
        )
        .with_evidence_control_mapping(
            workspace_name,
            "Unmapping evidence",
            "PP-UNMAP-A",
            "Alpha proof.",
        )
        .build()
        .await;
    let user_id = scenario.user(subject).id;
    let workspace = scenario.workspace(workspace_name);
    let workspace_id = workspace.id;
    let evidence_id = workspace.evidence("Unmapping evidence").id;
    let first_control = workspace.control("PP-UNMAP-A");
    let second_control = workspace.control("PP-UNMAP-B");
    let remaining_control = workspace.control("PP-UNMAP-C");
    let not_mapped_control = workspace.control("PP-UNMAP-D");

    let token = authorize_agent_connection(
        &app,
        subject,
        "Unmap Evidence Controls Manager",
        &[
            WorkspacePermission::ReadControls,
            WorkspacePermission::WriteControls,
        ],
    )
    .await;
    let connection_id =
        get_agent_connection_id_for(&app, subject, "Unmap Evidence Controls Manager").await;
    let client = McpClient::connect(app.mcp_server(), &token).await;

    let before_removal = client
        .call_tool(
            "list_evidence_control_mappings",
            json!({ "evidence_id": evidence_id }),
        )
        .await;
    let mappings = before_removal["mappings"]
        .as_array()
        .expect("mappings is an array");
    assert_eq!(mappings.len(), 3);
    assert_mapping_projection(&mappings[0], evidence_id, first_control, "Alpha proof.");
    assert_mapping_projection(&mappings[1], evidence_id, second_control, "Bravo proof.");
    assert_mapping_projection(
        &mappings[2],
        evidence_id,
        remaining_control,
        "Charlie proof.",
    );

    let ((removed, remove_request_id), remove_logs) = app
        .capture_audit_logs(async |request_id| {
            let removed = McpClient::connect_with_request_id(app.mcp_server(), &token, request_id)
                .await
                .call_tool(
                    "unmap_evidence_from_controls",
                    json!({
                        "evidence_id": evidence_id,
                        "control_ids": [second_control.id, first_control.id],
                    }),
                )
                .await;
            (removed, request_id)
        })
        .await;
    assert_eq!(
        removed,
        json!({
            "evidence_id": evidence_id,
            "count": 2,
            "control_ids": [second_control.id, first_control.id],
        })
    );
    assert_eq!(remove_logs.len(), 1);
    assert_mapping_audit_event(
        &remove_logs[0],
        "evidence_control_mappings.deleted",
        "unmap_evidence_from_controls",
        user_id,
        connection_id,
        workspace_id,
        remove_request_id,
        "evidence",
        evidence_id,
        batch_metadata(
            "evidence_id",
            evidence_id,
            "control_ids",
            &[second_control.id, first_control.id],
        ),
    );

    let after_removal = client
        .call_tool(
            "list_evidence_control_mappings",
            json!({ "evidence_id": evidence_id }),
        )
        .await;
    assert_single_mapping_listing(
        &after_removal,
        evidence_id,
        remaining_control,
        "Charlie proof.",
    );

    let ((rejected, after_rejection), rejection_logs) = app
        .capture_audit_logs(async |request_id| {
            let client =
                McpClient::connect_with_request_id(app.mcp_server(), &token, request_id).await;
            let rejected = client
                .call_tool_error(
                    "unmap_evidence_from_controls",
                    json!({
                        "evidence_id": evidence_id,
                        "control_ids": [remaining_control.id, not_mapped_control.id],
                    }),
                )
                .await;
            let after_rejection = client
                .call_tool(
                    "list_evidence_control_mappings",
                    json!({ "evidence_id": evidence_id }),
                )
                .await;
            (rejected, after_rejection)
        })
        .await;
    assert_batch_rejected(
        &rejected,
        "control_ids",
        "control_ids contains unknown or not-mapped ids",
        &[],
        "not_mapped_ids",
        &[not_mapped_control.id],
    );
    assert!(rejection_logs.is_empty());
    assert_eq!(after_rejection, after_removal);
}

#[tokio::test]
async fn unmap_control_from_evidence_removes_two_in_request_order_and_rejects_atomically() {
    let app = harness::app().await;
    let subject = "auth0|mcp-unmap-control-evidence";
    let workspace_name = "MCP Unmap Control Evidence";

    let scenario = ScenarioBuilder::new(&app)
        .with_user(subject)
        .with_workspace(subject, workspace_name)
        .with_evidence(workspace_name, "First mapped evidence")
        .with_evidence(workspace_name, "Second mapped evidence")
        .with_evidence(workspace_name, "Remaining mapped evidence")
        .with_control(workspace_name, "PP-UNMAP-ONE", "Evidence retention review")
        .with_evidence_control_mapping(
            workspace_name,
            "Second mapped evidence",
            "PP-UNMAP-ONE",
            "Second proof.",
        )
        .with_evidence_control_mapping(
            workspace_name,
            "Remaining mapped evidence",
            "PP-UNMAP-ONE",
            "Remaining proof.",
        )
        .with_evidence_control_mapping(
            workspace_name,
            "First mapped evidence",
            "PP-UNMAP-ONE",
            "First proof.",
        )
        .build()
        .await;
    let user_id = scenario.user(subject).id;
    let workspace = scenario.workspace(workspace_name);
    let workspace_id = workspace.id;
    let first_evidence_id = workspace.evidence("First mapped evidence").id;
    let second_evidence_id = workspace.evidence("Second mapped evidence").id;
    let remaining_evidence_id = workspace.evidence("Remaining mapped evidence").id;
    let control = workspace.control("PP-UNMAP-ONE");

    let token = authorize_agent_connection(
        &app,
        subject,
        "Unmap Control Evidence Manager",
        &[
            WorkspacePermission::ReadControls,
            WorkspacePermission::WriteControls,
        ],
    )
    .await;
    let connection_id =
        get_agent_connection_id_for(&app, subject, "Unmap Control Evidence Manager").await;
    let client = McpClient::connect(app.mcp_server(), &token).await;

    let first_before = client
        .call_tool(
            "list_evidence_control_mappings",
            json!({ "evidence_id": first_evidence_id }),
        )
        .await;
    let second_before = client
        .call_tool(
            "list_evidence_control_mappings",
            json!({ "evidence_id": second_evidence_id }),
        )
        .await;
    let remaining_before = client
        .call_tool(
            "list_evidence_control_mappings",
            json!({ "evidence_id": remaining_evidence_id }),
        )
        .await;
    assert_single_mapping_listing(&first_before, first_evidence_id, control, "First proof.");
    assert_single_mapping_listing(&second_before, second_evidence_id, control, "Second proof.");
    assert_single_mapping_listing(
        &remaining_before,
        remaining_evidence_id,
        control,
        "Remaining proof.",
    );

    let ((removed, remove_request_id), remove_logs) = app
        .capture_audit_logs(async |request_id| {
            let removed = McpClient::connect_with_request_id(app.mcp_server(), &token, request_id)
                .await
                .call_tool(
                    "unmap_control_from_evidence",
                    json!({
                        "control_id": control.id,
                        "evidence_ids": [second_evidence_id, first_evidence_id],
                    }),
                )
                .await;
            (removed, request_id)
        })
        .await;
    assert_eq!(
        removed,
        json!({
            "control_id": control.id,
            "count": 2,
            "evidence_ids": [second_evidence_id, first_evidence_id],
        })
    );
    assert_eq!(remove_logs.len(), 1);
    assert_mapping_audit_event(
        &remove_logs[0],
        "evidence_control_mappings.deleted",
        "unmap_control_from_evidence",
        user_id,
        connection_id,
        workspace_id,
        remove_request_id,
        "control",
        control.id,
        batch_metadata(
            "control_id",
            control.id,
            "evidence_ids",
            &[second_evidence_id, first_evidence_id],
        ),
    );

    let first_after = client
        .call_tool(
            "list_evidence_control_mappings",
            json!({ "evidence_id": first_evidence_id }),
        )
        .await;
    let second_after = client
        .call_tool(
            "list_evidence_control_mappings",
            json!({ "evidence_id": second_evidence_id }),
        )
        .await;
    let remaining_after = client
        .call_tool(
            "list_evidence_control_mappings",
            json!({ "evidence_id": remaining_evidence_id }),
        )
        .await;
    assert_eq!(first_after, json!({ "mappings": [] }));
    assert_eq!(second_after, json!({ "mappings": [] }));
    assert_eq!(remaining_after, remaining_before);

    let unknown_evidence_id = Uuid::new_v4();
    let ((rejected, after_rejection), rejection_logs) = app
        .capture_audit_logs(async |request_id| {
            let client =
                McpClient::connect_with_request_id(app.mcp_server(), &token, request_id).await;
            let rejected = client
                .call_tool_error(
                    "unmap_control_from_evidence",
                    json!({
                        "control_id": control.id,
                        "evidence_ids": [remaining_evidence_id, unknown_evidence_id],
                    }),
                )
                .await;
            let after_rejection = client
                .call_tool(
                    "list_evidence_control_mappings",
                    json!({ "evidence_id": remaining_evidence_id }),
                )
                .await;
            (rejected, after_rejection)
        })
        .await;
    assert_batch_rejected(
        &rejected,
        "evidence_ids",
        "evidence_ids contains unknown or not-mapped ids",
        &[unknown_evidence_id],
        "not_mapped_ids",
        &[],
    );
    assert!(rejection_logs.is_empty());
    assert_eq!(after_rejection, remaining_after);
}

#[track_caller]
fn assert_single_mapping_listing(
    listing: &Value,
    evidence_id: Uuid,
    control: &TestControl,
    rationale: &str,
) {
    assert_eq!(object_keys(listing), ["mappings"].into_iter().collect());
    let mappings = listing["mappings"]
        .as_array()
        .expect("mappings is an array");
    assert_eq!(mappings.len(), 1);
    assert_mapping_projection(&mappings[0], evidence_id, control, rationale);
}

#[track_caller]
fn assert_mapping_projection(
    mapping: &Value,
    evidence_id: Uuid,
    control: &TestControl,
    rationale: &str,
) {
    assert_eq!(
        object_keys(mapping),
        ["control", "created_at", "evidence_id", "rationale"]
            .into_iter()
            .collect()
    );
    assert_eq!(mapping["evidence_id"], evidence_id.to_string());
    assert_eq!(mapping["rationale"], rationale);
    assert_rfc3339(&mapping["created_at"]);
    assert_eq!(
        mapping["control"],
        json!({
            "id": control.id,
            "code": control.code,
            "title": control.title,
            "description": control.description,
        })
    );
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

#[track_caller]
fn assert_batch_rejected(
    error: &McpError,
    field: &str,
    message: &str,
    unknown_ids: &[Uuid],
    other_bucket: &str,
    other_ids: &[Uuid],
) {
    assert_eq!(error.code, ErrorCode::INVALID_PARAMS);
    let mut problem = serde_json::Map::new();
    problem.insert("code".to_owned(), json!("batch_rejected"));
    problem.insert("message".to_owned(), json!(message));
    problem.insert("field".to_owned(), json!(field));
    problem.insert("unknown_ids".to_owned(), json!(unknown_ids));
    problem.insert(other_bucket.to_owned(), json!(other_ids));
    assert_eq!(error.data, json!({ "problem": problem }));
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

#[track_caller]
fn assert_mapping_exists(error: &McpError) {
    assert_eq!(error.code, ErrorCode(-32000));
    assert_eq!(
        error.data,
        json!({
            "problem": {
                "code": "evidence_control_mapping_exists",
                "message": "this control is already mapped to the evidence",
            }
        })
    );
}
