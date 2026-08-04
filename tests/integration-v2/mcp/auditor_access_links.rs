use proofplane::{
    authentication::opaque_token::{ALPHABET, PREFIX, TOKEN_LENGTH},
    domain::WorkspacePermission,
};
use serde_json::{json, Value};
use url::Url;
use uuid::Uuid;

use crate::support::{
    agent_connections::get_agent_connection_id_for,
    harness,
    json::{assert_rfc3339, object_keys},
    mcp::{assert_not_found, assert_validation_error, McpClient},
    oauth::authorize_agent_connection,
    scenario::ScenarioBuilder,
};

const EXPIRES_AT: &str = "2099-01-01T00:00:00.000Z";
const PERIOD_START: &str = "2026-01-01T00:00:00.000Z";
const PERIOD_END: &str = "2026-03-31T23:59:59.000Z";

#[tokio::test]
async fn auditor_links_create_list_and_revoke_complete_scoped_projections_with_safe_audits() {
    let app = harness::app().await;
    let owner = "auth0|mcp-auditor-links-owner";
    let foreign = "auth0|mcp-auditor-links-foreign";

    let scenario = ScenarioBuilder::new(&app)
        .with_user(owner)
        .with_workspace(owner, "MCP Auditor Links Owner")
        .with_user(foreign)
        .with_workspace(foreign, "MCP Auditor Links Foreign")
        .build()
        .await;
    let owner_user_id = scenario.user(owner).id;
    let owner_workspace_id = scenario.workspace("MCP Auditor Links Owner").id;
    let foreign_workspace_id = scenario.workspace("MCP Auditor Links Foreign").id;

    let owner_token = authorize_agent_connection(
        &app,
        owner,
        "Auditor Links Owner",
        &[WorkspacePermission::ManageAuditorAccess],
    )
    .await;
    let foreign_token = authorize_agent_connection(
        &app,
        foreign,
        "Auditor Links Foreign",
        &[WorkspacePermission::ManageAuditorAccess],
    )
    .await;

    let owner_connection_id = get_agent_connection_id_for(&app, owner, "Auditor Links Owner").await;

    let (owner_created, create_logs) = app
        .capture_audit_logs(async |request_id| {
            McpClient::connect_with_request_id(app.mcp_server(), &owner_token, request_id)
                .await
                .call_tool(
                    "create_auditor_access_link",
                    create_arguments(" Owner-Auditor@Example.COM "),
                )
                .await
        })
        .await;
    let (owner_grant_id, owner_invite_token) = assert_create_response(
        &owner_created,
        owner_workspace_id,
        "owner-auditor@example.com",
    );
    assert_eq!(create_logs.len(), 1);
    assert_mcp_auditor_grant_audit_event(
        &create_logs[0],
        "auditor_access_grant.created",
        "create_auditor_access_link",
        owner_user_id,
        owner_connection_id,
        owner_workspace_id,
        owner_grant_id,
        "owner-auditor@example.com",
    );
    assert_eq!(
        owner_created
            .to_string()
            .matches(&owner_invite_token)
            .count(),
        1
    );

    let foreign_client = McpClient::connect(app.mcp_server(), &foreign_token).await;
    let foreign_created = foreign_client
        .call_tool(
            "create_auditor_access_link",
            create_arguments("foreign-auditor@example.com"),
        )
        .await;
    assert_create_response(
        &foreign_created,
        foreign_workspace_id,
        "foreign-auditor@example.com",
    );

    let owner_client = McpClient::connect(app.mcp_server(), &owner_token).await;
    let listed = owner_client
        .call_tool("list_auditor_access_links", json!({}))
        .await;
    assert_eq!(
        listed,
        json!({ "grants": [owner_created["grant"].clone()] })
    );

    let (revoked, revoke_logs) = app
        .capture_audit_logs(async |request_id| {
            McpClient::connect_with_request_id(app.mcp_server(), &owner_token, request_id)
                .await
                .call_tool(
                    "revoke_auditor_access_link",
                    json!({ "grant_id": owner_grant_id }),
                )
                .await
        })
        .await;
    assert_eq!(object_keys(&revoked), ["grant"].into_iter().collect());
    let revoked_at = revoked["grant"]["revoked_at"].clone();
    assert_rfc3339(&revoked_at);
    let mut expected_revoked_grant = owner_created["grant"].clone();
    expected_revoked_grant["revoked_at"] = revoked_at;
    assert_eq!(revoked, json!({ "grant": expected_revoked_grant.clone() }));

    assert_eq!(revoke_logs.len(), 1);
    assert_mcp_auditor_grant_audit_event(
        &revoke_logs[0],
        "auditor_access_grant.revoked",
        "revoke_auditor_access_link",
        owner_user_id,
        owner_connection_id,
        owner_workspace_id,
        owner_grant_id,
        "owner-auditor@example.com",
    );

    let relisted = owner_client
        .call_tool("list_auditor_access_links", json!({}))
        .await;
    assert_eq!(relisted, json!({ "grants": [expected_revoked_grant] }));
}

