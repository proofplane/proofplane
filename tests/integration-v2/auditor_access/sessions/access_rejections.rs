use http::{
    header::{LOCATION, SET_COOKIE},
    StatusCode,
};
use proofplane::{domain::WorkspacePermission, routes::request_context::REQUEST_ID_HEADER};
use serde_json::{json, Value};
use url::Url;

use crate::support::{
    auditor_access::invite_token, harness, http::request_cookie, mcp::McpClient,
    oauth::authorize_agent_connection, scenario::ScenarioBuilder,
};

use super::helpers::{
    assert_portal_data_not_found, assert_unavailable_page, assert_verification_page, wrong_otp,
    ResendControl,
};

#[tokio::test]
async fn rejected_browser_requests_show_complete_recovery_without_session_audits() {
    let app = harness::app().await;
    let owner_subject = "auth0|auditor-browser-rejection-owner";
    let foreign_subject = "auth0|auditor-browser-rejection-foreign";
    let auditor_email = "auditor-browser-rejection@example.com";

    let scenario = ScenarioBuilder::new(&app)
        .with_user(owner_subject)
        .with_workspace(owner_subject, "Auditor Browser Rejection Owner")
        .with_user(foreign_subject)
        .with_workspace(foreign_subject, "Auditor Browser Rejection Foreign")
        .build()
        .await;
    let owner_workspace_id = scenario.workspace("Auditor Browser Rejection Owner").id;
    let foreign_workspace_id = scenario.workspace("Auditor Browser Rejection Foreign").id;

    let token = authorize_agent_connection(
        &app,
        owner_subject,
        "Auditor Browser Rejection Agent",
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
                "period_start": "2026-10-01T00:00:00Z",
                "period_end": "2026-12-31T23:59:59Z",
            }),
        )
        .await;
    let invite_url = Url::parse(created["url"].as_str().expect("auditor URL is a string"))
        .expect("auditor URL parses");
    let invite_token = invite_token(&invite_url);

    app.app_server()
        .post(&format!(
            "/auditor-access/{owner_workspace_id}/otp/request/browser"
        ))
        .form(&[("token", invite_token.as_str())])
        .await
        .assert_status_ok();
    let delivered = app.mailer().sent_mail_for(auditor_email);
    assert_eq!(delivered.len(), 1);
    let wrong_code = wrong_otp(&delivered[0].code);

    let ((wrong, malformed, cross_workspace), logs) = app
        .capture_audit_logs(async |request_id| {
            let wrong = app
                .app_server()
                .post(&format!(
                    "/auditor-access/{owner_workspace_id}/otp/verify/browser"
                ))
                .add_header(REQUEST_ID_HEADER, request_id.to_string())
                .form(&[("token", invite_token.as_str()), ("code", wrong_code)])
                .await;
            let malformed = app
                .app_server()
                .post(&format!(
                    "/auditor-access/{owner_workspace_id}/otp/request/browser"
                ))
                .add_header(REQUEST_ID_HEADER, request_id.to_string())
                .form(&[("token", "not-a-token")])
                .await;
            let cross_workspace = app
                .app_server()
                .post(&format!(
                    "/auditor-access/{foreign_workspace_id}/otp/verify/browser"
                ))
                .add_header(REQUEST_ID_HEADER, request_id.to_string())
                .form(&[("token", invite_token.as_str()), ("code", wrong_code)])
                .await;
            (wrong, malformed, cross_workspace)
        })
        .await;

    wrong.assert_status_not_found();
    assert_verification_page(
        &wrong.text(),
        owner_workspace_id,
        &invite_token,
        auditor_email,
        Some("That code could not be verified. Request a new code if it expired."),
        ResendControl::Available,
    );

    malformed.assert_status_not_found();
    assert_unavailable_page(&malformed.text());
    cross_workspace.assert_status_not_found();
    assert_unavailable_page(&cross_workspace.text());
    assert!(logs.is_empty());
}

