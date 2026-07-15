use axum::http::{header, StatusCode};
use bytes::Bytes;
use chrono::{Duration, Utc};
use proofplane::domain::{
    NewOAuthAuthorizationRequest, OAuthAuthorizationRequestId, WorkspacePermission,
};
use uuid::Uuid;

use super::support::TestApp;

#[tokio::test]
async fn consent_cancellation_consumes_request_and_preserves_state() {
    let app = TestApp::start_without_default_auth().await;
    let request_id = seeded_request(&app, "auth0|oauth-cancel", "Cancel Client").await;

    let response = submit(&app, request_id, "cancel").await;
    response.assert_status(StatusCode::SEE_OTHER);
    let location = response.header(header::LOCATION);
    let location = url::Url::parse(location.to_str().expect("location is text"))
        .expect("redirect location is a URL");
    assert_eq!(location.path(), "/callback");
    assert_eq!(
        location
            .query_pairs()
            .collect::<std::collections::HashMap<_, _>>(),
        std::collections::HashMap::from([
            ("error".into(), "access_denied".into()),
            ("state".into(), "original-state".into()),
        ])
    );

    let replay = submit(&app, request_id, "cancel").await;
    replay.assert_status_ok();
    assert!(replay.text().contains("Return to your client"));
    assert!(!replay.text().contains("Cancel Client"));
}

#[tokio::test]
async fn consent_approval_authorizes_once_and_replay_is_generic() {
    let app = TestApp::start_without_default_auth().await;
    let request_id = seeded_request(&app, "auth0|oauth-approve", "Approval Client").await;

    let response = submit(&app, request_id, "approve").await;
    response.assert_status(StatusCode::SEE_OTHER);
    let location_header = response.header(header::LOCATION);
    let location = url::Url::parse(location_header.to_str().expect("location is text"))
        .expect("approval redirect is a URL");
    assert!(location.query_pairs().any(|(key, _)| key == "code"));
    assert!(location
        .query_pairs()
        .any(|(key, value)| key == "state" && value == "original-state"));

    let db = app.postgres().get().await.expect("database opens");
    let status: String = db
        .query_one(
            "SELECT status FROM agent_connections WHERE auth0_client_id = $1",
            &[&format!("oauth-client-{request_id}")],
        )
        .await
        .expect("authorized connection exists")
        .get("status");
    assert_eq!(status, "authorized");

    let replay = submit(&app, request_id, "approve").await;
    replay.assert_status_ok();
    assert!(replay.text().contains("Return to your client"));
    assert!(!replay.text().contains("Approval Client"));
}

#[tokio::test]
async fn expired_invalid_and_membershipless_consent_reveal_only_recovery_guidance() {
    let app = TestApp::start_without_default_auth().await;
    let request_id = seeded_request(&app, "auth0|oauth-membership", "Sensitive Client").await;
    let db = app.postgres().get().await.expect("database opens");
    db.execute(
        "DELETE FROM workspace_memberships WHERE user_id = (SELECT user_id FROM oauth_authorization_requests WHERE id = $1)",
        &[&Uuid::from(request_id)],
    )
    .await
    .expect("membership removes");

    let membershipless = submit(&app, request_id, "approve").await;
    assert_generic_recovery(&membershipless, "Sensitive Client");

    let expired = seeded_request(&app, "auth0|oauth-expired", "Expired Client").await;
    db.execute(
        "UPDATE oauth_authorization_requests SET created_at = now() - interval '2 hours', expires_at = now() - interval '1 hour' WHERE id = $1",
        &[&Uuid::from(expired)],
    )
    .await
    .expect("request expires");
    let expired_response = submit(&app, expired, "approve").await;
    assert_generic_recovery(&expired_response, "Expired Client");

    let invalid = app
        .server()
        .post("/oauth/consent")
        .add_header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .bytes(Bytes::from_static(
            b"request_id=not-a-uuid&decision=approve",
        ))
        .await;
    assert_generic_recovery(&invalid, "not-a-uuid");
}

#[tokio::test]
async fn authorization_server_metadata_advertises_cimd_and_drops_dcr() {
    let app = TestApp::start_without_default_auth().await;

    let response = app
        .server()
        .get("/.well-known/oauth-authorization-server")
        .await;
    response.assert_status_ok();
    let metadata = response.json::<serde_json::Value>();
    assert_eq!(
        metadata["client_id_metadata_document_supported"],
        serde_json::json!(true)
    );
    assert_eq!(
        metadata["token_endpoint_auth_methods_supported"],
        serde_json::json!(["none"])
    );
    assert!(
        metadata.get("registration_endpoint").is_none(),
        "registration endpoint must not be advertised once DCR is removed"
    );

    // The dynamic client registration endpoint is gone entirely.
    let register = app
        .server()
        .post("/oauth/register")
        .json(&serde_json::json!({ "redirect_uris": ["https://client.example/cb"] }))
        .await;
    register.assert_status(StatusCode::NOT_FOUND);
}

async fn seeded_request(
    app: &TestApp,
    subject: &str,
    client_name: &str,
) -> OAuthAuthorizationRequestId {
    let user_id = app.login(subject).await;
    app.create_workspace_as(subject, &format!("Workspace {client_name}"))
        .await;
    let request_id = OAuthAuthorizationRequestId::from(Uuid::new_v4());
    let client_id = format!("oauth-client-{request_id}");
    app.postgres()
        .create_oauth_authorization_request(&NewOAuthAuthorizationRequest {
            id: request_id,
            client_id,
            client_name: client_name.to_owned(),
            redirect_uri: "https://client.example/callback".to_owned(),
            code_challenge: "challenge".to_owned(),
            state: "original-state".to_owned(),
            resource: "http://127.0.0.1:3002/mcp".to_owned(),
            scopes: vec![WorkspacePermission::ReadControls],
            csrf_token: format!("csrf-{request_id}"),
            expires_at: Utc::now() + Duration::minutes(10),
        })
        .await
        .expect("OAuth request creates");
    app.postgres()
        .attach_oauth_authorization_subject(request_id, subject, user_id.into())
        .await
        .expect("OAuth subject attachment resolves")
        .expect("OAuth request attaches");
    request_id
}

async fn submit(
    app: &TestApp,
    request_id: OAuthAuthorizationRequestId,
    decision: &str,
) -> axum_test::TestResponse {
    app.server()
        .post("/oauth/consent")
        .add_header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .bytes(Bytes::from(format!(
            "request_id={}&decision={decision}",
            Uuid::from(request_id)
        )))
        .await
}

fn assert_generic_recovery(response: &axum_test::TestResponse, secret: &str) {
    response.assert_status_ok();
    let body = response.text();
    assert!(body.contains("Return to your client"));
    assert!(!body.contains(secret));
    assert!(!body.contains("workspace"));
    assert!(!body.contains("read_controls"));
}
