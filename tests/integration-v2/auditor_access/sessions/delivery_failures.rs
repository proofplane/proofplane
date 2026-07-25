use http::{
    header::{LOCATION, SET_COOKIE},
    StatusCode,
};
use proofplane::domain::WorkspacePermission;
use serde_json::{json, Value};
use url::Url;

use crate::support::{
    auditor_access::invite_token, harness, http::request_cookie, mcp::McpClient,
    oauth::authorize_agent_connection, scenario::ScenarioBuilder,
};

use super::helpers::{assert_initial_send_failure_page, assert_verification_page, ResendControl};

#[tokio::test]
async fn initial_mail_failures_are_retryable_without_consuming_send_budget() {
    let app = harness::app().await;
    let subject = "auth0|auditor-session-mail-failure";
    let auditor_email = "auditor-session-mail-failure@example.com";

    let scenario = ScenarioBuilder::new(&app)
        .with_user(subject)
        .with_workspace(subject, "Auditor Session Mail Failure")
        .build()
        .await;
    let workspace_id = scenario.workspace("Auditor Session Mail Failure").id;

    let token = authorize_agent_connection(
        &app,
        subject,
        "Auditor Session Mail Failure Agent",
        &[WorkspacePermission::ManageAuditorAccess],
    )
    .await;
    let client = McpClient::connect(app.mcp_server(), &token).await;
    let created = client
        .call_tool(
            "create_auditor_access_link",
            json!({
                "email": auditor_email,
                "period_start": "2026-04-01T00:00:00Z",
                "period_end": "2026-06-30T23:59:59Z",
            }),
        )
        .await;
    let invite_url = Url::parse(created["url"].as_str().expect("auditor URL is a string"))
        .expect("auditor URL parses");
    assert_eq!(invite_url.path(), format!("/auditor-access/{workspace_id}"));
    let invite_token = invite_token(&invite_url);

    app.app_server()
        .post(&format!("/auditor-access/{workspace_id}/otp/request"))
        .json(&json!({ "token": invite_token }))
        .await
        .assert_status_ok();

    let delivered = app.mailer().sent_mail_for(auditor_email);
    assert_eq!(delivered.len(), 1);
    let delivered_code = delivered[0].code.clone();

    let failure = app.mailer().fail_delivery_for(auditor_email);
    let api_failure = app
        .app_server()
        .post(&format!("/auditor-access/{workspace_id}/otp/request"))
        .json(&json!({ "token": invite_token }))
        .await;
    api_failure.assert_status(StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        api_failure.json::<Value>(),
        json!({
            "error": {
                "code": "mail_unavailable",
                "message": "verification email could not be sent; try again",
                "details": [],
            }
        })
    );

    let browser_failure = app
        .app_server()
        .post(&format!(
            "/auditor-access/{workspace_id}/otp/request/browser"
        ))
        .form(&[("token", invite_token.as_str())])
        .await;
    browser_failure.assert_status(StatusCode::SERVICE_UNAVAILABLE);
    assert_initial_send_failure_page(
        &browser_failure.text(),
        workspace_id,
        &invite_token,
        auditor_email,
    );
    assert_eq!(app.mailer().sent_mail_for(auditor_email), delivered);

    drop(failure);
    app.app_server()
        .post(&format!("/auditor-access/{workspace_id}/otp/verify"))
        .json(&json!({
            "token": invite_token,
            "code": delivered_code,
        }))
        .await
        .assert_status_ok();

    for _ in 0..2 {
        app.app_server()
            .post(&format!("/auditor-access/{workspace_id}/otp/request"))
            .json(&json!({ "token": invite_token }))
            .await
            .assert_status_ok();
    }
    assert_eq!(app.mailer().sent_mail_for(auditor_email).len(), 3);

    let rate_limited = app
        .app_server()
        .post(&format!("/auditor-access/{workspace_id}/otp/request"))
        .json(&json!({ "token": invite_token }))
        .await;
    rate_limited.assert_status(StatusCode::CONFLICT);
    assert_eq!(
        rate_limited.json::<Value>(),
        json!({
            "error": {
                "code": "auditor_otp_rate_limited",
                "message": "too many OTP requests",
                "details": [],
            }
        })
    );
    assert_eq!(app.mailer().sent_mail_for(auditor_email).len(), 3);
}

