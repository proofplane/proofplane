use std::sync::Arc;

use axum::http::{header, StatusCode};
use axum_test::TestServer;
use chrono::{Duration, Utc};
use proofplane::{
    domain::{
        AgentAuthorizationTransactionId, AgentConnection, AgentConnectionId,
        NewPendingAgentConnection, UserId, WorkspaceId, WorkspacePermission,
    },
    repository::Postgres,
    routes::internal_agent_connections::{self, InternalAgentConnectionsState},
    services::agent_connections::{digest_secret, AgentConnectionService},
    store,
};
use secrecy::SecretString;
use serde_json::{json, Value};
use uuid::Uuid;

use super::support::TestApp;

const ACTION_SECRET: &str = "integration-action-shared-secret-001";
const SUBJECT: &str = "auth0|agent-connection-user";
const CLIENT_ID: &str = "integration-mcp-client";
const RESOURCE: &str = "https://mcp.proofplane.test/mcp";

#[tokio::test]
async fn internal_routes_authenticate_validate_and_return_tagged_outcomes() {
    let (app, user_id, workspace_id) = fixture().await;
    let server = app.server();
    let resolve_path = "/internal/auth0-actions/agent-connections/resolve";
    let continuation_path = "/internal/auth0-actions/agent-connections/continuations/consume";

    server
        .post(resolve_path)
        .json(&json!({"subject": ""}))
        .await
        .assert_status_unauthorized();
    server
        .post(resolve_path)
        .add_header(header::AUTHORIZATION, "Bearer wrong")
        .json(&valid_resolve_body())
        .await
        .assert_status_unauthorized();

    let malformed = server
        .post(resolve_path)
        .add_header(header::AUTHORIZATION, format!("Bearer {ACTION_SECRET}"))
        .json(&json!({
            "subject": "",
            "client_id": " ",
            "resource": "not-a-url",
            "scopes": []
        }))
        .await;
    malformed.assert_status_bad_request();
    assert_eq!(
        malformed.json::<Value>()["error"]["details"],
        json!([
            "subject must not be blank",
            "client_id must not be blank",
            "resource must be an absolute URL",
            "scopes must not be empty"
        ])
    );

    let missing = authorized_post(server, resolve_path, valid_resolve_body()).await;
    missing.assert_status_ok();
    assert_eq!(missing.json::<Value>()["outcome"], "interaction_required");

    let pending = seed_pending(&app, user_id, workspace_id, "route-token", "route-nonce").await;
    let wrong_nonce = authorized_post(
        server,
        continuation_path,
        json!({
            "continuation_token": "route-token",
            "nonce": "wrong-route-nonce"
        }),
    )
    .await;
    wrong_nonce.assert_status_ok();
    assert_eq!(
        wrong_nonce.json::<Value>()["outcome"],
        "invalid_continuation"
    );

    let approved = authorized_post(
        server,
        continuation_path,
        json!({
            "continuation_token": "route-token",
            "nonce": "route-nonce"
        }),
    )
    .await;
    approved.assert_status_ok();
    let body = approved.json::<Value>();
    assert_eq!(body["outcome"], "approved");
    assert_eq!(body["connection_id"], pending.id.to_string());

    let replay = authorized_post(
        server,
        continuation_path,
        json!({
            "continuation_token": "route-token",
            "nonce": "route-nonce"
        }),
    )
    .await;
    replay.assert_status_ok();
    assert_eq!(replay.json::<Value>()["outcome"], "invalid_continuation");

    activate_seeded_connection(&app, pending.id).await;

    let scope_mismatch = authorized_post(
        server,
        resolve_path,
        json!({
            "subject": SUBJECT,
            "client_id": CLIENT_ID,
            "resource": RESOURCE,
            "scopes": ["read_controls"]
        }),
    )
    .await;
    scope_mismatch.assert_status_ok();
    assert_eq!(
        scope_mismatch.json::<Value>()["outcome"],
        "interaction_required"
    );

    let reusable = authorized_post(server, resolve_path, valid_resolve_body()).await;
    reusable.assert_status_ok();
    assert_eq!(reusable.json::<Value>()["outcome"], "reusable");

    remove_membership(&app, user_id, workspace_id).await;

    let membership_lost = authorized_post(server, resolve_path, valid_resolve_body()).await;
    membership_lost.assert_status_ok();
    assert_eq!(
        membership_lost.json::<Value>()["outcome"],
        "interaction_required"
    );
}

