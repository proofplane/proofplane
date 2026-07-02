use axum::http::{header::SET_COOKIE, StatusCode};
use chrono::{DateTime, Utc};
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
use serde_json::{json, Value};
use std::sync::Arc;
use uuid::Uuid;

use super::support::{capture_audit_logs, upload_attachment, TestApp};

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

#[tokio::test]
async fn portal_data_returns_workspace_graph_and_filters_archived_attachments() {
    let app = auditor_app().await;
    let workspace_id = app.workspace_id("workspace");
    let control_id = app.control_id("workspace", "PP-AC-01");
    let grant = create_grant(&app, workspace_id).await;
    let invite_token = grant.raw_secret.expose_secret();

    let request = app
        .create_evidence_request(
            workspace_id,
            &evidence_request("Access review evidence", "2026-03-01T00:00:00Z"),
        )
        .await;
    let request_id = uuid_field(&request["id"]);
    insert_control_mapping(
        &app,
        request_id,
        control_id,
        "Shows access reviews were performed.",
    )
    .await;

    let unmapped = app
        .create_evidence_request(
            workspace_id,
            &evidence_request("Unmapped evidence", "2026-02-01T00:00:00Z"),
        )
        .await;
    app.create_evidence_submission(
        workspace_id,
        uuid_field(&unmapped["id"]),
        &submission("unmapped should not appear"),
    )
    .await;

    let older = app
        .create_evidence_submission(workspace_id, request_id, &submission("older submission"))
        .await;
    let newer = app
        .create_evidence_submission(workspace_id, request_id, &submission("newer submission"))
        .await;
    let older_submission_id = uuid_field(&older["id"]);
    let newer_submission_id = uuid_field(&newer["id"]);
    set_received_at(&app, older_submission_id, "2026-04-01T00:00:00Z").await;
    set_received_at(&app, newer_submission_id, "2026-05-01T00:00:00Z").await;

    let uploaded = upload_attachment(
        &app,
        workspace_id,
        newer_submission_id,
        "eligible.txt",
        b"eligible",
    )
    .await;
    let uploaded_id = uuid_field(&uploaded["id"]);
    finalize_attachment(&app, workspace_id, newer_submission_id, uploaded_id).await;

    let pending = upload_attachment(
        &app,
        workspace_id,
        newer_submission_id,
        "pending.txt",
        b"pending",
    )
    .await;
    let pending_id = uuid_field(&pending["id"]);

    let archived = upload_attachment(
        &app,
        workspace_id,
        older_submission_id,
        "archived.txt",
        b"archived",
    )
    .await;
    let archived_id = uuid_field(&archived["id"]);
    finalize_attachment(&app, workspace_id, older_submission_id, archived_id).await;
    archive_attachment(&app, archived_id).await;

    let raw_session = verified_session_cookie(&app, workspace_id, invite_token).await;
    let request = app.server().get("/auditor-access/portal/data").add_header(
        "Cookie",
        format!("proofplane_auditor_session={raw_session}"),
    );
    let (response, logs) = capture_audit_logs(|request_id| async move {
        request
            .add_header(REQUEST_ID_HEADER, request_id.to_string())
            .await
    })
    .await;
    response.assert_status_ok();
    let body = response.json::<Value>();
    let serialized = serde_json::to_string(&body).expect("portal response serializes");

    assert_eq!(body["workspace_id"], workspace_id.to_string());
    assert_eq!(body["auditor_email"], "auditor@example.com");
    assert!(serialized.contains("Access review evidence"));
    assert!(!serialized.contains("Unmapped evidence"));
    assert!(!serialized.contains("Other auditor access workspace"));
    assert!(!serialized.contains("archived.txt"));
    assert!(!serialized.contains("object_key"));
    assert!(!serialized.contains("quarantine/"));
    assert!(!serialized.contains("workspaces/"));
    assert!(!serialized.contains(invite_token));
    assert!(!serialized.contains(&raw_session));

    let controls = body["controls"].as_array().expect("controls is array");
    let control = controls
        .iter()
        .find(|control| control["code"] == "PP-AC-01")
        .expect("mapped control appears");
    let portal_request = &control["evidence_requests"][0];
    assert_eq!(
        portal_request["mapping_rationale"],
        "Shows access reviews were performed."
    );
    let submissions = portal_request["submissions"]
        .as_array()
        .expect("submissions is array");
    assert_eq!(submissions.len(), 2);
    assert_eq!(
        submissions[0]["submission"]["id"],
        newer_submission_id.to_string()
    );
    assert_eq!(
        submissions[1]["submission"]["id"],
        older_submission_id.to_string()
    );

    let attachments = submissions[0]["attachments"]
        .as_array()
        .expect("attachments is array");
    assert_eq!(attachments[0]["filename"], "eligible.txt");
    assert_eq!(attachments[0]["upload_status"], "uploaded");
    assert_eq!(attachments[0]["download_eligible"], true);
    assert_eq!(attachments[1]["filename"], "pending.txt");
    assert_eq!(attachments[1]["upload_status"], "pending");
    assert_eq!(attachments[1]["download_eligible"], false);
    assert_eq!(uuid_field(&attachments[1]["id"]), pending_id);

    let logs = serde_json::to_string(&logs).expect("logs serialize");
    assert!(logs.contains("auditor_portal.read"));
    assert!(!logs.contains(invite_token));
    assert!(!logs.contains(&raw_session));
}

