use axum::http::StatusCode;
use futures_util::future::join_all;
use proofplane::routes::authentication::AUTHORIZATION_HEADER;
use proofplane::routes::request_context::REQUEST_ID_HEADER;
use serde_json::Value;
use uuid::Uuid;

use super::support::{capture_audit_logs, TestApp};

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
    assert_eq!(first_body["last_login_at"], Value::Null);
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
async fn login_updates_last_login_and_emits_audit_event_every_time() {
    let app = TestApp::start_without_default_auth().await;
    let sub = "auth0|login-audit";

    let (first, first_logs) = capture_audit_logs(|request_id| {
        let app = &app;
        async move {
            app.server()
                .post("/login")
                .add_header(AUTHORIZATION_HEADER, format!("Bearer {sub}"))
                .add_header(REQUEST_ID_HEADER, request_id.to_string())
                .await
        }
    })
    .await;
    first.assert_status_ok();
    let first_body = first.json::<Value>();
    let user_id = first_body["id"].as_str().expect("id is a string");
    let first_login_at = first_body["last_login_at"]
        .as_str()
        .expect("last_login_at is set");

    assert_eq!(first_logs.len(), 1);
    assert_audit_event(&first_logs[0], "user.logged_in", user_id, "login");
    assert_eq!(first_logs[0]["fields"]["object_type"], "user");
    assert_eq!(first_logs[0]["fields"]["object_id"], user_id);

    let (second, second_logs) = capture_audit_logs(|request_id| {
        let app = &app;
        async move {
            app.server()
                .post("/login")
                .add_header(AUTHORIZATION_HEADER, format!("Bearer {sub}"))
                .add_header(REQUEST_ID_HEADER, request_id.to_string())
                .await
        }
    })
    .await;
    second.assert_status_ok();
    let second_body = second.json::<Value>();
    let second_login_at = second_body["last_login_at"]
        .as_str()
        .expect("last_login_at is set");

    assert_eq!(second_body["id"], user_id);
    assert!(second_login_at >= first_login_at);
    assert_eq!(second_logs.len(), 1);
    assert_audit_event(&second_logs[0], "user.logged_in", user_id, "login");
}

#[tokio::test]
async fn me_does_not_update_last_login_or_emit_login_audit_event() {
    let app = TestApp::start_without_default_auth().await;
    let sub = "auth0|me-not-login";

    app.server()
        .post("/login")
        .add_header(AUTHORIZATION_HEADER, format!("Bearer {sub}"))
        .await
        .assert_status_ok();
    let before = last_login_at(&app, sub)
        .await
        .expect("login timestamp exists");

    let (me, logs) = capture_audit_logs(|request_id| {
        let app = &app;
        async move {
            app.server()
                .get("/me")
                .add_header(AUTHORIZATION_HEADER, format!("Bearer {sub}"))
                .add_header(REQUEST_ID_HEADER, request_id.to_string())
                .await
        }
    })
    .await;
    me.assert_status_ok();

    assert_eq!(last_login_at(&app, sub).await, Some(before));
    assert!(logs.is_empty());
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
async fn me_preserves_existing_profile_when_later_claims_are_absent() {
    let app = TestApp::start_without_default_auth().await;
    let sub = "auth0|me-profile-preserved";

    let first = app
        .server()
        .get("/me")
        .add_header(AUTHORIZATION_HEADER, format!("Bearer {sub}"))
        .await;
    first.assert_status_ok();
    let first_body = first.json::<Value>();
    let user_id = first_body["id"].clone();

    assert_eq!(first_body["email"], format!("{sub}@example.com"));
    assert_eq!(first_body["name"], "Integration Human");

    let second = app
        .server()
        .get("/me")
        .add_header(AUTHORIZATION_HEADER, format!("Bearer noprofile:{sub}"))
        .await;
    second.assert_status_ok();
    let second_body = second.json::<Value>();

    assert_eq!(second_body["id"], user_id);
    assert_eq!(second_body["email"], format!("{sub}@example.com"));
    assert_eq!(second_body["name"], "Integration Human");
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

async fn last_login_at(app: &TestApp, sub: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    let client = app.postgres().get().await.expect("pool client opens");
    let row = client
        .query_one(
            "SELECT last_login_at FROM users WHERE auth0_sub = $1",
            &[&sub],
        )
        .await
        .expect("user query runs");

    row.get("last_login_at")
}

fn assert_audit_event(record: &Value, event_name: &str, user_id: &str, operation: &str) {
    let fields = &record["fields"];
    assert_eq!(fields["type"], "audit_log");
    assert_eq!(fields["event_name"], event_name);
    assert_eq!(fields["outcome"], "success");
    assert_eq!(fields["actor_type"], "user");
    assert_eq!(fields["user_id"], user_id);
    assert_eq!(fields["client_type"], "rest");
    assert_eq!(fields["operation"], operation);
    assert!(Uuid::parse_str(fields["request_id"].as_str().expect("request id is set")).is_ok());
}

fn assert_unauthorized(body: &Value, status: StatusCode) {
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["error"]["code"], "unauthorized");
    assert_eq!(body["error"]["details"], serde_json::json!([]));
}