#[tokio::test]
async fn logout_and_mcp_grant_revocation_invalidate_existing_sessions() {
    let app = harness::app().await;
    let subject = "auth0|auditor-session-invalidation";
    let auditor_email = "auditor-session-invalidation@example.com";

    let scenario = ScenarioBuilder::new(&app)
        .with_user(subject)
        .with_workspace(subject, "Auditor Session Invalidation")
        .build()
        .await;
    let workspace_id = scenario.workspace("Auditor Session Invalidation").id;

    let token = authorize_agent_connection(
        &app,
        subject,
        "Auditor Session Invalidation Agent",
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
                "period_start": "2027-01-01T00:00:00Z",
                "period_end": "2027-03-31T23:59:59Z",
            }),
        )
        .await;
    let grant_id = created["grant"]["id"]
        .as_str()
        .expect("grant id is a string");
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
    let first_code = app.mailer().sent_mail_for(auditor_email)[0].code.clone();
    let first_verified = app
        .app_server()
        .post(&format!(
            "/auditor-access/{workspace_id}/otp/verify/browser"
        ))
        .form(&[
            ("token", invite_token.as_str()),
            ("code", first_code.as_str()),
        ])
        .await;
    first_verified.assert_status(StatusCode::SEE_OTHER);
    assert_eq!(first_verified.header(LOCATION), "/auditor-access/portal");
    let first_cookie = request_cookie(
        first_verified
            .headers()
            .get(SET_COOKIE)
            .expect("first verification sets a session cookie")
            .to_str()
            .expect("session cookie is text"),
    );
    app.app_server()
        .get("/auditor-access/portal/data")
        .add_header("cookie", first_cookie.clone())
        .await
        .assert_status_ok();

    let logout = app
        .app_server()
        .post("/auditor-access/logout")
        .add_header("cookie", first_cookie.clone())
        .await;
    logout.assert_status_ok();
    assert_eq!(logout.json::<Value>(), json!({ "status": "logged_out" }));
    assert_eq!(
        logout
            .headers()
            .get(SET_COOKIE)
            .expect("logout clears the session cookie")
            .to_str()
            .expect("clearing cookie is text"),
        "proofplane_auditor_session=; HttpOnly; SameSite=Lax; Path=/auditor-access; Max-Age=0"
    );
    assert_portal_data_not_found(
        app.app_server()
            .get("/auditor-access/portal/data")
            .add_header("cookie", first_cookie)
            .await,
    );

    app.app_server()
        .post(&format!(
            "/auditor-access/{workspace_id}/otp/request/browser"
        ))
        .form(&[("token", invite_token.as_str()), ("resend", "true")])
        .await
        .assert_status_ok();
    let sent = app.mailer().sent_mail_for(auditor_email);
    assert_eq!(sent.len(), 2);
    let second_code = sent[1].code.clone();
    let second_verified = app
        .app_server()
        .post(&format!(
            "/auditor-access/{workspace_id}/otp/verify/browser"
        ))
        .form(&[
            ("token", invite_token.as_str()),
            ("code", second_code.as_str()),
        ])
        .await;
    second_verified.assert_status(StatusCode::SEE_OTHER);
    assert_eq!(second_verified.header(LOCATION), "/auditor-access/portal");
    let second_cookie = request_cookie(
        second_verified
            .headers()
            .get(SET_COOKIE)
            .expect("second verification sets a session cookie")
            .to_str()
            .expect("session cookie is text"),
    );
    app.app_server()
        .get("/auditor-access/portal/data")
        .add_header("cookie", second_cookie.clone())
        .await
        .assert_status_ok();

    client
        .call_tool(
            "revoke_auditor_access_link",
            json!({ "grant_id": grant_id }),
        )
        .await;
    assert_portal_data_not_found(
        app.app_server()
            .get("/auditor-access/portal/data")
            .add_header("cookie", second_cookie)
            .await,
    );
}
