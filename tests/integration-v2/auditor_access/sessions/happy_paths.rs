use std::collections::{BTreeMap, BTreeSet};

use http::{header::SET_COOKIE, StatusCode};
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

use super::helpers::{
    assert_auth_completed_audit, assert_auth_failure_audit, assert_auth_started_audit,
    assert_authentication_rejected_page, assert_not_found_json, assert_session_created_audit,
};

#[tokio::test]
async fn hosted_login_creates_one_scoped_session_and_complete_secret_free_audits() {
    let app = harness::app().await;
    let owner_subject = "auth0|auditor-session-success-owner";
    let auth0_subject = "auth0|auditor-session-success-identity";
    let auditor_email = "auditor-session-success@example.com";
    let authorization_code = "auditor-session-success-code";

    let scenario = ScenarioBuilder::new(&app)
        .with_user(owner_subject)
        .with_workspace(owner_subject, "Auditor Session Success")
        .build()
        .await;
    let workspace_id = scenario.workspace("Auditor Session Success").id;

    let token = authorize_agent_connection(
        &app,
        owner_subject,
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
    let invitation_token = invite_token(&invite_url);
    assert_eq!(invitation_token.len(), TOKEN_LENGTH);
    assert!(invitation_token.starts_with(PREFIX));
    assert!(invitation_token[PREFIX.len()..]
        .bytes()
        .all(|byte| ALPHABET.contains(&byte)));

    let ((started, start_request_id), start_logs) = app
        .capture_audit_logs(async |request_id| {
            let response = app
                .app_server()
                .post(&format!("/auditor-access/{workspace_id}/login"))
                .add_header(REQUEST_ID_HEADER, request_id.to_string())
                .form(&[("token", invitation_token.as_str())])
                .await;
            (response, request_id)
        })
        .await;
    started.assert_status(StatusCode::SEE_OTHER);
    let authorization_url = Url::parse(
        started
            .header("location")
            .to_str()
            .expect("authorization redirect is text"),
    )
    .expect("authorization redirect parses");
    assert_eq!(authorization_url.scheme(), "https");
    assert_eq!(authorization_url.host_str(), Some("auth.proofplane.test"));
    assert_eq!(authorization_url.path(), "/authorize");
    let query_pairs = authorization_url
        .query_pairs()
        .map(|(name, value)| (name.into_owned(), value.into_owned()))
        .collect::<Vec<_>>();
    assert_eq!(query_pairs.len(), 11);
    let query = query_pairs.into_iter().collect::<BTreeMap<_, _>>();
    assert_eq!(
        query.keys().map(String::as_str).collect::<BTreeSet<_>>(),
        [
            "client_id",
            "code_challenge",
            "code_challenge_method",
            "connection",
            "login_hint",
            "nonce",
            "prompt",
            "redirect_uri",
            "response_type",
            "scope",
            "state",
        ]
        .into_iter()
        .collect()
    );
    assert_eq!(query["client_id"], "integration-auditor-client");
    assert_eq!(
        query["redirect_uri"],
        "https://api.proofplane.test/auditor-access/auth0/callback"
    );
    assert_eq!(query["response_type"], "code");
    assert_eq!(query["scope"], "openid email");
    assert_eq!(query["code_challenge_method"], "S256");
    assert_eq!(query["connection"], "email");
    assert_eq!(query["login_hint"], auditor_email);
    assert_eq!(query["prompt"], "login");
    for generated in [&query["state"], &query["nonce"], &query["code_challenge"]] {
        assert_eq!(generated.len(), 43);
        assert!(generated
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_')));
    }

    assert_eq!(start_logs.len(), 1);
    let transaction_id =
        assert_auth_started_audit(&start_logs[0], workspace_id, grant_id, start_request_id);

    let _identity =
        app.auditor_identity_provider()
            .verified(authorization_code, auth0_subject, auditor_email);
    let ((callback, callback_request_id), callback_logs) = app
        .capture_audit_logs(async |request_id| {
            let response = app
                .app_server()
                .get("/auditor-access/auth0/callback")
                .add_header(REQUEST_ID_HEADER, request_id.to_string())
                .add_query_param("code", authorization_code)
                .add_query_param("state", &query["state"])
                .await;
            (response, request_id)
        })
        .await;
    callback.assert_status(StatusCode::SEE_OTHER);
    assert_eq!(callback.header("location"), "/auditor-access/portal");
    let set_cookie = callback
        .headers()
        .get(SET_COOKIE)
        .expect("callback sets a session cookie")
        .to_str()
        .expect("session cookie is text");
    let cookie_parts = set_cookie.split("; ").collect::<Vec<_>>();
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

    let exchanges = app
        .auditor_identity_provider()
        .exchanges_for(authorization_code);
    assert_eq!(exchanges.len(), 1);
    assert_eq!(exchanges[0].authorization_code, authorization_code);
    assert_eq!(
        exchanges[0].redirect_uri.as_str(),
        "https://api.proofplane.test/auditor-access/auth0/callback"
    );
    assert_eq!(exchanges[0].pkce_verifier.len(), 43);
    assert!(exchanges[0]
        .pkce_verifier
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_')));
    assert_eq!(exchanges[0].expected_nonce_digest.as_bytes().len(), 32);

    assert_eq!(callback_logs.len(), 2);
    assert_auth_completed_audit(
        &callback_logs[0],
        workspace_id,
        transaction_id,
        auth0_subject,
        callback_request_id,
    );
    assert_session_created_audit(
        &callback_logs[1],
        workspace_id,
        auth0_subject,
        callback_request_id,
    );

    let cookie = request_cookie(set_cookie);
    app.app_server()
        .get("/auditor-access/portal/data")
        .add_header("cookie", cookie)
        .await
        .assert_status_ok();

    let ((replay, replay_request_id), replay_logs) = app
        .capture_audit_logs(async |request_id| {
            let response = app
                .app_server()
                .get("/auditor-access/auth0/callback")
                .add_header(REQUEST_ID_HEADER, request_id.to_string())
                .add_query_param("code", authorization_code)
                .add_query_param("state", &query["state"])
                .await;
            (response, request_id)
        })
        .await;
    replay.assert_status_bad_request();
    assert_authentication_rejected_page(&replay.text());
    assert_eq!(replay.headers().get_all(SET_COOKIE).iter().count(), 0);
    assert_eq!(replay_logs.len(), 1);
    assert_auth_failure_audit(&replay_logs[0], "rejected", replay_request_id);
    assert_eq!(
        app.auditor_identity_provider()
            .exchanges_for(authorization_code)
            .len(),
        1
    );
}

#[tokio::test]
async fn retired_otp_endpoints_return_the_complete_not_found_response() {
    let app = harness::app().await;
    let workspace_id = Uuid::new_v4();

    for path in [
        format!("/auditor-access/{workspace_id}/otp/request"),
        format!("/auditor-access/{workspace_id}/otp/verify"),
        format!("/auditor-access/{workspace_id}/otp/request/browser"),
        format!("/auditor-access/{workspace_id}/otp/verify/browser"),
    ] {
        let response = app.app_server().post(&path).json(&json!({})).await;
        response.assert_status_not_found();
        assert_not_found_json(&response);
    }
}
