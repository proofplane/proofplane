use chrono::{Duration, Utc};
use proofplane::{
    domain::{
        AgentAuthorizationTransactionId, AgentConnectionId, AgentConnectionStatus,
        NewPendingAgentConnection, UserId, WorkspaceId, WorkspacePermission,
    },
    repository::{ConflictKind, Error as RepositoryError},
    services::agent_connections::digest_secret,
};
use uuid::Uuid;

use super::support::TestApp;

const SUBJECT: &str = "auth0|agent-connection-repository-user";
const CLIENT_ID: &str = "integration-repository-mcp-client";
const RESOURCE: &str = "https://mcp.proofplane.test/repository";

#[tokio::test]
async fn pending_creation_stores_digests_and_canonical_permissions() {
    let (app, user_id, workspace_id) = fixture().await;
    let pending = new_pending(user_id, workspace_id, "stored-token", "stored-nonce");

    let created = app
        .postgres()
        .create_pending_agent_connection(&pending)
        .await
        .expect("pending connection creates");

    assert_eq!(created.status, AgentConnectionStatus::Pending);
    assert_eq!(created.permissions, canonical_test_permissions());

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
            &[&Uuid::from(created.id)],
        )
        .await
        .expect("transaction loads");
    let continuation: Vec<u8> = row.get("continuation_digest");
    let nonce: Vec<u8> = row.get("nonce_digest");
    assert_eq!(continuation, digest_secret("stored-token").as_bytes());
    assert_eq!(nonce, digest_secret("stored-nonce").as_bytes());
    assert_ne!(continuation, b"stored-token");
    assert_ne!(nonce, b"stored-nonce");
}

#[tokio::test]
async fn expired_pending_is_replaced_and_live_concurrent_creation_has_one_winner() {
    let (app, user_id, workspace_id) = fixture().await;
    let first = app
        .postgres()
        .create_pending_agent_connection(&new_pending(
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
UPDATE agent_connections
SET created_at = now() - interval '2 hours',
    pending_expires_at = now() - interval '1 hour'
WHERE id = $1
"#,
            &[&Uuid::from(first.id)],
        )
        .await
        .expect("pending expires");

    let replacement = app
        .postgres()
        .create_pending_agent_connection(&new_pending(
            user_id,
            workspace_id,
            "replacement",
            "replacement-nonce",
        ))
        .await
        .expect("expired pending is replaced");
    assert_ne!(replacement.id, first.id);

    assert!(app
        .postgres()
        .revoke_agent_connection(replacement.id)
        .await
        .expect("replacement revokes"));

    let left = new_pending(user_id, workspace_id, "left", "left-nonce");
    let right = new_pending(user_id, workspace_id, "right", "right-nonce");
    let repository = app.postgres_arc();
    let (left, right) = tokio::join!(
        repository.create_pending_agent_connection(&left),
        repository.create_pending_agent_connection(&right)
    );
    let results = [left, right];
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|result| matches!(
                result,
                Err(RepositoryError::Conflict(
                    ConflictKind::AgentConnectionExists
                ))
            ))
            .count(),
        1
    );
}

