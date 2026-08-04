use http::{header::SET_COOKIE, StatusCode};
use proofplane::{domain::WorkspacePermission, routes::request_context::REQUEST_ID_HEADER};
use serde_json::{json, Value};
use url::Url;

use crate::support::{
    auditor_access::{authenticate_auditor, invite_token},
    harness,
    mcp::McpClient,
    oauth::authorize_agent_connection,
    scenario::ScenarioBuilder,
};

use super::helpers::{
    assert_auth_failure_audit, assert_authentication_rejected_page, assert_portal_data_not_found,
    assert_unavailable_page,
};

#[tokio::test]
async fn mismatched_and_unverified_identities_are_rejected_without_sessions() {
    let app = harness::app().await;
    let owner_subject = "auth0|auditor-identity-rejection-owner";
    let auditor_email = "auditor-identity-rejection@example.com";

    let scenario = ScenarioBuilder::new(&app)
        .with_user(owner_subject)
        .with_workspace(owner_subject, "Auditor Identity Rejection")
        .build()
        .await;
    let workspace_id = scenario.workspace("Auditor Identity Rejection").id;
    let token = authorize_agent_connection(
        &app,
        owner_subject,
        "Auditor Identity Rejection Agent",
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
    let invitation_token = invite_token(
        &Url::parse(created["url"].as_str().expect("auditor URL is text"))
            .expect("auditor URL parses"),
    );

    let mismatched_start = app
        .app_server()
        .post(&format!("/auditor-access/{workspace_id}/login"))
        .form(&[("token", invitation_token.as_str())])
        .await;
    mismatched_start.assert_status(StatusCode::SEE_OTHER);
    let mismatched_state = Url::parse(
        mismatched_start
            .header("location")
            .to_str()
            .expect("authorization location is text"),
    )
    .expect("authorization URL parses")
    .query_pairs()
    .find_map(|(name, value)| (name == "state").then(|| value.into_owned()))
    .expect("authorization URL carries state");
    let mismatched_code = "auditor-mismatched-identity-code";
    let _mismatched_identity = app.auditor_identity_provider().verified(
        mismatched_code,
        "auth0|auditor-mismatched-identity",
        "someone-else@example.com",
    );
    let ((mismatched, mismatched_request_id), mismatched_logs) = app
        .capture_audit_logs(async |request_id| {
            let response = app
                .app_server()
                .get("/auditor-access/auth0/callback")
                .add_header(REQUEST_ID_HEADER, request_id.to_string())
                .add_query_param("code", mismatched_code)
                .add_query_param("state", &mismatched_state)
                .await;
            (response, request_id)
        })
        .await;
    mismatched.assert_status_bad_request();
    assert_authentication_rejected_page(&mismatched.text());
    assert_eq!(mismatched.headers().get_all(SET_COOKIE).iter().count(), 0);
    assert_eq!(mismatched_logs.len(), 1);
    assert_auth_failure_audit(&mismatched_logs[0], "rejected", mismatched_request_id);
    assert_eq!(
        app.auditor_identity_provider()
            .exchanges_for(mismatched_code)
            .len(),
        1
    );

    let unverified_start = app
        .app_server()
        .post(&format!("/auditor-access/{workspace_id}/login"))
        .form(&[("token", invitation_token.as_str())])
        .await;
    unverified_start.assert_status(StatusCode::SEE_OTHER);
    let unverified_state = Url::parse(
        unverified_start
            .header("location")
            .to_str()
            .expect("authorization location is text"),
    )
    .expect("authorization URL parses")
    .query_pairs()
    .find_map(|(name, value)| (name == "state").then(|| value.into_owned()))
    .expect("authorization URL carries state");
    let unverified_code = "auditor-unverified-identity-code";
    let _unverified_identity = app.auditor_identity_provider().unverified(
        unverified_code,
        "auth0|auditor-unverified-identity",
        auditor_email,
    );
    let ((unverified, unverified_request_id), unverified_logs) = app
        .capture_audit_logs(async |request_id| {
            let response = app
                .app_server()
                .get("/auditor-access/auth0/callback")
                .add_header(REQUEST_ID_HEADER, request_id.to_string())
                .add_query_param("code", unverified_code)
                .add_query_param("state", &unverified_state)
                .await;
            (response, request_id)
        })
        .await;
    unverified.assert_status_bad_request();
    assert_authentication_rejected_page(&unverified.text());
    assert_eq!(unverified.headers().get_all(SET_COOKIE).iter().count(), 0);
    assert_eq!(unverified_logs.len(), 1);
    assert_auth_failure_audit(&unverified_logs[0], "rejected", unverified_request_id);
    assert_eq!(
        app.auditor_identity_provider()
            .exchanges_for(unverified_code)
            .len(),
        1
    );

    assert_portal_data_not_found(app.app_server().get("/auditor-access/portal/data").await);
}

#[tokio::test]
async fn invalid_and_provider_error_callbacks_return_coarse_recovery_pages() {
    let app = harness::app().await;
    let owner_subject = "auth0|auditor-invalid-callback-owner";
    let auditor_email = "auditor-invalid-callback@example.com";

    let scenario = ScenarioBuilder::new(&app)
        .with_user(owner_subject)
        .with_workspace(owner_subject, "Auditor Invalid Callback")
        .build()
        .await;
    let workspace_id = scenario.workspace("Auditor Invalid Callback").id;
    let token = authorize_agent_connection(
        &app,
        owner_subject,
        "Auditor Invalid Callback Agent",
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
    let invitation_token = invite_token(
        &Url::parse(created["url"].as_str().expect("auditor URL is text"))
            .expect("auditor URL parses"),
    );

    let missing_code_start = app
        .app_server()
        .post(&format!("/auditor-access/{workspace_id}/login"))
        .form(&[("token", invitation_token.as_str())])
        .await;
    missing_code_start.assert_status(StatusCode::SEE_OTHER);
    let missing_code_state = Url::parse(
        missing_code_start
            .header("location")
            .to_str()
            .expect("authorization location is text"),
    )
    .expect("authorization URL parses")
    .query_pairs()
    .find_map(|(name, value)| (name == "state").then(|| value.into_owned()))
    .expect("authorization URL carries state");
    let ((missing_code, missing_code_request_id), missing_code_logs) = app
        .capture_audit_logs(async |request_id| {
            let response = app
                .app_server()
                .get("/auditor-access/auth0/callback")
                .add_header(REQUEST_ID_HEADER, request_id.to_string())
                .add_query_param("state", &missing_code_state)
                .await;
            (response, request_id)
        })
        .await;
    missing_code.assert_status_bad_request();
    assert_authentication_rejected_page(&missing_code.text());
    assert_eq!(missing_code.headers().get_all(SET_COOKIE).iter().count(), 0);
    assert_eq!(missing_code_logs.len(), 1);
    assert_auth_failure_audit(
        &missing_code_logs[0],
        "invalid_callback",
        missing_code_request_id,
    );

    let provider_error_start = app
        .app_server()
        .post(&format!("/auditor-access/{workspace_id}/login"))
        .form(&[("token", invitation_token.as_str())])
        .await;
    provider_error_start.assert_status(StatusCode::SEE_OTHER);
    let provider_error_state = Url::parse(
        provider_error_start
            .header("location")
            .to_str()
            .expect("authorization location is text"),
    )
    .expect("authorization URL parses")
    .query_pairs()
    .find_map(|(name, value)| (name == "state").then(|| value.into_owned()))
    .expect("authorization URL carries state");
    let ((provider_error, provider_error_request_id), provider_error_logs) = app
        .capture_audit_logs(async |request_id| {
            let response = app
                .app_server()
                .get("/auditor-access/auth0/callback")
                .add_header(REQUEST_ID_HEADER, request_id.to_string())
                .add_query_param("error", "access_denied")
                .add_query_param("error_description", "upstream details must stay hidden")
                .add_query_param("state", &provider_error_state)
                .await;
            (response, request_id)
        })
        .await;
    provider_error.assert_status_bad_request();
    assert_authentication_rejected_page(&provider_error.text());
    assert_eq!(
        provider_error.headers().get_all(SET_COOKIE).iter().count(),
        0
    );
    assert_eq!(provider_error_logs.len(), 1);
    assert_auth_failure_audit(
        &provider_error_logs[0],
        "provider_rejected",
        provider_error_request_id,
    );
}

#[tokio::test]
async fn logout_and_mcp_grant_revocation_invalidate_auth0_sessions() {
    let app = harness::app().await;
    let owner_subject = "auth0|auditor-session-invalidation-owner";
    let auditor_email = "auditor-session-invalidation@example.com";

    let scenario = ScenarioBuilder::new(&app)
        .with_user(owner_subject)
        .with_workspace(owner_subject, "Auditor Session Invalidation")
        .build()
        .await;
    let workspace_id = scenario.workspace("Auditor Session Invalidation").id;
    let token = authorize_agent_connection(
        &app,
        owner_subject,
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
                "period_start": "2027-04-01T00:00:00Z",
                "period_end": "2027-06-30T23:59:59Z",
            }),
        )
        .await;
    let grant_id = created["grant"]["id"]
        .as_str()
        .expect("grant id is a string");
    let invitation_token = invite_token(
        &Url::parse(created["url"].as_str().expect("auditor URL is text"))
            .expect("auditor URL parses"),
    );

    let first_cookie = authenticate_auditor(
        &app,
        workspace_id,
        &invitation_token,
        "auth0|auditor-session-invalidation-first",
        auditor_email,
    )
    .await;
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

    let second_cookie = authenticate_auditor(
        &app,
        workspace_id,
        &invitation_token,
        "auth0|auditor-session-invalidation-second",
        auditor_email,
    )
    .await;
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

#[tokio::test]
async fn revoked_grant_and_concurrent_callbacks_create_at_most_one_session() {
    let app = harness::app().await;
    let owner_subject = "auth0|auditor-callback-race-owner";
    let auditor_email = "auditor-callback-race@example.com";

    let scenario = ScenarioBuilder::new(&app)
        .with_user(owner_subject)
        .with_workspace(owner_subject, "Auditor Callback Race")
        .build()
        .await;
    let workspace_id = scenario.workspace("Auditor Callback Race").id;
    let token = authorize_agent_connection(
        &app,
        owner_subject,
        "Auditor Callback Race Agent",
        &[WorkspacePermission::ManageAuditorAccess],
    )
    .await;
    let client = McpClient::connect(app.mcp_server(), &token).await;
    let revoked = client
        .call_tool(
            "create_auditor_access_link",
            json!({
                "email": auditor_email,
                "expires_at": "2099-01-01T00:00:00Z",
                "period_start": "2027-07-01T00:00:00Z",
                "period_end": "2027-09-30T23:59:59Z",
            }),
        )
        .await;
    let revoked_grant_id = revoked["grant"]["id"].as_str().expect("grant id is text");
    let revoked_token = invite_token(
        &Url::parse(revoked["url"].as_str().expect("auditor URL is text"))
            .expect("auditor URL parses"),
    );
    let revoked_start = app
        .app_server()
        .post(&format!("/auditor-access/{workspace_id}/login"))
        .form(&[("token", revoked_token.as_str())])
        .await;
    revoked_start.assert_status(StatusCode::SEE_OTHER);
    let revoked_state = Url::parse(
        revoked_start
            .header("location")
            .to_str()
            .expect("authorization location is text"),
    )
    .expect("authorization URL parses")
    .query_pairs()
    .find_map(|(name, value)| (name == "state").then(|| value.into_owned()))
    .expect("authorization URL carries state");
    client
        .call_tool(
            "revoke_auditor_access_link",
            json!({ "grant_id": revoked_grant_id }),
        )
        .await;
    let revoked_code = "auditor-revoked-grant-code";
    let _revoked_identity = app.auditor_identity_provider().verified(
        revoked_code,
        "auth0|auditor-revoked-grant-identity",
        auditor_email,
    );
    let revoked_callback = app
        .app_server()
        .get("/auditor-access/auth0/callback")
        .add_query_param("code", revoked_code)
        .add_query_param("state", &revoked_state)
        .await;
    revoked_callback.assert_status_not_found();
    assert_unavailable_page(&revoked_callback.text());
    assert_eq!(
        revoked_callback
            .headers()
            .get_all(SET_COOKIE)
            .iter()
            .count(),
        0
    );

    let active = client
        .call_tool(
            "create_auditor_access_link",
            json!({
                "email": auditor_email,
                "expires_at": "2099-01-01T00:00:00Z",
                "period_start": "2027-10-01T00:00:00Z",
                "period_end": "2027-12-31T23:59:59Z",
            }),
        )
        .await;
    let active_token = invite_token(
        &Url::parse(active["url"].as_str().expect("auditor URL is text"))
            .expect("auditor URL parses"),
    );
    let race_start = app
        .app_server()
        .post(&format!("/auditor-access/{workspace_id}/login"))
        .form(&[("token", active_token.as_str())])
        .await;
    race_start.assert_status(StatusCode::SEE_OTHER);
    let race_state = Url::parse(
        race_start
            .header("location")
            .to_str()
            .expect("authorization location is text"),
    )
    .expect("authorization URL parses")
    .query_pairs()
    .find_map(|(name, value)| (name == "state").then(|| value.into_owned()))
    .expect("authorization URL carries state");
    let race_code = "auditor-concurrent-callback-code";
    let _race_identity = app.auditor_identity_provider().verified(
        race_code,
        "auth0|auditor-concurrent-callback-identity",
        auditor_email,
    );

    let first = app
        .app_server()
        .get("/auditor-access/auth0/callback")
        .add_query_param("code", race_code)
        .add_query_param("state", &race_state);
    let second = app
        .app_server()
        .get("/auditor-access/auth0/callback")
        .add_query_param("code", race_code)
        .add_query_param("state", &race_state);
    let (first, second) = tokio::join!(first, second);
    let responses = [first, second];
    assert_eq!(
        responses
            .iter()
            .filter(|response| response.status_code() == StatusCode::SEE_OTHER)
            .count(),
        1
    );
    assert_eq!(
        responses
            .iter()
            .filter(|response| response.status_code() == StatusCode::BAD_REQUEST)
            .count(),
        1
    );
    let cookies = responses
        .iter()
        .flat_map(|response| response.headers().get_all(SET_COOKIE).iter())
        .collect::<Vec<_>>();
    assert_eq!(cookies.len(), 1);
    let usable_cookie = cookies[0]
        .to_str()
        .expect("winning session cookie is text")
        .split(';')
        .next()
        .expect("winning session cookie has a value");
    app.app_server()
        .get("/auditor-access/portal/data")
        .add_header("cookie", usable_cookie)
        .await
        .assert_status_ok();
    assert_eq!(
        app.auditor_identity_provider()
            .exchanges_for(race_code)
            .len(),
        1
    );
}
