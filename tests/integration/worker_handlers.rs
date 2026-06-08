use axum::http::StatusCode;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use chrono::{Duration, SecondsFormat, Utc};
use proofplane::{
    repository::OutboxMessage,
    worker::{ATTACHMENT_FINALIZATION_REQUESTED, ATTACHMENT_SCAN_REQUESTED},
};
use serde_json::{json, Value};
use uuid::Uuid;

use super::support::{attachment_form, TestApp};

#[tokio::test]
async fn attachment_worker_handlers_are_idempotent_for_duplicate_deliveries() {
    let app = TestApp::builder()
        .workspace("workspace", "Worker idempotency workspace")
        .with_default_membership()
        .build()
        .await;
    let workspace_id = app.workspace_id("workspace");
    let submission_id = create_submission(&app, workspace_id).await;
    let upload = app
        .post(&format!(
            "/workspaces/{workspace_id}/evidence-submissions/{submission_id}/attachments"
        ))
        .multipart(attachment_form(
            b"clean attachment",
            "artifact.txt",
            "text/plain",
            None,
        ))
        .await;
    upload.assert_status(StatusCode::ACCEPTED);
    let attachment = upload.json::<Value>()["attachment"].clone();
    let attachment_id = attachment["id"]
        .as_str()
        .expect("attachment ID is a string");
    let quarantine_key = attachment["object_key"]
        .as_str()
        .expect("object key is a string");

    let worker = app.worker_server().await;
    let scan_message = outbox_message(&app, ATTACHMENT_SCAN_REQUESTED, attachment_id).await;

    deliver_twice(&worker, &scan_message).await;

    let finalization_messages =
        outbox_messages(&app, ATTACHMENT_FINALIZATION_REQUESTED, attachment_id).await;
    assert_eq!(
        finalization_messages.len(),
        1,
        "duplicate scan delivery must enqueue finalization once"
    );

    deliver_twice(&worker, &finalization_messages[0]).await;

    let detail = app
        .get(&format!(
            "/workspaces/{workspace_id}/evidence-submissions/{submission_id}"
        ))
        .await
        .json::<Value>();
    let finalized = &detail["attachments"][0];
    assert_eq!(finalized["upload_status"], "uploaded");

    let final_key = finalized["object_key"]
        .as_str()
        .expect("final object key is a string");
    assert_ne!(final_key, quarantine_key);
    assert!(app
        .object_storage_root()
        .join("objects")
        .join(final_key)
        .exists());
    assert!(!app
        .object_storage_root()
        .join("objects")
        .join(quarantine_key)
        .exists());

    assert_eq!(
        outbox_messages(&app, ATTACHMENT_FINALIZATION_REQUESTED, attachment_id)
            .await
            .len(),
        1,
        "duplicate finalization delivery must not create additional work"
    );
}

async fn deliver_twice(worker: &axum_test::TestServer, message: &OutboxMessage) {
    let envelope = pubsub_envelope(message);

    for _ in 0..2 {
        worker
            .post("/pubsub/messages")
            .json(&envelope)
            .await
            .assert_status(StatusCode::NO_CONTENT);
    }
}

async fn outbox_message(app: &TestApp, event_type: &str, aggregate_id: &str) -> OutboxMessage {
    let mut messages = outbox_messages(app, event_type, aggregate_id).await;
    assert_eq!(messages.len(), 1, "expected one matching outbox message");
    messages.remove(0)
}

async fn outbox_messages(
    app: &TestApp,
    event_type: &str,
    aggregate_id: &str,
) -> Vec<OutboxMessage> {
    app.postgres()
        .list_due_outbox_messages(Utc::now() + Duration::seconds(1), 20)
        .await
        .expect("outbox messages list")
        .into_iter()
        .filter(|message| message.event_type == event_type && message.aggregate_id == aggregate_id)
        .collect()
}

fn pubsub_envelope(message: &OutboxMessage) -> Value {
    let data = json!({
        "event_type": message.event_type,
        "aggregate_type": message.aggregate_type,
        "aggregate_id": message.aggregate_id,
        "request_id": message.request_id,
        "payload": message.payload,
    });

    json!({
        "message": {
            "messageId": format!("outbox-{}", message.id),
            "data": STANDARD.encode(data.to_string()),
        },
        "deliveryAttempt": 1,
    })
}

async fn create_submission(app: &TestApp, workspace_id: Uuid) -> Uuid {
    let request = app
        .create_evidence_request(
            workspace_id,
            &json!({
                "title": "Worker handler target",
                "description": "Integration fixture",
                "collection_instructions": "Upload the worker integration fixture.",
                "cadence": "quarterly",
                "due_at": (Utc::now() + Duration::days(7))
                    .to_rfc3339_opts(SecondsFormat::Secs, true),
                "schedule_anchor_at": "2026-01-01T00:00:00Z",
                "freshness_window_days": 30,
                "status": "active",
            }),
        )
        .await;
    let evidence_request_id = uuid_field(&request, "id");
    let submission = app
        .post(&format!(
            "/workspaces/{workspace_id}/evidence-requests/{evidence_request_id}/submissions"
        ))
        .json(&json!({
            "coverage_start_at": "2026-01-01T00:00:00Z",
            "coverage_end_at": "2026-01-31T23:59:59Z",
            "source_system": "integration",
            "collection_method": "worker test",
        }))
        .await;
    submission.assert_status_ok();
    uuid_field(&submission.json(), "id")
}

fn uuid_field(value: &Value, field: &str) -> Uuid {
    Uuid::parse_str(
        value[field]
            .as_str()
            .unwrap_or_else(|| panic!("{field} is a string")),
    )
    .unwrap_or_else(|_| panic!("{field} is a UUID"))
}
