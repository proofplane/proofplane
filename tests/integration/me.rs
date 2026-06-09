use axum::http::StatusCode;
use futures_util::future::join_all;
use proofplane::routes::authentication::AUTHORIZATION_HEADER;
use serde_json::Value;
use uuid::Uuid;

use super::support::TestApp;

#[tokio::test]
async fn me_returns_authenticated_user_and_provisions_once() {
    let app = TestApp::start_without_default_auth().await;
    let sub = "auth0|me-provisioning";

    let first = app
        .server()
        .get("/me")
        .add_header(AUTHORIZATION_HEADER, format!("Bearer {sub}"))
        .await;
    first.assert_status_ok();
    let first_body = first.json::<Value>();

    assert_eq!(first_body["auth0_sub"], sub);
    assert_eq!(first_body["email"], format!("{sub}@example.com"));
    assert_eq!(first_body["name"], "Integration Human");
    let user_id = first_body["id"]
        .as_str()
        .expect("id is a string")
        .to_owned();
    Uuid::parse_str(&user_id).expect("id is a UUID");

    let second = app
        .server()
        .get("/me")
        .add_header(AUTHORIZATION_HEADER, format!("Bearer {sub}"))
        .await;
    second.assert_status_ok();
    assert_eq!(second.json::<Value>()["id"], user_id);

    assert_eq!(count_users_with_sub(&app, sub).await, 1);
}

#[tokio::test]
async fn me_provisions_user_without_profile_claims() {
    let app = TestApp::start_without_default_auth().await;
    let sub = "auth0|me-no-profile";

    let response = app
        .server()
        .get("/me")
        .add_header(AUTHORIZATION_HEADER, format!("Bearer noprofile:{sub}"))
        .await;
    response.assert_status_ok();
    let body = response.json::<Value>();

    assert_eq!(body["auth0_sub"], sub);
    assert_eq!(body["email"], Value::Null);
    assert_eq!(body["name"], Value::Null);
    assert_eq!(count_users_with_sub(&app, sub).await, 1);
}

#[tokio::test]
async fn me_rejects_missing_or_invalid_bearer_tokens() {
    let app = TestApp::start_without_default_auth().await;

    let missing = app.server().get("/me").await;
    assert_unauthorized(&missing.json(), missing.status_code());

    let not_bearer = app
        .server()
        .get("/me")
        .add_header(AUTHORIZATION_HEADER, "Basic abc123")
        .await;
    assert_unauthorized(&not_bearer.json(), not_bearer.status_code());

    let empty_bearer = app
        .server()
        .get("/me")
        .add_header(AUTHORIZATION_HEADER, "Bearer ")
        .await;
    assert_unauthorized(&empty_bearer.json(), empty_bearer.status_code());

    let rejected = app
        .server()
        .get("/me")
        .add_header(AUTHORIZATION_HEADER, "Bearer invalid")
        .await;
    assert_unauthorized(&rejected.json(), rejected.status_code());
}

#[tokio::test]
async fn concurrent_first_requests_provision_exactly_one_user() {
    let app = TestApp::start_without_default_auth().await;
    let sub = "auth0|me-concurrent";

    let responses = join_all((0..16).map(|_| async {
        app.server()
            .get("/me")
            .add_header(AUTHORIZATION_HEADER, format!("Bearer {sub}"))
            .await
    }))
    .await;

    let ids = responses
        .iter()
        .map(|response| {
            response.assert_status_ok();
            response.json::<Value>()["id"]
                .as_str()
                .expect("id is a string")
                .to_owned()
        })
        .collect::<std::collections::HashSet<_>>();

    assert_eq!(ids.len(), 1, "all concurrent requests resolve to one user");
    assert_eq!(count_users_with_sub(&app, sub).await, 1);
}

async fn count_users_with_sub(app: &TestApp, sub: &str) -> i64 {
    let client = app.postgres().get().await.expect("pool client opens");
    let row = client
        .query_one("SELECT count(*) FROM users WHERE auth0_sub = $1", &[&sub])
        .await
        .expect("user count query runs");

    row.get(0)
}

fn assert_unauthorized(body: &Value, status: StatusCode) {
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["error"]["code"], "unauthorized");
    assert_eq!(body["error"]["details"], serde_json::json!([]));
}