#[tokio::test]
async fn auditor_link_rejections_are_exact_unaudited_and_leave_owner_listing_unchanged() {
    let app = harness::app().await;
    let owner = "auth0|mcp-auditor-link-rejections-owner";
    let foreign = "auth0|mcp-auditor-link-rejections-foreign";

    let scenario = ScenarioBuilder::new(&app)
        .with_user(owner)
        .with_workspace(owner, "MCP Auditor Link Rejections Owner")
        .with_user(foreign)
        .with_workspace(foreign, "MCP Auditor Link Rejections Foreign")
        .build()
        .await;
    let owner_workspace_id = scenario.workspace("MCP Auditor Link Rejections Owner").id;
    let foreign_workspace_id = scenario.workspace("MCP Auditor Link Rejections Foreign").id;

    let owner_token = authorize_agent_connection(
        &app,
        owner,
        "Auditor Link Rejections Owner",
        &[WorkspacePermission::ManageAuditorAccess],
    )
    .await;
    let foreign_token = authorize_agent_connection(
        &app,
        foreign,
        "Auditor Link Rejections Foreign",
        &[WorkspacePermission::ManageAuditorAccess],
    )
    .await;
    let read_only_token = authorize_agent_connection(
        &app,
        owner,
        "Auditor Link Rejections Reader",
        &[WorkspacePermission::ReadControls],
    )
    .await;

    let owner_client = McpClient::connect(app.mcp_server(), &owner_token).await;
    let owner_created = owner_client
        .call_tool(
            "create_auditor_access_link",
            create_arguments("owner-baseline@example.com"),
        )
        .await;
    assert_create_response(
        &owner_created,
        owner_workspace_id,
        "owner-baseline@example.com",
    );
    let owner_before = owner_client
        .call_tool("list_auditor_access_links", json!({}))
        .await;
    assert_eq!(
        owner_before,
        json!({ "grants": [owner_created["grant"].clone()] })
    );

    let foreign_client = McpClient::connect(app.mcp_server(), &foreign_token).await;
    let foreign_created = foreign_client
        .call_tool(
            "create_auditor_access_link",
            create_arguments("foreign-hidden@example.com"),
        )
        .await;
    let (foreign_grant_id, _) = assert_create_response(
        &foreign_created,
        foreign_workspace_id,
        "foreign-hidden@example.com",
    );

    let (
        (
            invalid_email,
            malformed_expiry,
            missing_period,
            inverted_period,
            malformed_id,
            cross_workspace,
            denied,
            owner_after,
        ),
        logs,
    ) = app
        .capture_audit_logs(async |request_id| {
            let manager =
                McpClient::connect_with_request_id(app.mcp_server(), &owner_token, request_id)
                    .await;
            let reader =
                McpClient::connect_with_request_id(app.mcp_server(), &read_only_token, request_id)
                    .await;

            let invalid_email = manager
                .call_tool_error(
                    "create_auditor_access_link",
                    create_arguments("not-an-email"),
                )
                .await;
            let malformed_expiry = manager
                .call_tool_error(
                    "create_auditor_access_link",
                    json!({
                        "email": "auditor@example.com",
                        "expires_at": "tomorrow",
                        "period_start": PERIOD_START,
                        "period_end": PERIOD_END,
                    }),
                )
                .await;
            let missing_period = manager
                .call_tool_error(
                    "create_auditor_access_link",
                    json!({ "email": "auditor@example.com" }),
                )
                .await;
            let inverted_period = manager
                .call_tool_error(
                    "create_auditor_access_link",
                    json!({
                        "email": "auditor@example.com",
                        "period_start": PERIOD_END,
                        "period_end": PERIOD_START,
                    }),
                )
                .await;
            let malformed_id = manager
                .call_tool_error(
                    "revoke_auditor_access_link",
                    json!({ "grant_id": "not-a-uuid" }),
                )
                .await;
            let cross_workspace = manager
                .call_tool_error(
                    "revoke_auditor_access_link",
                    json!({ "grant_id": foreign_grant_id }),
                )
                .await;
            let denied = reader
                .call_tool_error(
                    "create_auditor_access_link",
                    create_arguments("denied@example.com"),
                )
                .await;
            let owner_after = manager
                .call_tool("list_auditor_access_links", json!({}))
                .await;

            (
                invalid_email,
                malformed_expiry,
                missing_period,
                inverted_period,
                malformed_id,
                cross_workspace,
                denied,
                owner_after,
            )
        })
        .await;

    assert_validation_error(
        &invalid_email,
        json!([{"field": "email", "message": "auditor_email is invalid"}]),
    );
    assert_validation_error(
        &malformed_expiry,
        json!([{
            "field": "expires_at",
            "message": "must be an RFC 3339 timestamp"
        }]),
    );
    assert_validation_error(
        &missing_period,
        json!([
            {"field": "period_start", "message": "is required"},
            {"field": "period_end", "message": "is required"},
        ]),
    );
    assert_validation_error(
        &inverted_period,
        json!([{
            "field": "period_end",
            "message": "period_end must be greater than or equal to period_start"
        }]),
    );
    assert_validation_error(
        &malformed_id,
        json!([{"field": "grant_id", "message": "must be a UUID"}]),
    );
    assert_not_found(&cross_workspace);
    assert_not_found(&denied);
    assert!(logs.is_empty());
    assert_eq!(owner_after, owner_before);
}