#[tokio::test]
async fn continuation_is_single_use_and_requires_membership_and_pending_state() {
    let (app, user_id, workspace_id) = fixture().await;
    let pending = app
        .postgres()
        .create_pending_agent_connection(&new_pending(user_id, workspace_id, "continue", "nonce"))
        .await
        .expect("pending connection creates");

    assert!(app
        .postgres()
        .activate_agent_connection(pending.id)
        .await
        .expect("unconsumed activation resolves")
        .is_none());
    assert!(app
        .postgres()
        .consume_agent_connection_continuation(
            digest_secret("continue"),
            digest_secret("wrong-nonce"),
        )
        .await
        .expect("invalid nonce resolves")
        .is_none());

    let consumed = app
        .postgres()
        .consume_agent_connection_continuation(digest_secret("continue"), digest_secret("nonce"))
        .await
        .expect("continuation consumption succeeds")
        .expect("continuation is valid");
    assert_eq!(consumed.id, pending.id);
    assert_eq!(consumed.status, AgentConnectionStatus::Authorized);
    assert!(app
        .postgres()
        .find_reusable_agent_connection(SUBJECT, CLIENT_ID, RESOURCE)
        .await
        .expect("authorized lookup succeeds")
        .is_some());
    assert!(app
        .postgres()
        .consume_agent_connection_continuation(digest_secret("continue"), digest_secret("nonce"),)
        .await
        .expect("replay resolves")
        .is_none());

    let active = app
        .postgres()
        .activate_agent_connection(pending.id)
        .await
        .expect("activation succeeds")
        .expect("consumed connection activates");
    assert_eq!(active.status, AgentConnectionStatus::Active);
    assert!(app
        .postgres()
        .activate_agent_connection(pending.id)
        .await
        .expect("second activation resolves")
        .is_none());

    assert!(app
        .postgres()
        .revoke_agent_connection(active.id)
        .await
        .expect("active connection revokes"));
    let membership_pending = app
        .postgres()
        .create_pending_agent_connection(&new_pending(
            user_id,
            workspace_id,
            "membership",
            "membership-nonce",
        ))
        .await
        .expect("membership pending creates");
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
    assert!(app
        .postgres()
        .consume_agent_connection_continuation(
            digest_secret("membership"),
            digest_secret("membership-nonce"),
        )
        .await
        .expect("membership-less continuation resolves")
        .is_none());
    assert!(app
        .postgres()
        .activate_agent_connection(membership_pending.id)
        .await
        .expect("membership-less activation resolves")
        .is_none());
}

#[tokio::test]
async fn denial_and_revocation_prevent_continuation_and_reuse() {
    let (app, user_id, workspace_id) = fixture().await;
    let denied = app
        .postgres()
        .create_pending_agent_connection(&new_pending(
            user_id,
            workspace_id,
            "denied",
            "denied-nonce",
        ))
        .await
        .expect("denied pending creates");
    assert!(app
        .postgres()
        .deny_pending_agent_connection(digest_secret("denied"))
        .await
        .expect("denial succeeds"));
    assert!(app
        .postgres()
        .consume_agent_connection_continuation(
            digest_secret("denied"),
            digest_secret("denied-nonce"),
        )
        .await
        .expect("denied continuation resolves")
        .is_none());
    assert!(app
        .postgres()
        .activate_agent_connection(denied.id)
        .await
        .expect("denied activation resolves")
        .is_none());

    let pending = app
        .postgres()
        .create_pending_agent_connection(&new_pending(
            user_id,
            workspace_id,
            "revoked",
            "revoked-nonce",
        ))
        .await
        .expect("new pending creates");
    app.postgres()
        .consume_agent_connection_continuation(
            digest_secret("revoked"),
            digest_secret("revoked-nonce"),
        )
        .await
        .expect("continuation consumes")
        .expect("continuation is valid");
    let active = app
        .postgres()
        .activate_agent_connection(pending.id)
        .await
        .expect("activation succeeds")
        .expect("connection activates");
    assert!(app
        .postgres()
        .find_reusable_agent_connection(SUBJECT, CLIENT_ID, RESOURCE)
        .await
        .expect("active lookup succeeds")
        .is_some());
    assert!(app
        .postgres()
        .revoke_agent_connection(active.id)
        .await
        .expect("connection revokes"));
    assert!(app
        .postgres()
        .find_reusable_agent_connection(SUBJECT, CLIENT_ID, RESOURCE)
        .await
        .expect("revoked lookup resolves")
        .is_none());
}

