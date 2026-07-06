use std::sync::Arc;

use axum::http::{header, StatusCode};
use axum_test::TestServer;
use chrono::{Duration, Utc};
use proofplane::{
    domain::{AgentConnectionStatus, UserId, WorkspaceId, WorkspacePermission},
    repository::{Error as RepositoryError, Postgres},
    routes::internal_agent_connections::{self, InternalAgentConnectionsState},
    services::agent_connections::{
        AgentConnectionError, AgentConnectionService, CreatePendingConnection,
    },
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
async fn pending_continuation_is_single_use_and_active_reuse_is_exact() {
    let (app, service, user_id, workspace_id) = fixture().await;
    let mut wrong_subject = pending_request(
        user_id,
        workspace_id,
        "wrong-subject",
        "wrong-subject-nonce",
    );
    wrong_subject.auth0_subject = "auth0|someone-else".to_owned();
    assert!(matches!(
        service.create_pending(wrong_subject).await,
        Err(AgentConnectionError::Invalid(_))
    ));
    let pending = service
        .create_pending(pending_request(
            user_id,
            workspace_id,
            "continue-1",
            "nonce-1",
        ))
        .await
        .expect("pending connection creates");
    assert_eq!(pending.status, AgentConnectionStatus::Pending);
    assert!(service
        .activate(pending.id)
        .await
        .expect("unconsumed activation resolves")
        .is_none());

    let consumed = service
        .consume_continuation("continue-1", "nonce-1")
        .await
        .expect("continuation consumption succeeds")
        .expect("continuation is approved");
    assert_eq!(consumed.id, pending.id);
    assert!(service
        .consume_continuation("continue-1", "nonce-1")
        .await
        .expect("replay resolves")
        .is_none());

    let active = service
        .activate(pending.id)
        .await
        .expect("activation succeeds")
        .expect("pending connection activates");
    assert_eq!(active.status, AgentConnectionStatus::Active);
    assert!(service
        .find_reusable(SUBJECT, CLIENT_ID, RESOURCE, canonical_test_permissions(),)
        .await
        .expect("reuse lookup succeeds")
        .is_some());
    assert!(service
        .find_reusable(
            SUBJECT,
            CLIENT_ID,
            RESOURCE,
            vec![WorkspacePermission::ReadControls],
        )
        .await
        .expect("scope mismatch resolves")
        .is_none());

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
    assert!(service
        .find_reusable(SUBJECT, CLIENT_ID, RESOURCE, canonical_test_permissions(),)
        .await
        .expect("membership-less lookup resolves")
        .is_none());
}

#[tokio::test]
async fn expired_pending_is_replaced_and_live_concurrent_creation_has_one_winner() {
    let (app, service, user_id, workspace_id) = fixture().await;
    let first = service
        .create_pending(pending_request(
            user_id,
            workspace_id,
            "expired",
            "expired-nonce",
        ))
        .await
        .expect("initial pending creates");
    let client = app
        .postgres()
        .get()
        .await
        .expect("database connection opens");
    client
        .execute(
            r#"
UPDATE agent_authorization_transactions
SET created_at = now() - interval '2 hours',
    expires_at = now() - interval '1 hour'
WHERE agent_connection_id = $1
"#,
            &[&Uuid::from(first.id)],
        )
        .await
        .expect("transaction expires");
    client
        .execute(
            r#"
UPDATE agent_connections
SET created_at = now() - interval '2 hours',
    pending_expires_at = now() - interval '1 hour'
WHERE id = $1
"#,
            &[&Uuid::from(first.id)],
        )
        .await
        .expect("pending expires");

    let replacement = service
        .create_pending(pending_request(
            user_id,
            workspace_id,
            "replacement",
            "replacement-nonce",
        ))
        .await
        .expect("expired pending is replaced");
    assert_ne!(replacement.id, first.id);

    service
        .revoke(replacement.id)
        .await
        .expect("replacement revokes");
    let left = service.clone();
    let right = service.clone();
    let left_request = pending_request(user_id, workspace_id, "left", "left-nonce");
    let right_request = pending_request(user_id, workspace_id, "right", "right-nonce");
    let (left, right) = tokio::join!(
        left.create_pending(left_request),
        right.create_pending(right_request)
    );
    let results = [left, right];
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|result| matches!(result, Err(AgentConnectionError::AlreadyExists)))
            .count(),
        1
    );
}

