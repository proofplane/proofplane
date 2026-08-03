use std::collections::BTreeSet;

use proofplane::domain::WorkspacePermission;
use serde_json::Value;

use crate::support::{
    harness, mcp::McpClient, oauth::authorize_agent_connection, scenario::ScenarioBuilder,
};

/// Pins the whole published tool surface: which tools exist, how each one is
/// described to a model, and which fields its schemas expose. Descriptions are
/// part of the contract with agents, so a reworded one should show up here.
#[tokio::test]
async fn tool_catalog_matches_the_published_surface() {
    let app = harness::app().await;
    let subject = "auth0|mcp-tool-catalog";

    ScenarioBuilder::new(&app)
        .with_user(subject)
        .with_workspace(subject, "Tool Catalog")
        .build()
        .await;

    let token =
        authorize_agent_connection(&app, subject, "Claude", &WorkspacePermission::ALL).await;
    let client = McpClient::connect(app.mcp_server(), &token).await;
    let tools = client.list_tools().await;

    let tool_names = tools
        .iter()
        .map(|tool| tool["name"].as_str().expect("tool has a name"))
        .collect::<BTreeSet<_>>();
    let expected_tool_names = [
        "create_evidence",
        "list_evidence",
        "get_evidence",
        "list_evidence_submissions",
        "get_evidence_submission",
        "get_latest_evidence_submission",
        "prepare_evidence_submission_upload",
        "manage_policy_document",
        "manage_evidence_submissions",
        "create_auditor_access_link",
        "list_auditor_access_links",
        "revoke_auditor_access_link",
        "list_frameworks",
        "list_framework_requirements",
        "list_controls",
        "get_control",
        "create_control",
        "replace_control",
        "list_evidence_control_mappings",
        "map_evidence_to_control",
        "map_evidence_to_controls",
        "map_control_to_evidence",
        "unmap_evidence_from_controls",
        "unmap_control_from_evidence",
        "remove_evidence_control_mapping",
        "list_policies",
        "get_policy",
        "prepare_policy_document_upload",
        "create_policy",
        "update_policy",
        "archive_policy",
        "attach_policy_to_control",
        "attach_policy_to_controls",
        "attach_control_to_policies",
        "detach_policy_from_control",
        "detach_policy_from_controls",
        "detach_control_from_policies",
        "get_proofplane_guide",
    ]
    .into_iter()
    .collect::<BTreeSet<_>>();
    assert_eq!(tool_names, expected_tool_names);
    let expected_descriptions = [
        (
            "create_evidence",
            "Create a piece of evidence that states what the organization must prove and how to collect it; for guidance, call get_proofplane_guide with topic submitting-evidence.",
        ),
        (
            "list_evidence",
            "List evidence with their collection instructions and status; for guidance, call get_proofplane_guide with topic submitting-evidence.",
        ),
        (
            "get_evidence",
            "Get one piece of evidence with its collection instructions and status by evidence ID; for guidance, call get_proofplane_guide with topic submitting-evidence.",
        ),
        (
            "list_evidence_submissions",
            "List the submissions for a piece of evidence, each one file with its coverage window, provenance, and document metadata; for guidance, call get_proofplane_guide with topic submitting-evidence.",
        ),
        (
            "get_evidence_submission",
            "Get one evidence submission with its coverage window, provenance, and document metadata by submission ID; for guidance, call get_proofplane_guide with topic submitting-evidence.",
        ),
        (
            "get_latest_evidence_submission",
            "Get the latest submission for a piece of evidence with its coverage window, provenance, and document metadata; for guidance, call get_proofplane_guide with topic submitting-evidence.",
        ),
        (
            "manage_evidence_submissions",
            "Use this when a human will upload in a browser: create a short-lived bearer-secret URL for one or more evidence files in a coverage window; each file becomes one submission; for guidance, call get_proofplane_guide with topic submitting-evidence.",
        ),
        (
            "prepare_evidence_submission_upload",
            "Use this when a trusted runtime can read a local file and execute HTTP PUT: prepare a short-lived bearer-secret descriptor without sending the file path or bytes through MCP; for guidance, call get_proofplane_guide with topic submitting-evidence.",
        ),
        (
            "manage_policy_document",
            "Create a short-lived bearer-secret browser URL for a human to manage an active policy’s document; file bytes never pass through MCP; for guidance, call get_proofplane_guide with topic policies.",
        ),
        (
            "create_auditor_access_link",
            "Create a bearer-secret browser link that lets the named auditor review compliance evidence whose coverage window overlaps the audit period from period_start to period_end, and cannot see or download anything outside it, until the grant expires.",
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
            "list_evidence_control_mappings",
            "List the controls mapped to a piece of evidence, including each mapping rationale; for guidance, call get_proofplane_guide with topic controls-and-mappings.",
        ),
        (
            "map_evidence_to_control",
            "Map a piece of evidence to a control with a rationale explaining how that proof supports it; for guidance, call get_proofplane_guide with topic controls-and-mappings.",
        ),
        (
            "map_evidence_to_controls",
            "Map one piece of evidence to many controls in a single all-or-nothing batch, each with its own rationale; if any control id is unknown or already mapped the whole batch is rejected; for guidance, call get_proofplane_guide with topic controls-and-mappings.",
        ),
        (
            "map_control_to_evidence",
            "Map one control to many pieces of evidence in a single all-or-nothing batch, each with its own rationale; if any evidence id is unknown or already mapped the whole batch is rejected; for guidance, call get_proofplane_guide with topic controls-and-mappings.",
        ),
        (
            "unmap_evidence_from_controls",
            "Remove the mappings between one piece of evidence and many controls in a single all-or-nothing batch; if any control id is unknown or not currently mapped the whole batch is rejected; for guidance, call get_proofplane_guide with topic controls-and-mappings.",
        ),
        (
            "unmap_control_from_evidence",
            "Remove the mappings between one control and many pieces of evidence in a single all-or-nothing batch; if any evidence id is unknown or not currently mapped the whole batch is rejected; for guidance, call get_proofplane_guide with topic controls-and-mappings.",
        ),
        (
            "remove_evidence_control_mapping",
            "Remove the mapping between a piece of evidence and a control by their IDs; for guidance, call get_proofplane_guide with topic controls-and-mappings.",
        ),
        (
            "list_policies",
            "List active policies with their mapped-control counts and current document status; for guidance, call get_proofplane_guide with topic policies.",
        ),
        (
            "get_policy",
            "Get one active policy with its mapped controls and safe current document metadata by policy ID; for guidance, call get_proofplane_guide with topic policies.",
        ),
        (
            "prepare_policy_document_upload",
            "Use this when a trusted runtime can read a local policy file and execute HTTP PUT: prepare a short-lived bearer-secret descriptor without sending the file path or bytes through MCP; for guidance, call get_proofplane_guide with topic policies.",
        ),
        (
            "create_policy",
            "Create a policy with optional control mappings and return its complete active metadata; for guidance, call get_proofplane_guide with topic policies.",
        ),
        (
            "update_policy",
            "Update an active policy’s name and optional description without changing mappings or document state; for guidance, call get_proofplane_guide with topic policies.",
        ),
        (
            "archive_policy",
            "Archive an active policy when its current document is not being processed; for guidance, call get_proofplane_guide with topic policies.",
        ),
        (
            "attach_policy_to_control",
            "Attach an active policy to a control without changing the control or its other mappings; for guidance, call get_proofplane_guide with topic policies.",
        ),
        (
            "attach_policy_to_controls",
            "Attach one active policy to many controls in a single all-or-nothing batch; if any control id is unknown or already attached the whole batch is rejected; for guidance, call get_proofplane_guide with topic policies.",
        ),
        (
            "attach_control_to_policies",
            "Attach one control to many active policies in a single all-or-nothing batch; if any policy id is unknown, archived, or already attached the whole batch is rejected; for guidance, call get_proofplane_guide with topic policies.",
        ),
        (
            "detach_policy_from_control",
            "Detach an active policy from a control without changing the control or its other mappings; for guidance, call get_proofplane_guide with topic policies.",
        ),
        (
            "detach_policy_from_controls",
            "Remove the mappings between one active policy and many controls in a single all-or-nothing batch; if any control id is unknown or not currently mapped the whole batch is rejected; for guidance, call get_proofplane_guide with topic policies.",
        ),
        (
            "detach_control_from_policies",
            "Remove the mappings between one control and many active policies in a single all-or-nothing batch; if any policy id is unknown, archived, or not currently mapped the whole batch is rejected; for guidance, call get_proofplane_guide with topic policies.",
        ),
        (
            "get_proofplane_guide",
            "Return embedded Proofplane guidance for a topic, or the ordered topic index when the topic is omitted or unknown.",
        ),
    ];
    for (name, expected_description) in expected_descriptions {
        assert_eq!(
            find_tool(&tools, name)["description"],
            expected_description,
            "{name} exposes its expected description"
        );
    }
    let upload_description = find_tool(&tools, "manage_evidence_submissions")["description"]
        .as_str()
        .expect("upload tool has a description");
    assert!(upload_description.contains("each file becomes one submission"));
    assert_schema_has_property(
        &find_tool(&tools, "get_proofplane_guide")["inputSchema"],
        "topic",
    );
    assert_schema_lacks_property(
        &find_tool(&tools, "get_proofplane_guide")["inputSchema"],
        "workspace_id",
    );
    assert_schema_has_property(
        &find_tool(&tools, "create_evidence")["inputSchema"],
        "title",
    );
    assert_schema_has_property(
        &find_tool(&tools, "create_evidence")["inputSchema"],
        "collection_instructions",
    );
    assert_schema_lacks_property(
        &find_tool(&tools, "create_evidence")["inputSchema"],
        "workspace_id",
    );
    assert_schema_lacks_property(
        &find_tool(&tools, "list_evidence_submissions")["inputSchema"],
        "workspace_id",
    );
    assert_schema_has_property(
        &find_tool(&tools, "list_evidence_submissions")["inputSchema"],
        "evidence_id",
    );
    assert_schema_has_property(
        &find_tool(&tools, "get_evidence_submission")["inputSchema"],
        "submission_id",
    );
    assert_schema_has_property(
        &find_tool(&tools, "manage_evidence_submissions")["inputSchema"],
        "valid_from",
    );
    assert_schema_has_property(
        &find_tool(&tools, "manage_evidence_submissions")["inputSchema"],
        "valid_until",
    );
    assert_schema_has_property(
        &find_tool(&tools, "manage_evidence_submissions")["inputSchema"],
        "evidence_id",
    );
    assert_schema_lacks_property(
        &find_tool(&tools, "manage_evidence_submissions")["inputSchema"],
        "submission_id",
    );
    assert_schema_has_property(
        &find_tool(&tools, "manage_policy_document")["inputSchema"],
        "policy_id",
    );
    assert_schema_has_property(
        &find_tool(&tools, "create_auditor_access_link")["inputSchema"],
        "email",
    );
    assert_schema_has_property(
        &find_tool(&tools, "create_auditor_access_link")["inputSchema"],
        "expires_at",
    );
    assert_schema_has_property(
        &find_tool(&tools, "create_auditor_access_link")["inputSchema"],
        "period_start",
    );
    assert_schema_has_property(
        &find_tool(&tools, "create_auditor_access_link")["inputSchema"],
        "period_end",
    );
    assert_schema_has_property(
        &find_tool(&tools, "revoke_auditor_access_link")["inputSchema"],
        "grant_id",
    );
    assert_schema_has_property(
        &find_tool(&tools, "map_evidence_to_control")["inputSchema"],
        "rationale",
    );
    assert_schema_has_property(
        &find_tool(&tools, "map_evidence_to_controls")["inputSchema"],
        "items",
    );
    assert_schema_has_property(
        &find_tool(&tools, "map_control_to_evidence")["inputSchema"],
        "items",
    );
    assert_schema_has_property(
        &find_tool(&tools, "remove_evidence_control_mapping")["inputSchema"],
        "control_id",
    );
    assert_schema_has_property(&find_tool(&tools, "get_policy")["inputSchema"], "policy_id");
    assert_schema_has_property(
        &find_tool(&tools, "create_policy")["inputSchema"],
        "control_ids",
    );
    assert_schema_has_property(
        &find_tool(&tools, "update_policy")["inputSchema"],
        "description",
    );
    assert_schema_has_property(
        &find_tool(&tools, "attach_policy_to_control")["inputSchema"],
        "control_id",
    );
    assert_schema_has_property(
        &find_tool(&tools, "attach_policy_to_controls")["inputSchema"],
        "control_ids",
    );
    assert_schema_has_property(
        &find_tool(&tools, "attach_control_to_policies")["inputSchema"],
        "policy_ids",
    );
    assert_schema_has_property(
        &find_tool(&tools, "detach_policy_from_controls")["inputSchema"],
        "control_ids",
    );
    assert_schema_has_property(
        &find_tool(&tools, "detach_control_from_policies")["inputSchema"],
        "policy_ids",
    );
    assert_schema_has_property(
        &find_tool(&tools, "list_framework_requirements")["inputSchema"],
        "framework_id",
    );
    assert_schema_lacks_property(
        &find_tool(&tools, "list_framework_requirements")["inputSchema"],
        "workspace_id",
    );
    assert_schema_has_property(
        &find_tool(&tools, "get_control")["inputSchema"],
        "control_id",
    );
    assert_schema_lacks_property(
        &find_tool(&tools, "get_control")["inputSchema"],
        "workspace_id",
    );
    assert_schema_has_property(
        &find_tool(&tools, "create_control")["inputSchema"],
        "framework_requirement_ids",
    );
    assert_schema_has_property(
        &find_tool(&tools, "replace_control")["inputSchema"],
        "framework_requirement_ids",
    );
    assert_schema_has_property(
        &find_tool(&tools, "list_evidence")["outputSchema"],
        "evidence",
    );
    assert_schema_has_property(
        &find_tool(&tools, "create_evidence")["outputSchema"],
        "evidence",
    );
    assert_schema_has_property(
        &find_tool(&tools, "get_evidence_submission")["outputSchema"],
        "submission",
    );
    assert_schema_has_property(
        &find_tool(&tools, "get_evidence_submission")["outputSchema"],
        "document",
    );
    assert_schema_has_property(
        &find_tool(&tools, "list_controls")["outputSchema"],
        "controls",
    );
    assert_schema_has_property(
        &find_tool(&tools, "list_frameworks")["outputSchema"],
        "frameworks",
    );
    assert_schema_has_property(
        &find_tool(&tools, "list_framework_requirements")["outputSchema"],
        "requirements",
    );
    assert_schema_has_property(&find_tool(&tools, "get_control")["outputSchema"], "id");
    assert_schema_has_property(
        &find_tool(&tools, "create_control")["outputSchema"],
        "framework_requirements",
    );
    assert_schema_has_property(
        &find_tool(&tools, "replace_control")["outputSchema"],
        "framework_requirements",
    );
    assert_schema_has_property(
        &find_tool(&tools, "list_evidence_control_mappings")["outputSchema"],
        "mappings",
    );
    assert_schema_has_property(
        &find_tool(&tools, "map_evidence_to_control")["outputSchema"],
        "control",
    );
    assert_schema_has_property(
        &find_tool(&tools, "map_evidence_to_controls")["outputSchema"],
        "control_ids",
    );
    assert_schema_has_property(
        &find_tool(&tools, "map_control_to_evidence")["outputSchema"],
        "evidence_ids",
    );
    assert_schema_has_property(
        &find_tool(&tools, "remove_evidence_control_mapping")["outputSchema"],
        "removed",
    );
    assert_schema_has_property(
        &find_tool(&tools, "list_policies")["outputSchema"],
        "policies",
    );
    assert_schema_has_property(&find_tool(&tools, "get_policy")["outputSchema"], "controls");
    assert_schema_has_property(
        &find_tool(&tools, "create_policy")["outputSchema"],
        "document",
    );
    assert_schema_has_property(
        &find_tool(&tools, "archive_policy")["outputSchema"],
        "archived_at",
    );
    assert_schema_has_property(
        &find_tool(&tools, "detach_policy_from_control")["outputSchema"],
        "policy_id",
    );
    assert_schema_has_property(
        &find_tool(&tools, "attach_policy_to_controls")["outputSchema"],
        "control_ids",
    );
    assert_schema_has_property(
        &find_tool(&tools, "attach_control_to_policies")["outputSchema"],
        "policy_ids",
    );
    assert_schema_has_property(
        &find_tool(&tools, "detach_policy_from_controls")["outputSchema"],
        "control_ids",
    );
    assert_schema_has_property(
        &find_tool(&tools, "detach_control_from_policies")["outputSchema"],
        "policy_ids",
    );
    assert_schema_has_property(
        &find_tool(&tools, "manage_evidence_submissions")["outputSchema"],
        "url_secret_type",
    );
    assert_schema_has_property(
        &find_tool(&tools, "manage_evidence_submissions")["outputSchema"],
        "expires_at",
    );
    assert_schema_has_property(
        &find_tool(&tools, "manage_evidence_submissions")["outputSchema"],
        "intended_use",
    );
    assert_schema_has_property(
        &find_tool(&tools, "manage_policy_document")["outputSchema"],
        "url_secret_type",
    );
    assert_schema_has_property(
        &find_tool(&tools, "manage_policy_document")["outputSchema"],
        "policy_id",
    );
    assert_schema_has_property(
        &find_tool(&tools, "create_auditor_access_link")["outputSchema"],
        "url",
    );
    assert_schema_has_property(
        &find_tool(&tools, "create_auditor_access_link")["outputSchema"],
        "grant",
    );
    for property in [
        "id",
        "auditor_email",
        "created_at",
        "expires_at",
        "period_start",
        "period_end",
        "revoked_at",
    ] {
        assert_schema_has_property(
            &find_tool(&tools, "create_auditor_access_link")["outputSchema"],
            property,
        );
        assert_schema_has_property(
            &find_tool(&tools, "list_auditor_access_links")["outputSchema"],
            property,
        );
        assert_schema_has_property(
            &find_tool(&tools, "revoke_auditor_access_link")["outputSchema"],
            property,
        );
    }
    assert_schema_has_property(
        &find_tool(&tools, "create_auditor_access_link")["outputSchema"],
        "url_secret_type",
    );
    assert_schema_has_property(
        &find_tool(&tools, "create_auditor_access_link")["outputSchema"],
        "intended_use",
    );
    assert_schema_has_property(
        &find_tool(&tools, "list_auditor_access_links")["outputSchema"],
        "grants",
    );
    assert_schema_has_property(
        &find_tool(&tools, "revoke_auditor_access_link")["outputSchema"],
        "grant",
    );
    for property in ["topic", "title", "markdown", "topics"] {
        assert_schema_has_property(
            &find_tool(&tools, "get_proofplane_guide")["outputSchema"],
            property,
        );
    }
}

fn find_tool<'a>(tools: &'a [Value], name: &str) -> &'a Value {
    tools
        .iter()
        .find(|tool| tool["name"] == name)
        .unwrap_or_else(|| panic!("{name} tool is registered"))
}

#[track_caller]
fn assert_schema_has_property(schema: &Value, property: &str) {
    assert!(
        schema_has_property(schema, property),
        "schema exposes {property}: {schema}"
    );
}

#[track_caller]
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
