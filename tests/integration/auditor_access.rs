use axum::http::{header::SET_COOKIE, StatusCode};
use proofplane::{
    domain::{WorkspacePermission, WorkspacePermissions},
    mailer::DisabledMailAdapter,
    routes::request_context::REQUEST_ID_HEADER,
    services::{
        agent_connections::AgentConnectionContext,
        auditor_access_grants::{
            AuditorAccessGrantService, CreateAuditorAccessGrantRequest, IssuedAuditorAccessGrant,
        },
        auditor_access_sessions::{AuditorAccessSessionError, AuditorAccessSessionService},
    },
};
use secrecy::ExposeSecret;
use std::sync::Arc;
use uuid::Uuid;

use super::support::{capture_audit_logs, TestApp};

#[tokio::test]
async fn valid_invite_otp_creates_digest_only_session_cookie_and_audits_without_secrets() {
    let app = auditor_app().await;
    let workspace_id = app.workspace_id("workspace");
    let grant = create_grant(&app, workspace_id).await;
    let invite_token = grant.raw_secret.expose_secret().to_owned();

    let request = app
        .server()
        .post(&format!("/auditor-access/{workspace_id}/otp/request"))
        .json(&serde_json::json!({ "token": invite_token }));
    let (response, request_logs) = capture_audit_logs(|request_id| async move {
        request
            .add_header(REQUEST_ID_HEADER, request_id.to_string())
            .await
    })
    .await;
    response.assert_status_ok();
    assert_eq!(app.sent_mail().len(), 1);
    assert_eq!(app.sent_mail()[0].auditor_email, "auditor@example.com");
    let otp = app.sent_mail()[0].code.clone();

    let request = app
        .server()
        .post(&format!("/auditor-access/{workspace_id}/otp/verify"))
        .json(&serde_json::json!({ "token": invite_token, "code": otp }));
    let (response, verify_logs) = capture_audit_logs(|request_id| async move {
        request
            .add_header(REQUEST_ID_HEADER, request_id.to_string())
            .await
    })
    .await;
    response.assert_status_ok();
    let set_cookie = response
        .headers()
        .get(SET_COOKIE)
        .expect("session cookie set")
        .to_str()
        .expect("cookie is ASCII");
    assert!(set_cookie.contains("proofplane_auditor_session="));
    assert!(set_cookie.contains("HttpOnly"));
    assert!(set_cookie.contains("Secure"));
    assert!(set_cookie.contains("SameSite=Lax"));
    assert!(set_cookie.contains("Path=/auditor-access"));
    assert!(set_cookie.contains("Max-Age=604800"));
    let raw_session = cookie_value(set_cookie);

    let session =
        AuditorAccessSessionService::new(app.postgres_arc(), Arc::new(DisabledMailAdapter))
            .load_session(raw_session)
            .await
            .expect("session loads");
    assert_eq!(session.auditor_email, "auditor@example.com");

    let row = app
        .postgres()
        .get()
        .await
        .expect("connection opens")
        .query_one(
            "SELECT octet_length(session_digest) AS digest_length, encode(session_digest, 'hex') AS digest_hex FROM auditor_sessions",
            &[],
        )
        .await
        .expect("session row reads");
    assert_eq!(row.get::<_, i32>("digest_length"), 32);
    assert!(!row.get::<_, String>("digest_hex").contains(raw_session));

    let logs = serde_json::to_string(&(request_logs, verify_logs)).expect("logs serialize");
    assert!(logs.contains("auditor_access_otp.requested"));
    assert!(logs.contains("auditor_access_otp.verified"));
    assert!(logs.contains("auditor_access_session.created"));
    assert!(!logs.contains(&invite_token));
    assert!(!logs.contains(&otp));
    assert!(!logs.contains(raw_session));
}

