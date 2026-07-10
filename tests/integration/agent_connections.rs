use axum::http::StatusCode;
use chrono::{Duration, Utc};
use proofplane::{
    domain::{
        AgentAuthorizationTransactionId, AgentConnectionId, NewPendingAgentConnection, UserId,
        WorkspaceId, WorkspacePermission,
    },
    routes::{authentication::AUTHORIZATION_HEADER, request_context::REQUEST_ID_HEADER},
    services::agent_connections::digest_secret,
};
use serde_json::Value;
use uuid::Uuid;

use super::support::{capture_audit_logs, TestApp};

#[tokio::test]
async fn listing_returns_only_owned_live_connections_in_authorization_order() {
    let app = TestApp::start_without_default_auth().await;
    let subject = "auth0|connection-list-owner";
    let (user_id, workspace_id) = user_workspace(&app, subject, "List Owner").await;
    let older = seed_connection(&app, user_id, workspace_id, "Claude", "older").await;
    let newer = seed_connection(&app, user_id, workspace_id, "Codex", "newer").await;
    let (foreign_user_id, foreign_workspace_id) =
        user_workspace(&app, "auth0|connection-list-foreign", "Foreign Owner").await;
    let foreign = seed_connection(
        &app,
        foreign_user_id,
        foreign_workspace_id,
        "Foreign client",
        "foreign",
    )
    .await;

    let db = app.postgres().get().await.expect("database opens");
    db.execute(
        "UPDATE agent_authorization_transactions SET created_at = now() - interval '3 hours', consumed_at = now() - interval '2 hours' WHERE agent_connection_id = $1",
        &[&older],
    )
    .await
    .expect("older authorization time updates");
    db.execute(
        "UPDATE agent_connections SET status = 'active', activated_at = now(), last_used_at = now() WHERE id = $1",
        &[&newer],
    )
    .await
    .expect("newer connection activates");
    db.execute(
        "UPDATE agent_connections SET status = 'revoked', revoked_at = now() WHERE id = $1",
        &[&foreign],
    )
    .await
    .expect("foreign connection revokes");

    let response = app
        .server()
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
    assert_eq!(connections[0]["id"], newer.to_string());
    assert_eq!(connections[0]["client_name"], "Codex");
    assert_eq!(connections[0]["status"], "active");
    assert!(connections[0]["authorized_at"].is_string());
    assert!(connections[0]["last_used_at"].is_string());
    assert_eq!(connections[1]["id"], older.to_string());
    assert_eq!(connections[1]["status"], "authorized");
    assert_eq!(connections[1]["last_used_at"], Value::Null);

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
        for hidden in [
            "workspace_id",
            "scopes",
            "permissions",
            "resource",
            "auth0_subject",
            "auth0_client_id",
            "user_id",
        ] {
            assert!(connection.get(hidden).is_none(), "revealed {hidden}");
        }
    }
}

#[tokio::test]
async fn revocation_is_user_scoped_audited_and_repeat_or_foreign_requests_are_not_found() {
    let app = TestApp::start_without_default_auth().await;
    let subject = "auth0|connection-revoke-owner";
    let (user_id, workspace_id) = user_workspace(&app, subject, "Revoke Owner").await;
    let owned = seed_connection(&app, user_id, workspace_id, "Claude", "owned").await;
    let (foreign_user_id, foreign_workspace_id) =
        user_workspace(&app, "auth0|connection-revoke-foreign", "Foreign Revoke").await;
    let foreign = seed_connection(
        &app,
        foreign_user_id,
        foreign_workspace_id,
        "Codex",
        "foreign-revoke",
    )
    .await;

    let (response, logs) = capture_audit_logs(|request_id| {
        let app = &app;
        async move {
            app.server()
                .delete(&format!("/agent-connections/{owned}"))
                .add_header(AUTHORIZATION_HEADER, format!("Bearer {subject}"))
                .add_header(REQUEST_ID_HEADER, request_id.to_string())
                .await
        }
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
    assert_eq!(fields["object_id"], owned.to_string());

    let repeat = app
        .server()
        .delete(&format!("/agent-connections/{owned}"))
        .add_header(AUTHORIZATION_HEADER, format!("Bearer {subject}"))
        .await;
    repeat.assert_status_not_found();
    let foreign_attempt = app
        .server()
        .delete(&format!("/agent-connections/{foreign}"))
        .add_header(AUTHORIZATION_HEADER, format!("Bearer {subject}"))
        .await;
    foreign_attempt.assert_status_not_found();

    let db = app.postgres().get().await.expect("database opens");
    let owned_status: String = db
        .query_one(
            "SELECT status FROM agent_connections WHERE id = $1",
            &[&owned],
        )
        .await
        .expect("owned connection loads")
        .get("status");
    let foreign_status: String = db
        .query_one(
            "SELECT status FROM agent_connections WHERE id = $1",
            &[&foreign],
        )
        .await
        .expect("foreign connection loads")
        .get("status");
    assert_eq!(owned_status, "revoked");
    assert_eq!(foreign_status, "authorized");

    let list = app
        .server()
        .get("/agent-connections")
        .add_header(AUTHORIZATION_HEADER, format!("Bearer {subject}"))
        .await;
    list.assert_status_ok();
    assert!(list.json::<Value>()["connections"]
        .as_array()
        .expect("connections is an array")
        .is_empty());
}

#[tokio::test]
async fn connection_routes_require_authentication() {
    let app = TestApp::start_without_default_auth().await;

    app.server()
        .get("/agent-connections")
        .await
        .assert_status_unauthorized();
    app.server()
        .delete(&format!("/agent-connections/{}", Uuid::new_v4()))
        .await
        .assert_status_unauthorized();
}

async fn user_workspace(app: &TestApp, subject: &str, name: &str) -> (Uuid, Uuid) {
    let user_id = app.login(subject).await;
    let workspace = app.create_workspace_as(subject, name).await;
    let workspace_id = Uuid::parse_str(workspace["id"].as_str().expect("workspace id is a string"))
        .expect("workspace id is a UUID");
    (user_id, workspace_id)
}

async fn seed_connection(
    app: &TestApp,
    user_id: Uuid,
    workspace_id: Uuid,
    client_name: &str,
    suffix: &str,
) -> Uuid {
    let id = Uuid::new_v4();
    let continuation = format!("continuation-{suffix}-{id}");
    let nonce = format!("nonce-{suffix}-{id}");
    let user = app
        .postgres()
        .get_user(UserId::from(user_id))
        .await
        .expect("user lookup succeeds")
        .expect("user exists");
    app.postgres()
        .create_pending_agent_connection(&NewPendingAgentConnection {
            id: AgentConnectionId::from(id),
            transaction_id: AgentAuthorizationTransactionId::from(Uuid::new_v4()),
            user_id: UserId::from(user_id),
            workspace_id: WorkspaceId::from(workspace_id),
            auth0_subject: user.auth0_sub,
            auth0_client_id: format!("client-{suffix}-{id}"),
            client_display_name: client_name.to_owned(),
            resource: format!("https://mcp.proofplane.test/{suffix}/{id}"),
            permissions: WorkspacePermission::ALL.to_vec(),
            pending_expires_at: Utc::now() + Duration::minutes(10),
            continuation_digest: digest_secret(&continuation),
            nonce_digest: digest_secret(&nonce),
        })
        .await
        .expect("pending connection creates");
    app.postgres()
        .consume_agent_connection_continuation(digest_secret(&continuation), digest_secret(&nonce))
        .await
        .expect("continuation consumption resolves")
        .expect("connection authorizes");
    id
}