#[tokio::test]
async fn browser_resend_rate_limit_preserves_complete_verification_controls() {
    let app = harness::app().await;
    let subject = "auth0|auditor-browser-resend-limit";
    let auditor_email = "auditor-browser-resend-limit@example.com";

    let scenario = ScenarioBuilder::new(&app)
        .with_user(subject)
        .with_workspace(subject, "Auditor Browser Resend Limit")
        .build()
        .await;
    let workspace_id = scenario.workspace("Auditor Browser Resend Limit").id;

    let token = authorize_agent_connection(
        &app,
        subject,
        "Auditor Browser Resend Limit Agent",
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
                "period_start": "2026-04-01T00:00:00Z",
                "period_end": "2026-06-30T23:59:59Z",
            }),
        )
        .await;
    let invite_url = Url::parse(created["url"].as_str().expect("auditor URL is a string"))
        .expect("auditor URL parses");
    let invite_token = invite_token(&invite_url);

    for resend in [false, true, true] {
        let form = if resend {
            vec![("token", invite_token.as_str()), ("resend", "true")]
        } else {
            vec![("token", invite_token.as_str())]
        };
        app.app_server()
            .post(&format!(
                "/auditor-access/{workspace_id}/otp/request/browser"
            ))
            .form(&form)
            .await
            .assert_status_ok();
    }
    assert_eq!(app.mailer().sent_mail_for(auditor_email).len(), 3);

    let limited = app
        .app_server()
        .post(&format!(
            "/auditor-access/{workspace_id}/otp/request/browser"
        ))
        .form(&[("token", invite_token.as_str()), ("resend", "true")])
        .await;
    limited.assert_status(StatusCode::CONFLICT);
    assert_eq!(app.mailer().sent_mail_for(auditor_email).len(), 3);
    assert_verification_page(
        &limited.text(),
        workspace_id,
        &invite_token,
        auditor_email,
        Some("Too many code requests. Use the latest code or wait before trying again."),
        ResendControl::Available,
    );
}

#[tokio::test]
async fn failed_browser_resend_keeps_the_previous_code_usable() {
    let app = harness::app().await;
    let subject = "auth0|auditor-browser-resend-failure";
    let auditor_email = "auditor-browser-resend-failure@example.com";

    let scenario = ScenarioBuilder::new(&app)
        .with_user(subject)
        .with_workspace(subject, "Auditor Browser Resend Failure")
        .build()
        .await;
    let workspace_id = scenario.workspace("Auditor Browser Resend Failure").id;

    let token = authorize_agent_connection(
        &app,
        subject,
        "Auditor Browser Resend Failure Agent",
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
                "period_start": "2026-07-01T00:00:00Z",
                "period_end": "2026-09-30T23:59:59Z",
            }),
        )
        .await;
    let invite_url = Url::parse(created["url"].as_str().expect("auditor URL is a string"))
        .expect("auditor URL parses");
    let invite_token = invite_token(&invite_url);

    app.app_server()
        .post(&format!(
            "/auditor-access/{workspace_id}/otp/request/browser"
        ))
        .form(&[("token", invite_token.as_str())])
        .await
        .assert_status_ok();
    let delivered = app.mailer().sent_mail_for(auditor_email);
    assert_eq!(delivered.len(), 1);
    let previous_code = delivered[0].code.clone();

    let failure = app.mailer().fail_delivery_for(auditor_email);
    let failed = app
        .app_server()
        .post(&format!(
            "/auditor-access/{workspace_id}/otp/request/browser"
        ))
        .form(&[("token", invite_token.as_str()), ("resend", "true")])
        .await;
    failed.assert_status(StatusCode::SERVICE_UNAVAILABLE);
    assert_verification_page(
        &failed.text(),
        workspace_id,
        &invite_token,
        auditor_email,
        Some("We couldn't send a new code. Your previous code may still work."),
        ResendControl::Available,
    );
    assert_eq!(app.mailer().sent_mail_for(auditor_email), delivered);

    drop(failure);
    let verified = app
        .app_server()
        .post(&format!(
            "/auditor-access/{workspace_id}/otp/verify/browser"
        ))
        .form(&[
            ("token", invite_token.as_str()),
            ("code", previous_code.as_str()),
        ])
        .await;
    verified.assert_status(StatusCode::SEE_OTHER);
    assert_eq!(verified.header(LOCATION), "/auditor-access/portal");
    let cookie = request_cookie(
        verified
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