#[tokio::test]
async fn wrong_reused_rate_limited_and_invalid_invites_do_not_create_sessions() {
    let app = auditor_app().await;
    let workspace_id = app.workspace_id("workspace");
    let other_workspace_id = app.workspace_id("other");
    let grant = create_grant(&app, workspace_id).await;
    let invite_token = grant.raw_secret.expose_secret();

    for _ in 0..3 {
        app.server()
            .post(&format!("/auditor-access/{workspace_id}/otp/request"))
            .json(&serde_json::json!({ "token": invite_token }))
            .await
            .assert_status_ok();
    }
    let rate_limited = app
        .server()
        .post(&format!("/auditor-access/{workspace_id}/otp/request"))
        .json(&serde_json::json!({ "token": invite_token }))
        .await;
    assert_eq!(rate_limited.status_code(), StatusCode::CONFLICT);

    let wrong = app
        .server()
        .post(&format!("/auditor-access/{workspace_id}/otp/verify"))
        .json(&serde_json::json!({ "token": invite_token, "code": "000000" }))
        .await;
    assert_eq!(wrong.status_code(), StatusCode::NOT_FOUND);

    let code = app.sent_mail().last().expect("OTP sent").code.clone();
    app.server()
        .post(&format!("/auditor-access/{workspace_id}/otp/verify"))
        .json(&serde_json::json!({ "token": invite_token, "code": code }))
        .await
        .assert_status_ok();
    let reused = app
        .server()
        .post(&format!("/auditor-access/{workspace_id}/otp/verify"))
        .json(&serde_json::json!({ "token": invite_token, "code": code }))
        .await;
    assert_eq!(reused.status_code(), StatusCode::NOT_FOUND);

    let invalid = app
        .server()
        .post(&format!("/auditor-access/{workspace_id}/otp/request"))
        .json(&serde_json::json!({ "token": "not-a-token" }))
        .await;
    assert_eq!(invalid.status_code(), StatusCode::NOT_FOUND);
    let cross_workspace = app
        .server()
        .post(&format!("/auditor-access/{other_workspace_id}/otp/request"))
        .json(&serde_json::json!({ "token": invite_token }))
        .await;
    assert_eq!(cross_workspace.status_code(), StatusCode::NOT_FOUND);
    assert_eq!(app.sent_mail().len(), 3);
}

#[tokio::test]
async fn logout_and_grant_revocation_invalidate_existing_session() {
    let app = auditor_app().await;
    let workspace_id = app.workspace_id("workspace");
    let grant = create_grant(&app, workspace_id).await;
    let invite_token = grant.raw_secret.expose_secret();
    let session_service =
        AuditorAccessSessionService::new(app.postgres_arc(), Arc::new(DisabledMailAdapter));

    let raw_session = verified_session_cookie(&app, workspace_id, invite_token).await;
    app.server()
        .post("/auditor-access/logout")
        .add_header(
            "Cookie",
            format!("proofplane_auditor_session={raw_session}"),
        )
        .await
        .assert_status_ok();
    assert_unavailable(session_service.load_session(&raw_session).await);

    let raw_session = verified_session_cookie(&app, workspace_id, invite_token).await;
    AuditorAccessGrantService::new(app.postgres_arc())
        .revoke(
            &agent_connection_context(&app, workspace_id),
            grant.grant.id,
        )
        .await
        .expect("grant revokes");
    assert_unavailable(session_service.load_session(&raw_session).await);
}

async fn verified_session_cookie(app: &TestApp, workspace_id: Uuid, invite_token: &str) -> String {
    app.server()
        .post(&format!("/auditor-access/{workspace_id}/otp/request"))
        .json(&serde_json::json!({ "token": invite_token }))
        .await
        .assert_status_ok();
    let code = app.sent_mail().last().expect("OTP sent").code.clone();
    let response = app
        .server()
        .post(&format!("/auditor-access/{workspace_id}/otp/verify"))
        .json(&serde_json::json!({ "token": invite_token, "code": code }))
        .await;
    response.assert_status_ok();
    cookie_value(
        response
            .headers()
            .get(SET_COOKIE)
            .expect("cookie set")
            .to_str()
            .expect("cookie is ASCII"),
    )
    .to_owned()
}

async fn auditor_app() -> TestApp {
    TestApp::builder()
        .without_default_auth()
        .workspace("workspace", "Auditor access workspace")
        .with_default_membership()
        .workspace("other", "Other auditor access workspace")
        .with_default_membership()
        .build()
        .await
}

async fn create_grant(app: &TestApp, workspace_id: Uuid) -> IssuedAuditorAccessGrant {
    AuditorAccessGrantService::new(app.postgres_arc())
        .create(
            &agent_connection_context(app, workspace_id),
            CreateAuditorAccessGrantRequest {
                auditor_email: "auditor@example.com".to_owned(),
                expires_at: None,
            },
        )
        .await
        .expect("auditor grant creates")
}

fn agent_connection_context(app: &TestApp, workspace_id: Uuid) -> AgentConnectionContext {
    AgentConnectionContext {
        user_id: app.user_id().into(),
        connection_id: app.api_token_id().into(),
        workspace_id: workspace_id.into(),
        permissions: WorkspacePermissions::from_iter(WorkspacePermission::ALL),
    }
}

fn cookie_value(set_cookie: &str) -> &str {
    set_cookie
        .split(';')
        .next()
        .expect("cookie pair")
        .split_once('=')
        .expect("cookie has value")
        .1
}

fn assert_unavailable(result: Result<impl std::fmt::Debug, AuditorAccessSessionError>) {
    assert!(matches!(
        result,
        Err(AuditorAccessSessionError::Unavailable)
    ));
}