#[tokio::test]
async fn internal_route_repository_failure_returns_500() {
    let pool = store::conn_pool(
        "postgres://postgres:postgres@127.0.0.1:1/postgres?connect_timeout=1",
        1,
    )
    .await
    .expect("disconnected pool builds");
    let state = InternalAgentConnectionsState {
        service: AgentConnectionService::new(Arc::new(Postgres::new(pool))),
        action_shared_secret: SecretString::from(ACTION_SECRET),
    };
    let server = TestServer::new(internal_agent_connections::router(state));

    let response = authorized_post(
        &server,
        "/internal/auth0-actions/agent-connections/resolve",
        valid_resolve_body(),
    )
    .await;
    response.assert_status(StatusCode::INTERNAL_SERVER_ERROR);
}

async fn fixture() -> (TestApp, Uuid, Uuid) {
    let app = TestApp::start_without_default_auth().await;
    let user_id = app.login(SUBJECT).await;
    let workspace = app.create_workspace_as(SUBJECT, "Agent Workspace").await;
    let workspace_id = Uuid::parse_str(workspace["id"].as_str().expect("workspace id is returned"))
        .expect("workspace id is a UUID");
    (app, user_id, workspace_id)
}

async fn seed_pending(
    app: &TestApp,
    user_id: Uuid,
    workspace_id: Uuid,
    continuation_token: &str,
    nonce: &str,
) -> AgentConnection {
    app.postgres()
        .create_pending_agent_connection(&NewPendingAgentConnection {
            id: AgentConnectionId::from(Uuid::new_v4()),
            transaction_id: AgentAuthorizationTransactionId::from(Uuid::new_v4()),
            user_id: UserId::from(user_id),
            workspace_id: WorkspaceId::from(workspace_id),
            auth0_subject: SUBJECT.to_owned(),
            auth0_client_id: CLIENT_ID.to_owned(),
            client_display_name: "Integration MCP Client".to_owned(),
            resource: RESOURCE.to_owned(),
            permissions: canonical_test_permissions(),
            pending_expires_at: Utc::now() + Duration::minutes(5),
            continuation_digest: digest_secret(continuation_token),
            nonce_digest: digest_secret(nonce),
        })
        .await
        .expect("pending connection seeds")
}

async fn activate_seeded_connection(app: &TestApp, id: AgentConnectionId) {
    app.postgres()
        .activate_agent_connection(id)
        .await
        .expect("repository activation succeeds")
        .expect("consumed pending connection activates");
}

async fn remove_membership(app: &TestApp, user_id: Uuid, workspace_id: Uuid) {
    app.postgres()
        .get()
        .await
        .expect("database connection opens")
        .execute(
            "DELETE FROM workspace_memberships WHERE workspace_id = $1 AND user_id = $2",
            &[&workspace_id, &user_id],
        )
        .await
        .expect("membership removes");
}

async fn authorized_post(server: &TestServer, path: &str, body: Value) -> axum_test::TestResponse {
    server
        .post(path)
        .add_header(header::AUTHORIZATION, format!("Bearer {ACTION_SECRET}"))
        .json(&body)
        .await
}

fn canonical_test_permissions() -> Vec<WorkspacePermission> {
    vec![
        WorkspacePermission::ReadEvidenceRequests,
        WorkspacePermission::WriteControls,
    ]
}

fn valid_resolve_body() -> Value {
    json!({
        "subject": SUBJECT,
        "client_id": CLIENT_ID,
        "resource": RESOURCE,
        "scopes": ["write_controls", "read_evidence_requests"]
    })
}
