use http::StatusCode;
use proofplane::{
    domain::WorkspacePermission,
    routes::{authentication::AUTHORIZATION_HEADER, request_context::REQUEST_ID_HEADER},
};
use serde_json::Value;

use crate::support::{
    auth::assert_unauthorized,
    harness,
    oauth::{authorize_agent_connection, consent_agent_connection},
    scenario::ScenarioBuilder,
};

#[tokio::test]
async fn connection_routes_require_authentication() {
    let app = harness::app().await;

    let list = app.app_server().get("/agent-connections").await;
    assert_unauthorized(&list.json(), list.status_code());

    let revoke = app
        .app_server()
        .delete("/agent-connections/00000000-0000-0000-0000-000000000000")
        .await;
    assert_unauthorized(&revoke.json(), revoke.status_code());
}

#[tokio::test]
async fn listing_returns_only_owned_live_connections_in_authorization_order() {
    let app = harness::app().await;
    let subject = "auth0|connection-list-owner";

    ScenarioBuilder::new(&app)
        .with_user(subject)
        .with_workspace(subject, "List Owner")
        .with_user("auth0|connection-list-foreign")
        .with_workspace("auth0|connection-list-foreign", "Foreign Owner")
        .build()
        .await;

    consent_agent_connection(&app, subject, "Claude", &WorkspacePermission::ALL).await;
    authorize_agent_connection(&app, subject, "Codex", &WorkspacePermission::ALL).await;
    authorize_agent_connection(
        &app,
        "auth0|connection-list-foreign",
        "Codex",
        &WorkspacePermission::ALL,
    )
    .await;

    let response = app
        .app_server()
        .get("/agent-connections")
        .add_header(AUTHORIZATION_HEADER, format!("Bearer {subject}"))
        .await;
    response.assert_status_ok();

    let body = response.json::<Value>();
    assert_eq!(body["mcp_url"], "https://mcp.proofplane.test/mcp");

    let connections = body["connections"]
        .as_array()
        .expect("connections is an array");

    assert_eq!(connections.len(), 2);
    assert_eq!(connections[0]["client_name"], "Codex");
    assert_eq!(connections[0]["status"], "active");
    assert!(connections[0]["authorized_at"].is_string());
    assert!(connections[0]["last_used_at"].is_string());
    assert_eq!(connections[1]["client_name"], "Claude");
    assert_eq!(connections[1]["status"], "authorized");
    assert_eq!(connections[1]["last_used_at"], Value::Null);

    // The complete key set. Anything the response should not expose — the
    // workspace, the scopes, the resource, the Auth0 identifiers — cannot appear
    // without failing here, so there is nothing to enumerate separately.
    for connection in connections {
        let fields = connection
            .as_object()
            .expect("connection is an object")
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>();
        assert_eq!(
            fields,
            [
                "authorized_at",
                "client_name",
                "id",
                "last_used_at",
                "status"
            ]
        );
    }
}

#[tokio::test]
async fn revocation_is_user_scoped_audited_and_repeat_or_foreign_requests_are_not_found() {
    let app = harness::app().await;
    let subject = "auth0|connection-revoke-owner";

    let test_scenario = ScenarioBuilder::new(&app)
        .with_user(subject)
        .with_workspace(subject, "Revoke Owner")
        .build()
        .await;

    let user_id = test_scenario.user(subject).id;

    authorize_agent_connection(&app, subject, "Claude", &WorkspacePermission::ALL).await;

    let response = app
        .app_server()
        .get("/agent-connections")
        .add_header(AUTHORIZATION_HEADER, format!("Bearer {subject}"))
        .await;
    response.assert_status_ok();

    let body = response.json::<Value>();
    let connections = body["connections"]
        .as_array()
        .expect("connections is an array");

    let connection_id = connections[0]["id"]
        .as_str()
        .expect("connection id can be converted to a string");

    let (response, logs) = app
        .capture_audit_logs(async |request_id| {
            app.app_server()
                .delete(&format!("/agent-connections/{connection_id}"))
                .add_header(AUTHORIZATION_HEADER, format!("Bearer {subject}"))
                .add_header(REQUEST_ID_HEADER, request_id.to_string())
                .await
        })
        .await;
    response.assert_status(StatusCode::NO_CONTENT);
    assert_eq!(logs.len(), 1);
    let fields = &logs[0]["fields"];
    assert_eq!(fields["event_name"], "agent_connection.revoked");
    assert_eq!(fields["actor_type"], "user");
    assert_eq!(fields["user_id"], user_id.to_string());
    assert_eq!(fields["operation"], "revoke_agent_connection");
    assert_eq!(fields["object_type"], "agent_connection");
    assert_eq!(fields["object_id"], connection_id.to_string());

    let repeat = app
        .app_server()
        .delete(&format!("/agent-connections/{connection_id}"))
        .add_header(AUTHORIZATION_HEADER, format!("Bearer {subject}"))
        .await;
    repeat.assert_status_not_found();

    let list = app
        .app_server()
        .get("/agent-connections")
        .add_header(AUTHORIZATION_HEADER, format!("Bearer {subject}"))
        .await;
    list.assert_status_ok();
    assert!(list.json::<Value>()["connections"]
        .as_array()
        .expect("connections is an array")
        .is_empty());
}
