use std::{
    io,
    sync::{Arc, Mutex},
};

use axum::http::StatusCode;
use proofplane::routes::authentication::ACTOR_ID_HEADER;
use serde_json::Value;
use uuid::Uuid;

use chrono::{Duration, Utc};
use proofplane::{domain::UpdateApiCredentialPayload, routes::authentication::API_KEY_HEADER};

use super::support::{TestApp, INTEGRATION_ACTOR_ID};

#[tokio::test]
async fn evidence_request_routes_require_valid_api_keys() {
    let app = TestApp::start_without_default_auth().await;
    let workspace_id = app.insert_workspace("Protected workspace").await;
    let path = format!("/workspaces/{workspace_id}/evidence-requests");

    let missing = app.server().get(&path).await;
    assert_unauthorized(&missing.json(), missing.status_code());

    let missing_actor_id = app
        .server()
        .get(&path)
        .add_header(API_KEY_HEADER, app.api_key())
        .await;
    assert_unauthorized(&missing_actor_id.json(), missing_actor_id.status_code());

    let missing_api_key = app
        .server()
        .get(&path)
        .add_header(ACTOR_ID_HEADER, INTEGRATION_ACTOR_ID)
        .await;
    assert_unauthorized(&missing_api_key.json(), missing_api_key.status_code());

    let invalid = app
        .server()
        .get(&path)
        .add_header(ACTOR_ID_HEADER, INTEGRATION_ACTOR_ID)
        .add_header(API_KEY_HEADER, "not-a-known-key")
        .await;
    assert_unauthorized(&invalid.json(), invalid.status_code());

    let wrong_actor = app
        .server()
        .get(&path)
        .add_header(ACTOR_ID_HEADER, "wrong-actor")
        .add_header(API_KEY_HEADER, app.api_key())
        .await;
    assert_unauthorized(&wrong_actor.json(), wrong_actor.status_code());

    let valid = app
        .server()
        .get(&path)
        .add_header(ACTOR_ID_HEADER, INTEGRATION_ACTOR_ID)
        .add_header(API_KEY_HEADER, app.api_key())
        .await;
    valid.assert_status_ok();
}

#[tokio::test]
async fn evidence_request_routes_reject_revoked_and_expired_credentials() {
    let app = TestApp::start_without_default_auth().await;
    let workspace_id = app.insert_workspace("Lifecycle workspace").await;
    let path = format!("/workspaces/{workspace_id}/evidence-requests");
    let credential = app
        .postgres()
        .get_api_credential("integration-api-key")
        .await
        .expect("credential reads")
        .expect("credential exists");

    app.postgres()
        .update_api_credential(
            &credential.id,
            &UpdateApiCredentialPayload {
                actor_id: credential.actor_id.clone(),
                name: credential.name.clone(),
                key_id: credential.key_id.clone(),
                credential_hash: credential.credential_hash.clone(),
                expires_at: credential.expires_at,
                revoked_at: Some(Utc::now()),
            },
        )
        .await
        .expect("credential revokes");
    let revoked = app
        .server()
        .get(&path)
        .add_header(ACTOR_ID_HEADER, INTEGRATION_ACTOR_ID)
        .add_header(API_KEY_HEADER, app.api_key())
        .await;
    assert_unauthorized(&revoked.json(), revoked.status_code());

    app.postgres()
        .update_api_credential(
            &credential.id,
            &UpdateApiCredentialPayload {
                actor_id: credential.actor_id,
                name: credential.name,
                key_id: credential.key_id,
                credential_hash: credential.credential_hash,
                expires_at: Some(Utc::now() - Duration::seconds(1)),
                revoked_at: None,
            },
        )
        .await
        .expect("credential expires");
    let expired = app
        .server()
        .get(&path)
        .add_header(ACTOR_ID_HEADER, INTEGRATION_ACTOR_ID)
        .add_header(API_KEY_HEADER, app.api_key())
        .await;
    assert_unauthorized(&expired.json(), expired.status_code());
}

#[tokio::test]
async fn public_routes_do_not_require_api_keys() {
    let app = TestApp::start_without_default_auth().await;

    app.server().get("/livez").await.assert_status_ok();
    app.server().get("/version").await.assert_status_ok();
}

#[tokio::test]
async fn request_ids_are_generated_propagated_and_validated() {
    let app = TestApp::start_without_default_auth().await;

    let generated = app.server().get("/version").await;
    generated.assert_status_ok();
    let generated_id = generated.header("x-request-id");
    Uuid::parse_str(generated_id.to_str().expect("generated request ID is text"))
        .expect("generated request ID is a UUID");

    let inbound_id = Uuid::new_v4().to_string();
    let propagated = app
        .server()
        .get("/version")
        .add_header("x-request-id", &inbound_id)
        .await;
    propagated.assert_status_ok();
    assert_eq!(
        propagated
            .header("x-request-id")
            .to_str()
            .expect("response request ID is text"),
        inbound_id
    );

    let invalid = app
        .server()
        .get("/version")
        .add_header("x-request-id", "not-a-uuid")
        .await;
    assert_eq!(invalid.status_code(), StatusCode::BAD_REQUEST);
    assert_eq!(invalid.json::<Value>()["error"]["code"], "bad_request");
}

#[tokio::test(flavor = "current_thread")]
async fn authenticated_request_logs_context_without_api_key() {
    let log_bytes = Arc::new(Mutex::new(Vec::new()));
    let writer_bytes = log_bytes.clone();
    let subscriber = tracing_subscriber::fmt()
        .json()
        .with_writer(move || LogWriter(writer_bytes.clone()))
        .finish();
    tracing::subscriber::set_global_default(subscriber)
        .expect("integration process has no tracing subscriber yet");
    let app = TestApp::start_without_default_auth().await;
    let workspace_id = app.insert_workspace("Logged workspace").await;
    let request_id = Uuid::new_v4().to_string();

    app.server()
        .get(&format!("/workspaces/{workspace_id}/evidence-requests"))
        .add_header(ACTOR_ID_HEADER, INTEGRATION_ACTOR_ID)
        .add_header(API_KEY_HEADER, app.api_key())
        .add_header("x-request-id", &request_id)
        .await
        .assert_status_ok();
    let invalid_request_path = "/invalid-request-id-log-coverage";
    app.server()
        .get(invalid_request_path)
        .add_header("x-request-id", "not-a-uuid")
        .await
        .assert_status_bad_request();

    let logs = String::from_utf8(log_bytes.lock().expect("log buffer locks").clone())
        .expect("logs are UTF-8");
    assert!(logs.contains(&request_id), "captured logs: {logs}");
    assert!(logs.contains("integration-system"), "captured logs: {logs}");
    assert!(logs.contains(invalid_request_path), "captured logs: {logs}");
    assert!(!logs.contains(app.api_key()), "captured logs: {logs}");
}

fn assert_unauthorized(body: &Value, status: StatusCode) {
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["error"]["code"], "unauthorized");
    assert_eq!(body["error"]["details"], serde_json::json!([]));
}

#[derive(Clone)]
struct LogWriter(Arc<Mutex<Vec<u8>>>);

impl io::Write for LogWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0
            .lock()
            .expect("log buffer locks")
            .extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
