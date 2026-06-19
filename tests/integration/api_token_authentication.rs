use axum::http::StatusCode;
use chrono::{Duration as ChronoDuration, Utc};
use proofplane::{
    authentication::{
        paseto::{ApiTokenSigner, ApiTokenVerifier, RegisteredClaims},
        ApiTokenAuthenticator, UserApiTokenClaims,
    },
    config::{PasetoApiConfig, PasetoApiSigningKey, PasetoApiVerificationKey},
    domain::{ApiTokenId, CreateApiTokenPayload, UserId, WorkspaceId, WorkspacePermission},
    routes::authentication::{ACTOR_ID_HEADER, API_KEY_HEADER, AUTHORIZATION_HEADER},
};
use secrecy::SecretString;
use tokio::time::{sleep, Duration};
use uuid::Uuid;

use super::support::{TestApp, INTEGRATION_ACTOR_ID};

const API_AUDIENCE: &str = "proofplane-api";
const API_SECRET: &str = "k4.secret.sEP9YtkNeO7EGJbpVYznvHnVXotZyGbkzuvHkOO3RgXAqGWIhrrfscm74zMx72tBOOD02gy8G4sB8-60b1cWiw";
const API_PUBLIC: &str = "k4.public.wKhliIa637HJu-MzMe9rQTjg9NoMvBuLAfPutG9XFos";

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
    let other_user_id = app.login("auth0|api-token-auth-other-user").await;
    let workspace_id = workspace_uuid(&app.create_workspace_as(sub, "Auth Rejections").await);
    let other_workspace_id = workspace_uuid(&app.create_workspace_as(sub, "Other Auth").await);

    let unknown = sign_token(
        user_id,
        workspace_id,
        vec![WorkspacePermission::ReadControls],
    );
    assert_authenticates_none(&app, &unknown.raw).await;
    assert_authenticates_none(&app, "not-a-token").await;

    let revoked = issue_token(
        &app,
        user_id,
        workspace_id,
        vec![WorkspacePermission::ReadControls],
    )
    .await;
    execute(
        &app,
        "UPDATE api_tokens SET revoked_at = now() WHERE id = $1",
        &[&revoked.token_id],
    )
    .await;
    assert_authenticates_none(&app, &revoked.raw).await;

    let expired_row = issue_token(
        &app,
        user_id,
        workspace_id,
        vec![WorkspacePermission::ReadControls],
    )
    .await;
    execute(
        &app,
        "UPDATE api_tokens SET expires_at = now() - interval '1 second' WHERE id = $1",
        &[&expired_row.token_id],
    )
    .await;
    assert_authenticates_none(&app, &expired_row.raw).await;

    let user_mismatch = issue_token(
        &app,
        user_id,
        workspace_id,
        vec![WorkspacePermission::ReadControls],
    )
    .await;
    execute(
        &app,
        "UPDATE api_tokens SET user_id = $2 WHERE id = $1",
        &[&user_mismatch.token_id, &other_user_id],
    )
    .await;
    assert_authenticates_none(&app, &user_mismatch.raw).await;

    let workspace_mismatch = issue_token(
        &app,
        user_id,
        workspace_id,
        vec![WorkspacePermission::ReadControls],
    )
    .await;
    execute(
        &app,
        "UPDATE api_tokens SET workspace_id = $2 WHERE id = $1",
        &[&workspace_mismatch.token_id, &other_workspace_id],
    )
    .await;
    assert_authenticates_none(&app, &workspace_mismatch.raw).await;

    let expiration_mismatch = issue_token(
        &app,
        user_id,
        workspace_id,
        vec![WorkspacePermission::ReadControls],
    )
    .await;
    execute(
        &app,
        "UPDATE api_tokens SET expires_at = expires_at + interval '1 second' WHERE id = $1",
        &[&expiration_mismatch.token_id],
    )
    .await;
    assert_authenticates_none(&app, &expiration_mismatch.raw).await;

    let permission_mismatch = issue_token(
        &app,
        user_id,
        workspace_id,
        vec![
            WorkspacePermission::ReadControls,
            WorkspacePermission::WriteControls,
        ],
    )
    .await;
    execute(
        &app,
        "DELETE FROM api_token_permissions WHERE api_token_id = $1 AND permission = 'write_controls'",
        &[&permission_mismatch.token_id],
    )
    .await;
    assert_authenticates_none(&app, &permission_mismatch.raw).await;

    let stale_membership = issue_token(
        &app,
        user_id,
        workspace_id,
        vec![WorkspacePermission::ReadControls],
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

#[tokio::test]
async fn paseto_bearer_does_not_authenticate_current_data_plane_routes() {
    let app = TestApp::builder()
        .without_default_auth()
        .workspace("workspace", "REST unchanged")
        .with_default_membership()
        .build()
        .await;
    let workspace_id = app.workspace_id("workspace");
    let user_id = app.login("auth0|api-token-auth-rest").await;
    let issued = issue_token(
        &app,
        user_id,
        workspace_id,
        vec![WorkspacePermission::ReadEvidenceRequests],
    )
    .await;
    let path = format!("/workspaces/{workspace_id}/evidence-requests");

    let bearer_only = app
        .server()
        .get(&path)
        .add_header(AUTHORIZATION_HEADER, format!("Bearer {}", issued.raw))
        .await;
    assert_eq!(bearer_only.status_code(), StatusCode::UNAUTHORIZED);

    app.server()
        .get(&path)
        .add_header(ACTOR_ID_HEADER, INTEGRATION_ACTOR_ID)
        .add_header(API_KEY_HEADER, app.api_key())
        .await
        .assert_status_ok();
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
) -> IssuedToken {
    let signed = sign_token(user_id, workspace_id, permissions.clone());
    app.postgres()
        .create_api_token(&CreateApiTokenPayload {
            id: ApiTokenId::from(signed.token_id),
            user_id: UserId::from(user_id),
            workspace_id: WorkspaceId::from(workspace_id),
            name: "Integration auth token".to_owned(),
            expires_at: signed.expires_at,
            permissions,
        })
        .await
        .expect("API token row inserts");

    IssuedToken {
        raw: signed.raw,
        token_id: signed.token_id,
    }
}

struct SignedToken {
    raw: String,
    token_id: Uuid,
    expires_at: chrono::DateTime<Utc>,
}

fn sign_token(
    user_id: Uuid,
    workspace_id: Uuid,
    permissions: Vec<WorkspacePermission>,
) -> SignedToken {
    let token_id = Uuid::new_v4();
    let expires_at = Utc::now() + ChronoDuration::days(30);
    let issued = signer()
        .issue(
            RegisteredClaims {
                subject: user_id,
                token_id,
                expires_at,
            },
            &UserApiTokenClaims::new(WorkspaceId::from(workspace_id), &permissions),
        )
        .expect("PASETO issues");

    SignedToken {
        raw: issued.token,
        token_id,
        expires_at: issued.expires_at,
    }
}

fn authenticator(app: &TestApp) -> ApiTokenAuthenticator {
    ApiTokenAuthenticator::new(verifier(), app.postgres_arc())
}

fn signer() -> ApiTokenSigner {
    ApiTokenSigner::from_config(issuer(), API_AUDIENCE, &api_config()).unwrap()
}

fn verifier() -> ApiTokenVerifier {
    ApiTokenVerifier::from_config(issuer(), API_AUDIENCE, &api_config()).unwrap()
}

fn api_config() -> PasetoApiConfig {
    PasetoApiConfig {
        active_signing_key: PasetoApiSigningKey {
            id: "integration-api-001".to_owned(),
            secret: SecretString::from(API_SECRET),
        },
        verification_keys: vec![PasetoApiVerificationKey {
            id: "integration-api-001".to_owned(),
            public: API_PUBLIC.to_owned(),
        }],
    }
}

fn issuer() -> url::Url {
    url::Url::parse("https://api.proofplane.test/").unwrap()
}

async fn assert_authenticates_none(app: &TestApp, raw: &str) {
    assert!(authenticator(app)
        .authenticate(raw)
        .await
        .expect("auth infrastructure succeeds")
        .is_none());
}

async fn execute(app: &TestApp, sql: &str, params: &[&(dyn tokio_postgres::types::ToSql + Sync)]) {
    app.postgres()
        .get()
        .await
        .expect("connection opens")
        .execute(sql, params)
        .await
        .expect("fixture SQL executes");
}

fn workspace_uuid(value: &serde_json::Value) -> Uuid {
    Uuid::parse_str(value["id"].as_str().expect("workspace id is a string"))
        .expect("workspace id is a UUID")
}