#[tokio::test]
async fn schema_enforces_permissions_lifecycle_uniqueness_and_foreign_keys() {
    let (app, user_id, workspace_id) = fixture().await;
    let pending = app
        .postgres()
        .create_pending_agent_connection(&new_pending(
            user_id,
            workspace_id,
            "constraints",
            "constraints-nonce",
        ))
        .await
        .expect("pending connection creates");
    let client = app
        .postgres()
        .get()
        .await
        .expect("database connection opens");

    let permission_rows = client
        .query(
            "SELECT permission FROM workspace_permissions ORDER BY permission",
            &[],
        )
        .await
        .expect("workspace permission lookup loads");
    let mut persisted_permissions = permission_rows
        .into_iter()
        .map(|row| row.get::<_, String>("permission"))
        .collect::<Vec<_>>();
    let mut expected_permissions = WorkspacePermission::ALL
        .into_iter()
        .map(|permission| permission.as_str().to_owned())
        .collect::<Vec<_>>();
    persisted_permissions.sort();
    expected_permissions.sort();
    assert_eq!(persisted_permissions, expected_permissions);

    let transaction_expiration_column = client
        .query_opt(
            r#"
SELECT 1
FROM information_schema.columns
WHERE table_schema = 'public'
  AND table_name = 'agent_authorization_transactions'
  AND column_name = 'expires_at'
"#,
            &[],
        )
        .await
        .expect("authorization transaction schema loads");
    assert!(
        transaction_expiration_column.is_none(),
        "pending connection expiration is the sole authorization deadline"
    );

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
INSERT INTO api_token_permissions (api_token_id, permission)
VALUES ($1, 'delete_everything')
"#,
            &[&app.api_token_id()],
        )
        .await
        .is_err());
    assert!(client
        .execute(
            r#"
UPDATE agent_connections
SET status = 'authorized', activated_at = now()
WHERE id = $1
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

    let duplicate = app
        .postgres()
        .create_pending_agent_connection(&new_pending(
            user_id,
            workspace_id,
            "duplicate",
            "duplicate-nonce",
        ))
        .await
        .expect_err("live tuple is unique");
    assert!(matches!(
        duplicate,
        RepositoryError::Conflict(ConflictKind::AgentConnectionExists)
    ));

    let repository_error = app
        .postgres()
        .create_pending_agent_connection(&NewPendingAgentConnection {
            id: AgentConnectionId::from(Uuid::new_v4()),
            transaction_id: AgentAuthorizationTransactionId::from(Uuid::new_v4()),
            user_id: UserId::from(Uuid::new_v4()),
            workspace_id: WorkspaceId::from(workspace_id),
            auth0_subject: "auth0|missing-user".to_owned(),
            auth0_client_id: "other-client".to_owned(),
            client_display_name: "Other".to_owned(),
            resource: RESOURCE.to_owned(),
            permissions: vec![WorkspacePermission::ReadControls],
            pending_expires_at: Utc::now() + Duration::minutes(5),
            continuation_digest: digest_secret("missing-user"),
            nonce_digest: digest_secret("missing-user-nonce"),
        })
        .await
        .expect_err("foreign key is enforced");
    assert!(matches!(repository_error, RepositoryError::Database(_)));
}

async fn fixture() -> (TestApp, Uuid, Uuid) {
    let app = TestApp::start_without_default_auth().await;
    let user_id = app.login(SUBJECT).await;
    let workspace = app
        .create_workspace_as(SUBJECT, "Agent Repository Workspace")
        .await;
    let workspace_id = Uuid::parse_str(workspace["id"].as_str().expect("workspace id is returned"))
        .expect("workspace id is a UUID");
    (app, user_id, workspace_id)
}

fn new_pending(
    user_id: Uuid,
    workspace_id: Uuid,
    continuation_token: &str,
    nonce: &str,
) -> NewPendingAgentConnection {
    NewPendingAgentConnection {
        id: AgentConnectionId::from(Uuid::new_v4()),
        transaction_id: AgentAuthorizationTransactionId::from(Uuid::new_v4()),
        user_id: UserId::from(user_id),
        workspace_id: WorkspaceId::from(workspace_id),
        auth0_subject: SUBJECT.to_owned(),
        auth0_client_id: CLIENT_ID.to_owned(),
        client_display_name: "Integration Repository MCP Client".to_owned(),
        resource: RESOURCE.to_owned(),
        permissions: canonical_test_permissions(),
        pending_expires_at: Utc::now() + Duration::minutes(5),
        continuation_digest: digest_secret(continuation_token),
        nonce_digest: digest_secret(nonce),
    }
}

fn canonical_test_permissions() -> Vec<WorkspacePermission> {
    vec![
        WorkspacePermission::ReadEvidenceRequests,
        WorkspacePermission::WriteControls,
    ]
}