#[tokio::test]
async fn denial_and_revocation_prevent_continuation_and_reuse() {
    let (_app, service, user_id, workspace_id) = fixture().await;
    let denied = service
        .create_pending(pending_request(
            user_id,
            workspace_id,
            "denied",
            "denied-nonce",
        ))
        .await
        .expect("denied pending creates");
    assert!(service
        .deny_pending("denied")
        .await
        .expect("denial succeeds"));
    assert!(service
        .consume_continuation("denied", "denied-nonce")
        .await
        .expect("denied continuation resolves")
        .is_none());
    assert!(service
        .activate(denied.id)
        .await
        .expect("denied activation resolves")
        .is_none());

    let pending = service
        .create_pending(pending_request(
            user_id,
            workspace_id,
            "revoked",
            "revoked-nonce",
        ))
        .await
        .expect("new pending creates");
    service
        .consume_continuation("revoked", "revoked-nonce")
        .await
        .expect("continuation consumes");
    service
        .activate(pending.id)
        .await
        .expect("connection activates");
    assert!(service
        .revoke(pending.id)
        .await
        .expect("connection revokes"));
    assert!(service
        .find_reusable(SUBJECT, CLIENT_ID, RESOURCE, canonical_test_permissions(),)
        .await
        .expect("revoked lookup resolves")
        .is_none());
}

#[tokio::test]
async fn migration_enforces_digests_permissions_lifecycle_and_live_tuple() {
    let (app, service, user_id, workspace_id) = fixture().await;
    let pending = service
        .create_pending(pending_request(
            user_id,
            workspace_id,
            "stored-token",
            "stored-nonce",
        ))
        .await
        .expect("pending connection creates");
    let client = app
        .postgres()
        .get()
        .await
        .expect("database connection opens");

    let row = client
        .query_one(
            r#"
SELECT continuation_digest, nonce_digest
FROM agent_authorization_transactions
WHERE agent_connection_id = $1
"#,
            &[&Uuid::from(pending.id)],
        )
        .await
        .expect("transaction loads");
    let continuation: Vec<u8> = row.get("continuation_digest");
    let nonce: Vec<u8> = row.get("nonce_digest");
    assert_eq!(continuation.len(), 32);
    assert_eq!(nonce.len(), 32);
    assert_ne!(continuation, b"stored-token");
    assert_ne!(nonce, b"stored-nonce");

    assert!(client
        .execute(
            r#"
INSERT INTO agent_connection_permissions (agent_connection_id, permission)
VALUES ($1, 'delete_everything')
"#,
            &[&Uuid::from(pending.id)],
        )
        .await
        .is_err());
    assert!(client
        .execute(
            r#"
UPDATE agent_connections
SET status = 'active', activated_at = NULL
WHERE id = $1
"#,
            &[&Uuid::from(pending.id)],
        )
        .await
        .is_err());

    let duplicate = service
        .create_pending(pending_request(
            user_id,
            workspace_id,
            "duplicate",
            "duplicate-nonce",
        ))
        .await
        .expect_err("live tuple is unique");
    assert!(matches!(duplicate, AgentConnectionError::AlreadyExists));

    let repository_error = app
        .postgres()
        .create_pending_agent_connection(&proofplane::domain::CreatePendingAgentConnection {
            id: Uuid::new_v4().into(),
            transaction_id: Uuid::new_v4().into(),
            user_id: Uuid::new_v4().into(),
            workspace_id: workspace_id.into(),
            auth0_subject: "auth0|missing-user".to_owned(),
            auth0_client_id: "other-client".to_owned(),
            client_display_name: "Other".to_owned(),
            resource: RESOURCE.to_owned(),
            permissions: vec![WorkspacePermission::ReadControls],
            pending_expires_at: Utc::now() + Duration::minutes(5),
            continuation_digest: proofplane::services::agent_connections::digest_secret("a"),
            nonce_digest: proofplane::services::agent_connections::digest_secret("b"),
        })
        .await
        .expect_err("foreign key is enforced");
    assert!(matches!(repository_error, RepositoryError::Database(_)));
}

