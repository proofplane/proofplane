use chrono::{Duration as ChronoDuration, Utc};
use proofplane::{
    authentication::{opaque_token::generate_opaque_token, ApiTokenAuthenticator},
    domain::{ApiTokenId, CreateApiTokenPayload, UserId, WorkspaceId, WorkspacePermission},
};
use secrecy::ExposeSecret;
use tokio::time::{sleep, Duration};
use uuid::Uuid;

use super::support::TestApp;

#[tokio::test]
async fn valid_active_token_for_current_member_authenticates() {
    let app = TestApp::start_without_default_auth().await;
    let user_id = app.login("auth0|api-token-auth-valid").await;
    let workspace_id = workspace_uuid(
        &app.create_workspace_as("auth0|api-token-auth-valid", "Auth Valid")
            .await,
    );
    let issued = issue_token(
        &app,
        user_id,
        workspace_id,
        vec![
            WorkspacePermission::ReadControls,
            WorkspacePermission::WriteControls,
        ],
        Utc::now() + ChronoDuration::days(30),
    )
    .await;

    let context = authenticator(&app)
        .authenticate(&issued.raw)
        .await
        .expect("auth infrastructure succeeds")
        .expect("token authenticates");

    assert_eq!(context.user_id, UserId::from(user_id));
    assert_eq!(context.api_token_id, ApiTokenId::from(issued.token_id));
    assert_eq!(context.workspace_id, WorkspaceId::from(workspace_id));
    assert!(context.allows(
        WorkspaceId::from(workspace_id),
        WorkspacePermission::ReadControls
    ));
    assert!(!context.allows(
        WorkspaceId::from(workspace_id),
        WorkspacePermission::ReadEvidenceRequests
    ));
    assert!(!context.allows(
        WorkspaceId::from(Uuid::new_v4()),
        WorkspacePermission::ReadControls
    ));
}

#[tokio::test]
async fn invalid_or_stale_tokens_authenticate_as_none() {
    let app = TestApp::start_without_default_auth().await;
    let sub = "auth0|api-token-auth-rejections";
    let user_id = app.login(sub).await;
    let workspace_id = workspace_uuid(&app.create_workspace_as(sub, "Auth Rejections").await);

    let unknown = generate_opaque_token().expect("unknown opaque token generates");
    assert_authenticates_none(&app, unknown.raw_token.expose_secret()).await;
    assert_authenticates_none(&app, "not-a-token").await;
    assert_authenticates_none(&app, "v4.public.example").await;
    assert_authenticates_none(&app, bad_checksum_token()).await;

    let revoked = issue_token(
        &app,
        user_id,
        workspace_id,
        vec![WorkspacePermission::ReadControls],
        Utc::now() + ChronoDuration::days(30),
    )
    .await;
    execute(
        &app,
        "UPDATE api_tokens SET revoked_at = now() WHERE id = $1",
        &[&revoked.token_id],
    )
    .await;
    assert_authenticates_none(&app, &revoked.raw).await;

    let expired = issue_token(
        &app,
        user_id,
        workspace_id,
        vec![WorkspacePermission::ReadControls],
        Utc::now() - ChronoDuration::minutes(5),
    )
    .await;
    assert_authenticates_none(&app, &expired.raw).await;

    let stale_membership = issue_token(
        &app,
        user_id,
        workspace_id,
        vec![WorkspacePermission::ReadControls],
        Utc::now() + ChronoDuration::days(30),
    )
    .await;
    execute(
        &app,
        "DELETE FROM workspace_memberships WHERE workspace_id = $1 AND user_id = $2",
        &[&workspace_id, &user_id],
    )
    .await;
    assert_authenticates_none(&app, &stale_membership.raw).await;
}

#[tokio::test]
async fn last_used_at_is_set_and_advances_on_successful_authentication() {
    let app = TestApp::start_without_default_auth().await;
    let sub = "auth0|api-token-auth-last-used";
    let user_id = app.login(sub).await;
    let workspace_id = workspace_uuid(&app.create_workspace_as(sub, "Last Used").await);
    let issued = issue_token(
        &app,
        user_id,
        workspace_id,
        vec![WorkspacePermission::ReadControls],
        Utc::now() + ChronoDuration::days(30),
    )
    .await;

    authenticator(&app)
        .authenticate(&issued.raw)
        .await
        .expect("auth succeeds")
        .expect("token authenticates");
    let first = app
        .postgres()
        .get_api_token(ApiTokenId::from(issued.token_id))
        .await
        .expect("token reads")
        .expect("token exists")
        .token
        .last_used_at
        .expect("last_used_at is set");

    sleep(Duration::from_millis(20)).await;
    authenticator(&app)
        .authenticate(&issued.raw)
        .await
        .expect("auth succeeds")
        .expect("token authenticates");
    let second = app
        .postgres()
        .get_api_token(ApiTokenId::from(issued.token_id))
        .await
        .expect("token reads")
        .expect("token exists")
        .token
        .last_used_at
        .expect("last_used_at is set again");

    assert!(second > first);
}

struct IssuedToken {
    raw: String,
    token_id: Uuid,
}

async fn issue_token(
    app: &TestApp,
    user_id: Uuid,
    workspace_id: Uuid,
    permissions: Vec<WorkspacePermission>,
    expires_at: chrono::DateTime<Utc>,
) -> IssuedToken {
    let generated = generate_opaque_token().expect("opaque token generates");
    let token_id = Uuid::new_v4();
    app.postgres()
        .create_api_token(&CreateApiTokenPayload {
            id: ApiTokenId::from(token_id),
            token_digest: generated.digest,
            user_id: UserId::from(user_id),
            workspace_id: WorkspaceId::from(workspace_id),
            name: "Integration auth token".to_owned(),
            expires_at,
            permissions,
        })
        .await
        .expect("API token row inserts");

    IssuedToken {
        raw: generated.raw_token.expose_secret().to_owned(),
        token_id,
    }
}

fn authenticator(app: &TestApp) -> ApiTokenAuthenticator {
    ApiTokenAuthenticator::new(app.postgres_arc())
}

async fn assert_authenticates_none(app: &TestApp, raw: &str) {
    assert!(authenticator(app)
        .authenticate(raw)
        .await
        .expect("auth infrastructure succeeds")
        .is_none());
}

fn bad_checksum_token() -> &'static str {
    "ppat_000000000000000000000000000000000000"
}

async fn execute(app: &TestApp, sql: &str, params: &[&(dyn tokio_postgres::types::ToSql + Sync)]) {
    app.postgres()
        .get()
        .await
        .expect("connection opens")
        .execute(sql, params)
        .await
        .expect("statement executes");
}

fn workspace_uuid(created: &serde_json::Value) -> Uuid {
    Uuid::parse_str(created["id"].as_str().expect("workspace id is a string"))
        .expect("workspace id is a UUID")
}