fn create_arguments(email: &str) -> Value {
    json!({
        "email": email,
        "expires_at": EXPIRES_AT,
        "period_start": PERIOD_START,
        "period_end": PERIOD_END,
    })
}

#[track_caller]
fn assert_create_response(response: &Value, workspace_id: Uuid, email: &str) -> (Uuid, String) {
    assert_eq!(
        object_keys(response),
        ["grant", "intended_use", "url", "url_secret_type"]
            .into_iter()
            .collect()
    );
    let grant_id = Uuid::parse_str(
        response["grant"]["id"]
            .as_str()
            .expect("grant id is a string"),
    )
    .expect("grant id is a UUID");
    assert_grant_projection(&response["grant"], grant_id, email, None);
    assert_eq!(response["url_secret_type"], "bearer_secret");
    assert_eq!(response["intended_use"], "auditor_browser_access");

    let url = Url::parse(response["url"].as_str().expect("auditor URL is a string"))
        .expect("auditor URL parses");
    assert_eq!(url.scheme(), "https");
    assert_eq!(url.host_str(), Some("api.proofplane.test"));
    assert_eq!(url.path(), format!("/auditor-access/{workspace_id}"));
    let query = url
        .query_pairs()
        .map(|(name, value)| (name.into_owned(), value.into_owned()))
        .collect::<Vec<_>>();
    let [(name, token)] = query.as_slice() else {
        panic!("auditor URL has exactly one query parameter: {query:?}");
    };
    assert_eq!(name, "token");
    assert_eq!(token.len(), TOKEN_LENGTH);
    assert!(token.starts_with(PREFIX));
    assert!(token[PREFIX.len()..]
        .bytes()
        .all(|byte| ALPHABET.contains(&byte)));

    (grant_id, token.clone())
}

#[track_caller]
fn assert_grant_projection(grant: &Value, grant_id: Uuid, email: &str, revoked_at: Option<&Value>) {
    assert_eq!(
        object_keys(grant),
        [
            "auditor_email",
            "created_at",
            "expires_at",
            "id",
            "period_end",
            "period_start",
            "revoked_at",
        ]
        .into_iter()
        .collect()
    );
    assert_eq!(grant["id"], grant_id.to_string());
    assert_eq!(grant["auditor_email"], email);
    assert_rfc3339(&grant["created_at"]);
    assert_eq!(grant["expires_at"], EXPIRES_AT);
    assert_eq!(grant["period_start"], PERIOD_START);
    assert_eq!(grant["period_end"], PERIOD_END);
    assert_eq!(
        grant["revoked_at"],
        revoked_at.cloned().unwrap_or(Value::Null)
    );
}

#[allow(clippy::too_many_arguments)]
#[track_caller]
fn assert_mcp_auditor_grant_audit_event(
    record: &Value,
    event_name: &str,
    operation: &str,
    user_id: Uuid,
    connection_id: Uuid,
    workspace_id: Uuid,
    grant_id: Uuid,
    email: &str,
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
    assert_eq!(fields["object_type"], "auditor_access_grant");
    assert_eq!(fields["object_id"], grant_id.to_string());
    assert_eq!(
        serde_json::from_str::<Value>(
            fields["metadata"]
                .as_str()
                .expect("audit metadata is serialized JSON"),
        )
        .expect("audit metadata parses"),
        json!({
            "auditor_email": email,
            "expires_at": EXPIRES_AT,
            "period_start": PERIOD_START,
            "period_end": PERIOD_END,
        })
    );
}