#[tokio::test]
async fn internal_routes_authenticate_validate_and_return_tagged_outcomes() {
    let (app, service, user_id, workspace_id) = fixture().await;
    let server = app.server();
    let resolve_path = "/internal/auth0-actions/agent-connections/resolve";
    let continuation_path = "/internal/auth0-actions/agent-connections/continuations/consume";

    server
        .post(resolve_path)
        .json(&valid_resolve_body())
        .await
        .assert_status_unauthorized();
    server
        .post(resolve_path)
        .add_header(header::AUTHORIZATION, "Bearer wrong")
        .json(&valid_resolve_body())
        .await
        .assert_status_unauthorized();
    server
        .post(resolve_path)
        .add_header(header::AUTHORIZATION, format!("Bearer {ACTION_SECRET}"))
        .json(&json!({"subject": ""}))
        .await
        .assert_status_bad_request();

    let response = server
        .post(resolve_path)
        .add_header(header::AUTHORIZATION, format!("Bearer {ACTION_SECRET}"))
        .json(&valid_resolve_body())
        .await;
    response.assert_status_ok();
    assert_eq!(response.json::<Value>()["outcome"], "interaction_required");

    let pending = service
        .create_pending(pending_request(
            user_id,
            workspace_id,
            "route-token",
            "route-nonce",
        ))
        .await
        .expect("route pending creates");
    let response = server
        .post(continuation_path)
        .add_header(header::AUTHORIZATION, format!("Bearer {ACTION_SECRET}"))
        .json(&json!({
            "continuation_token": "route-token",
            "nonce": "route-nonce"
        }))
        .await;
    response.assert_status_ok();
    let body = response.json::<Value>();
    assert_eq!(body["outcome"], "approved");
    assert_eq!(body["connection_id"], pending.id.to_string());

    let replay = server
        .post(continuation_path)
        .add_header(header::AUTHORIZATION, format!("Bearer {ACTION_SECRET}"))
        .json(&json!({
            "continuation_token": "route-token",
            "nonce": "route-nonce"
        }))
        .await;
    replay.assert_status_ok();
    assert_eq!(replay.json::<Value>()["outcome"], "invalid_continuation");

    service
        .activate(pending.id)
        .await
        .expect("route connection activates");
    let reusable = server
        .post(resolve_path)
        .add_header(header::AUTHORIZATION, format!("Bearer {ACTION_SECRET}"))
        .json(&valid_resolve_body())
        .await;
    reusable.assert_status_ok();
    assert_eq!(reusable.json::<Value>()["outcome"], "reusable");
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

    let response = server
        .post("/internal/auth0-actions/agent-connections/resolve")
        .add_header(header::AUTHORIZATION, format!("Bearer {ACTION_SECRET}"))
        .json(&valid_resolve_body())
        .await;
    response.assert_status(StatusCode::INTERNAL_SERVER_ERROR);
}

async fn fixture() -> (TestApp, AgentConnectionService, Uuid, Uuid) {
    let app = TestApp::start_without_default_auth().await;
    let user_id = app.login(SUBJECT).await;
    let workspace = app.create_workspace_as(SUBJECT, "Agent Workspace").await;
    let workspace_id = Uuid::parse_str(workspace["id"].as_str().expect("workspace id is returned"))
        .expect("workspace id is a UUID");
    let service = AgentConnectionService::new(app.postgres_arc());
    (app, service, user_id, workspace_id)
}

fn pending_request(
    user_id: Uuid,
    workspace_id: Uuid,
    continuation_token: &str,
    nonce: &str,
) -> CreatePendingConnection {
    CreatePendingConnection {
        user_id: UserId::from(user_id),
        workspace_id: WorkspaceId::from(workspace_id),
        auth0_subject: SUBJECT.to_owned(),
        auth0_client_id: CLIENT_ID.to_owned(),
        client_display_name: "Integration MCP Client".to_owned(),
        resource: RESOURCE.to_owned(),
        permissions: canonical_test_permissions(),
        expires_at: Utc::now() + Duration::minutes(5),
        continuation_token: continuation_token.to_owned(),
        nonce: nonce.to_owned(),
    }
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