#[tokio::test]
async fn portal_data_rejects_missing_tampered_and_revoked_sessions() {
    let app = auditor_app().await;
    let workspace_id = app.workspace_id("workspace");
    let grant = create_grant(&app, workspace_id).await;
    let invite_token = grant.raw_secret.expose_secret();

    app.server()
        .get("/auditor-access/portal/data")
        .await
        .assert_status_not_found();
    app.server()
        .get("/auditor-access/portal/data")
        .add_header("Cookie", "proofplane_auditor_session=tampered")
        .await
        .assert_status_not_found();

    let raw_session = verified_session_cookie(&app, workspace_id, invite_token).await;
    AuditorAccessGrantService::new(app.postgres_arc())
        .revoke(
            &agent_connection_context(&app, workspace_id),
            grant.grant.id,
        )
        .await
        .expect("grant revokes");
    app.server()
        .get("/auditor-access/portal/data")
        .add_header(
            "Cookie",
            format!("proofplane_auditor_session={raw_session}"),
        )
        .await
        .assert_status_not_found();
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
        .with_control("PP-AC-01", "Access reviews", vec![])
        .with_default_membership()
        .workspace("other", "Other auditor access workspace")
        .with_control("PP-OTHER", "Other control", vec![])
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

fn evidence_request(title: &str, due_at: &str) -> Value {
    json!({
        "title": title,
        "description": format!("Description for {title}."),
        "collection_instructions": format!("Collect {title}."),
        "cadence": "quarterly",
        "due_at": due_at,
        "schedule_anchor_at": "2026-01-01T00:00:00Z",
        "freshness_window_days": 90,
        "status": "active"
    })
}

fn submission(summary: &str) -> Value {
    json!({
        "coverage_start_at": "2026-01-01T00:00:00Z",
        "coverage_end_at": "2026-03-31T23:59:59Z",
        "source_system": "test",
        "collection_method": "manual",
        "summary": summary,
        "description": format!("Description for {summary}.")
    })
}

fn uuid_field(value: &Value) -> Uuid {
    Uuid::parse_str(value.as_str().expect("UUID field is a string")).expect("field is a UUID")
}

async fn insert_control_mapping(
    app: &TestApp,
    evidence_request_id: Uuid,
    control_id: Uuid,
    rationale: &str,
) {
    app.postgres()
        .get()
        .await
        .expect("connection opens")
        .execute(
            r#"
INSERT INTO evidence_request_control_mappings (evidence_request_id, control_id, rationale)
VALUES ($1, $2, $3)
"#,
            &[&evidence_request_id, &control_id, &rationale],
        )
        .await
        .expect("control mapping inserts");
}

async fn set_received_at(app: &TestApp, submission_id: Uuid, received_at: &str) {
    let received_at = DateTime::parse_from_rfc3339(received_at).expect("received_at parses");
    let received_at = received_at.with_timezone(&Utc);
    app.postgres()
        .get()
        .await
        .expect("connection opens")
        .execute(
            "UPDATE evidence_submissions SET received_at = $2 WHERE id = $1",
            &[&submission_id, &received_at],
        )
        .await
        .expect("submission received_at updates");
}

async fn archive_attachment(app: &TestApp, attachment_id: Uuid) {
    app.postgres()
        .get()
        .await
        .expect("connection opens")
        .execute(
            "UPDATE evidence_attachments SET archived = true WHERE id = $1",
            &[&attachment_id],
        )
        .await
        .expect("attachment archives");
}

async fn finalize_attachment(
    app: &TestApp,
    _workspace_id: Uuid,
    _submission_id: Uuid,
    attachment_id: Uuid,
) {
    app.postgres()
        .get()
        .await
        .expect("connection opens")
        .execute(
            "UPDATE evidence_attachments SET upload_status = 'uploaded' WHERE id = $1",
            &[&attachment_id],
        )
        .await
        .expect("attachment finalizes");
}
