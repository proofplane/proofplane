use http::{
    header::{LOCATION, SET_COOKIE},
    StatusCode,
};
use proofplane::{
    authentication::opaque_token::{ALPHABET, PREFIX, TOKEN_LENGTH},
    domain::WorkspacePermission,
    routes::request_context::REQUEST_ID_HEADER,
};
use serde_json::{json, Value};
use url::Url;
use uuid::Uuid;

use crate::support::{
    auditor_access::invite_token,
    harness,
    http::request_cookie,
    json::{assert_rfc3339, object_keys},
    mcp::McpClient,
    oauth::authorize_agent_connection,
    scenario::ScenarioBuilder,
};

use super::helpers::{assert_audit_record, assert_verification_page, wrong_otp, ResendControl};

#[tokio::test]
async fn invite_otp_creates_one_scoped_session_and_safe_audits() {
    let app = harness::app().await;
    let subject = "auth0|auditor-session-success";
    let auditor_email = "auditor-session-success@example.com";

    let scenario = ScenarioBuilder::new(&app)
        .with_user(subject)
        .with_workspace(subject, "Auditor Session Success")
        .build()
        .await;
    let workspace_id = scenario.workspace("Auditor Session Success").id;

    let token = authorize_agent_connection(
        &app,
        subject,
        "Auditor Session Success Agent",
        &[WorkspacePermission::ManageAuditorAccess],
    )
    .await;
    let client = McpClient::connect(app.mcp_server(), &token).await;
    let created = client
        .call_tool(
            "create_auditor_access_link",
            json!({
                "email": " Auditor-Session-Success@Example.COM ",
                "expires_at": "2099-01-01T00:00:00Z",
                "period_start": "2026-01-01T00:00:00Z",
                "period_end": "2026-03-31T23:59:59Z",
            }),
        )
        .await;

    assert_eq!(
        object_keys(&created),
        ["grant", "intended_use", "url", "url_secret_type"]
            .into_iter()
            .collect()
    );
    assert_eq!(
        object_keys(&created["grant"]),
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
    let grant_id = created["grant"]["id"]
        .as_str()
        .expect("grant id is a string");
    Uuid::parse_str(grant_id).expect("grant id is a UUID");
    assert_eq!(created["grant"]["auditor_email"], auditor_email);
    assert_rfc3339(&created["grant"]["created_at"]);
    assert_eq!(created["grant"]["expires_at"], "2099-01-01T00:00:00.000Z");
    assert_eq!(created["grant"]["period_start"], "2026-01-01T00:00:00.000Z");
    assert_eq!(created["grant"]["period_end"], "2026-03-31T23:59:59.000Z");
    assert_eq!(created["grant"]["revoked_at"], Value::Null);
    assert_eq!(created["url_secret_type"], "bearer_secret");
    assert_eq!(created["intended_use"], "auditor_browser_access");

    let invite_url = Url::parse(created["url"].as_str().expect("auditor URL is a string"))
        .expect("auditor URL parses");
    assert_eq!(invite_url.scheme(), "https");
    assert_eq!(invite_url.host_str(), Some("api.proofplane.test"));
    assert_eq!(invite_url.path(), format!("/auditor-access/{workspace_id}"));
    let invite_token = invite_token(&invite_url);
    assert_eq!(invite_token.len(), TOKEN_LENGTH);
    assert!(invite_token.starts_with(PREFIX));
    assert!(invite_token[PREFIX.len()..]
        .bytes()
        .all(|byte| ALPHABET.contains(&byte)));

    let (requested, request_logs) = app
        .capture_audit_logs(async |request_id| {
            app.app_server()
                .post(&format!("/auditor-access/{workspace_id}/otp/request"))
                .add_header(REQUEST_ID_HEADER, request_id.to_string())
                .json(&json!({ "token": invite_token }))
                .await
        })
        .await;
    requested.assert_status_ok();
    assert_eq!(requested.json::<Value>(), json!({ "status": "sent" }));

    let sent = app.mailer().sent_mail_for(auditor_email);
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].auditor_email, auditor_email);
    assert!(app
        .mailer()
        .sent_mail_for("auditor-session-success-other@example.com")
        .is_empty());
    let code = sent[0].code.clone();

    let wrong = app
        .app_server()
        .post(&format!("/auditor-access/{workspace_id}/otp/verify"))
        .json(&json!({
            "token": invite_token,
            "code": wrong_otp(&code),
        }))
        .await;
    wrong.assert_status_not_found();
    assert_eq!(
        wrong.json::<Value>(),
        json!({
            "error": {
                "code": "not_found",
                "message": "route not found",
                "details": [],
            }
        })
    );

    let (verified, verify_logs) = app
        .capture_audit_logs(async |request_id| {
            app.app_server()
                .post(&format!("/auditor-access/{workspace_id}/otp/verify"))
                .add_header(REQUEST_ID_HEADER, request_id.to_string())
                .json(&json!({
                    "token": invite_token,
                    "code": code,
                }))
                .await
        })
        .await;
    verified.assert_status_ok();
    assert_eq!(verified.json::<Value>(), json!({ "status": "verified" }));

    let cookie = verified
        .headers()
        .get(SET_COOKIE)
        .expect("verification sets a session cookie")
        .to_str()
        .expect("session cookie is text");
    let cookie_parts = cookie.split("; ").collect::<Vec<_>>();
    assert_eq!(cookie_parts.len(), 6);
    let (cookie_name, raw_session) = cookie_parts[0]
        .split_once('=')
        .expect("session cookie has a name and value");
    assert_eq!(cookie_name, "proofplane_auditor_session");
    assert_eq!(raw_session.len(), 43);
    assert!(raw_session
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_')));
    assert_eq!(
        &cookie_parts[1..],
        [
            "HttpOnly",
            "SameSite=Lax",
            "Path=/auditor-access",
            "Max-Age=604800",
            "Secure",
        ]
    );

    let reused = app
        .app_server()
        .post(&format!("/auditor-access/{workspace_id}/otp/verify"))
        .json(&json!({
            "token": invite_token,
            "code": code,
        }))
        .await;
    reused.assert_status_not_found();
    assert_eq!(
        reused.json::<Value>(),
        json!({
            "error": {
                "code": "not_found",
                "message": "route not found",
                "details": [],
            }
        })
    );

    assert_eq!(request_logs.len(), 1);
    assert_audit_record(
        &request_logs[0],
        "auditor_access_otp.requested",
        "request_auditor_access_otp",
        workspace_id,
        auditor_email,
        Some(grant_id),
    );

    assert_eq!(verify_logs.len(), 2);
    assert_audit_record(
        &verify_logs[0],
        "auditor_access_otp.verified",
        "verify_auditor_access_otp",
        workspace_id,
        auditor_email,
        Some(grant_id),
    );
    assert_audit_record(
        &verify_logs[1],
        "auditor_access_session.created",
        "create_auditor_access_session",
        workspace_id,
        auditor_email,
        None,
    );
    assert_eq!(
        verify_logs[0]["fields"]["request_id"],
        verify_logs[1]["fields"]["request_id"]
    );
}

#[tokio::test]
async fn browser_resend_replaces_the_previous_code_and_opens_a_session() {
    let app = harness::app().await;
    let subject = "auth0|auditor-browser-resend-success";
    let auditor_email = "auditor-browser-resend-success@example.com";

    let scenario = ScenarioBuilder::new(&app)
        .with_user(subject)
        .with_workspace(subject, "Auditor Browser Resend Success")
        .build()
        .await;
    let workspace_id = scenario.workspace("Auditor Browser Resend Success").id;

    let token = authorize_agent_connection(
        &app,
        subject,
        "Auditor Browser Resend Success Agent",
        &[WorkspacePermission::ManageAuditorAccess],
    )
    .await;
    let client = McpClient::connect(app.mcp_server(), &token).await;
    let created = client
        .call_tool(
            "create_auditor_access_link",
            json!({
                "email": auditor_email,
                "expires_at": "2099-01-01T00:00:00Z",
                "period_start": "2026-01-01T00:00:00Z",
                "period_end": "2026-03-31T23:59:59Z",
            }),
        )
        .await;
    let invite_url = Url::parse(created["url"].as_str().expect("auditor URL is a string"))
        .expect("auditor URL parses");
    let invite_token = invite_token(&invite_url);

    let initial = app
        .app_server()
        .post(&format!(
            "/auditor-access/{workspace_id}/otp/request/browser"
        ))
        .form(&[("token", invite_token.as_str())])
        .await;
    initial.assert_status_ok();
    assert_verification_page(
        &initial.text(),
        workspace_id,
        &invite_token,
        auditor_email,
        Some("Code sent. Check the intended auditor inbox."),
        ResendControl::Available,
    );

    let sent = app.mailer().sent_mail_for(auditor_email);
    assert_eq!(sent.len(), 1);
    let previous_code = sent[0].code.clone();

    let resend = app
        .app_server()
        .post(&format!(
            "/auditor-access/{workspace_id}/otp/request/browser"
        ))
        .form(&[("token", invite_token.as_str()), ("resend", "true")])
        .await;
    resend.assert_status_ok();
    assert_verification_page(
        &resend.text(),
        workspace_id,
        &invite_token,
        auditor_email,
        None,
        ResendControl::Sent,
    );

    let sent = app.mailer().sent_mail_for(auditor_email);
    assert_eq!(sent.len(), 2);
    let newest_code = sent[1].code.clone();

    let previous = app
        .app_server()
        .post(&format!(
            "/auditor-access/{workspace_id}/otp/verify/browser"
        ))
        .form(&[
            ("token", invite_token.as_str()),
            ("code", previous_code.as_str()),
        ])
        .await;
    previous.assert_status_not_found();
    assert_verification_page(
        &previous.text(),
        workspace_id,
        &invite_token,
        auditor_email,
        Some("That code could not be verified. Request a new code if it expired."),
        ResendControl::Available,
    );

    let newest = app
        .app_server()
        .post(&format!(
            "/auditor-access/{workspace_id}/otp/verify/browser"
        ))
        .form(&[
            ("token", invite_token.as_str()),
            ("code", newest_code.as_str()),
        ])
        .await;
    newest.assert_status(StatusCode::SEE_OTHER);
    assert_eq!(newest.header(LOCATION), "/auditor-access/portal");
    let cookie = request_cookie(
        newest
            .headers()
            .get(SET_COOKIE)
            .expect("browser verification sets a session cookie")
            .to_str()
            .expect("session cookie is text"),
    );
    app.app_server()
        .get("/auditor-access/portal/data")
        .add_header("cookie", cookie)
        .await
        .assert_status_ok();
}
