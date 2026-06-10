use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use axum::http::StatusCode;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use bytes::Bytes;
use chrono::{Duration, SecondsFormat, Utc};
use futures_core::Stream;
use proofplane::{
    handlers::{
        attachment_finalization::AttachmentFinalizationHandler,
        attachment_scan::AttachmentScanHandler,
    },
    object_storage::{
        ObjectKey, ObjectMetadata, ObjectStore, ObjectStream, PutObjectRequest, StorageError,
    },
    repository::OutboxMessage,
    scanner::{
        MalwareScanError, MalwareScanOutcome, MalwareScanResult, MalwareScanner, ScanObjectRequest,
    },
    worker::{WorkerMessage, ATTACHMENT_FINALIZATION_REQUESTED, ATTACHMENT_SCAN_REQUESTED},
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
    assert_eq!(
        final_key,
        format!(
            "workspaces/{workspace_id}/evidence-submissions/{submission_id}/attachments/{attachment_id}/artifact.txt"
        )
    );
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

#[tokio::test]
async fn attachment_scan_handler_coordinates_concrete_postgres_outcomes_and_retries() {
    let app = worker_test_app().await;
    let workspace_id = app.workspace_id("workspace");
    let submission_id = create_submission(&app, workspace_id).await;

    let clean = upload_attachment(&app, workspace_id, submission_id, "clean.txt").await;
    let request_id = Uuid::new_v4();
    let clean_handler = AttachmentScanHandler::new(
        app.postgres.clone(),
        Arc::new(FakeScanner::outcome(MalwareScanOutcome::Clean)),
        5,
    );
    let clean_message = scan_worker_message(&clean, submission_id, Some(request_id), Some(1));
    clean_handler
        .handle_scan_requested(clean_message.clone())
        .await
        .expect("clean scan succeeds");
    clean_handler
        .handle_scan_requested(clean_message)
        .await
        .expect("duplicate clean scan is acknowledged");

    assert_eq!(attachment_status(&app, clean.id).await, "finalizing");
    let finalization_messages = outbox_messages(
        &app,
        ATTACHMENT_FINALIZATION_REQUESTED,
        &clean.id.to_string(),
    )
    .await;
    assert_eq!(finalization_messages.len(), 1);
    assert_eq!(finalization_messages[0].request_id, Some(request_id));

    let malicious = upload_attachment(&app, workspace_id, submission_id, "malicious.txt").await;
    AttachmentScanHandler::new(
        app.postgres.clone(),
        Arc::new(FakeScanner::outcome(MalwareScanOutcome::Malicious {
            reason: "EICAR".to_owned(),
        })),
        5,
    )
    .handle_scan_requested(scan_worker_message(
        &malicious,
        submission_id,
        None,
        Some(1),
    ))
    .await
    .expect("malicious scan succeeds");
    assert_eq!(
        attachment_status(&app, malicious.id).await,
        "contains_virus"
    );

    let failed = upload_attachment(&app, workspace_id, submission_id, "failed.txt").await;
    AttachmentScanHandler::new(
        app.postgres.clone(),
        Arc::new(FakeScanner::outcome(MalwareScanOutcome::Failed {
            reason: "scanner refused".to_owned(),
        })),
        5,
    )
    .handle_scan_requested(scan_worker_message(&failed, submission_id, None, Some(1)))
    .await
    .expect("failed scan outcome is persisted");
    assert_eq!(attachment_status(&app, failed.id).await, "failed");

    let retry = upload_attachment(&app, workspace_id, submission_id, "retry.txt").await;
    let retry_handler =
        AttachmentScanHandler::new(app.postgres.clone(), Arc::new(FakeScanner::error()), 5);
    assert!(retry_handler
        .handle_scan_requested(scan_worker_message(&retry, submission_id, None, Some(4)))
        .await
        .is_err());
    assert_eq!(attachment_status(&app, retry.id).await, "pending");
    retry_handler
        .handle_scan_requested(scan_worker_message(&retry, submission_id, None, Some(5)))
        .await
        .expect("final delivery persists failure");
    assert_eq!(attachment_status(&app, retry.id).await, "failed");
}

#[tokio::test]
async fn attachment_scan_handler_rolls_back_update_and_outbox_failures() {
    let app = worker_test_app().await;
    let workspace_id = app.workspace_id("workspace");
    let submission_id = create_submission(&app, workspace_id).await;
    let handler = AttachmentScanHandler::new(
        app.postgres.clone(),
        Arc::new(FakeScanner::outcome(MalwareScanOutcome::Clean)),
        5,
    );

    let update_failure =
        upload_attachment(&app, workspace_id, submission_id, "update-failure.txt").await;
    install_failure_trigger(
        &app,
        "evidence_attachments",
        "attachment_update_failure",
        "BEFORE UPDATE",
        "NEW.upload_status = 'finalizing'",
    )
    .await;
    assert!(handler
        .handle_scan_requested(scan_worker_message(
            &update_failure,
            submission_id,
            None,
            Some(1),
        ))
        .await
        .is_err());
    assert_eq!(attachment_status(&app, update_failure.id).await, "pending");
    remove_failure_trigger(&app, "evidence_attachments", "attachment_update_failure").await;

    let outbox_failure =
        upload_attachment(&app, workspace_id, submission_id, "outbox-failure.txt").await;
    install_failure_trigger(
        &app,
        "outbox_messages",
        "attachment_outbox_failure",
        "BEFORE INSERT",
        "NEW.event_type = 'attachment.finalization_requested'",
    )
    .await;
    assert!(handler
        .handle_scan_requested(scan_worker_message(
            &outbox_failure,
            submission_id,
            None,
            Some(1),
        ))
        .await
        .is_err());
    assert_eq!(attachment_status(&app, outbox_failure.id).await, "pending");
    assert!(outbox_messages(
        &app,
        ATTACHMENT_FINALIZATION_REQUESTED,
        &outbox_failure.id.to_string()
    )
    .await
    .is_empty());
    remove_failure_trigger(&app, "outbox_messages", "attachment_outbox_failure").await;
}

#[tokio::test]
async fn attachment_finalization_handler_uses_concrete_postgres_and_external_store_failures() {
    let app = worker_test_app().await;
    let workspace_id = app.workspace_id("workspace");
    let submission_id = create_submission(&app, workspace_id).await;
    let scanner = AttachmentScanHandler::new(
        app.postgres.clone(),
        Arc::new(FakeScanner::outcome(MalwareScanOutcome::Clean)),
        5,
    );

    let successful = upload_attachment(&app, workspace_id, submission_id, "successful.txt").await;
    scanner
        .handle_scan_requested(scan_worker_message(
            &successful,
            submission_id,
            None,
            Some(1),
        ))
        .await
        .expect("scan prepares finalization");
    let store = Arc::new(FakeObjectStore::default());
    AttachmentFinalizationHandler::new(app.postgres.clone(), store.clone())
        .handle_finalization_requested(finalization_worker_message(&successful, submission_id))
        .await
        .expect("finalization succeeds");
    AttachmentFinalizationHandler::new(app.postgres.clone(), store.clone())
        .handle_finalization_requested(finalization_worker_message(&successful, submission_id))
        .await
        .expect("duplicate finalization is acknowledged");
    assert_eq!(attachment_status(&app, successful.id).await, "uploaded");
    {
        let state = store.state.lock().unwrap();
        assert_eq!(state.copied.len(), 1);
        assert_eq!(state.deleted, vec![successful.object_key.clone()]);
    }

    let delete_failure =
        upload_attachment(&app, workspace_id, submission_id, "delete-failure.txt").await;
    scanner
        .handle_scan_requested(scan_worker_message(
            &delete_failure,
            submission_id,
            None,
            Some(1),
        ))
        .await
        .expect("scan prepares delete failure");
    AttachmentFinalizationHandler::new(
        app.postgres.clone(),
        Arc::new(FakeObjectStore::delete_failure()),
    )
    .handle_finalization_requested(finalization_worker_message(&delete_failure, submission_id))
    .await
    .expect("delete failure is best effort");
    assert_eq!(attachment_status(&app, delete_failure.id).await, "uploaded");

    let copy_failure =
        upload_attachment(&app, workspace_id, submission_id, "copy-failure.txt").await;
    scanner
        .handle_scan_requested(scan_worker_message(
            &copy_failure,
            submission_id,
            None,
            Some(1),
        ))
        .await
        .expect("scan prepares copy failure");
    let failing_store = Arc::new(FakeObjectStore::copy_failure());
    assert!(
        AttachmentFinalizationHandler::new(app.postgres.clone(), failing_store)
            .handle_finalization_requested(finalization_worker_message(
                &copy_failure,
                submission_id,
            ))
            .await
            .is_err()
    );
    assert_eq!(attachment_status(&app, copy_failure.id).await, "finalizing");

    let database_failure =
        upload_attachment(&app, workspace_id, submission_id, "database-failure.txt").await;
    scanner
        .handle_scan_requested(scan_worker_message(
            &database_failure,
            submission_id,
            None,
            Some(1),
        ))
        .await
        .expect("scan prepares database failure");
    install_failure_trigger(
        &app,
        "evidence_attachments",
        "attachment_uploaded_failure",
        "BEFORE UPDATE",
        "NEW.upload_status = 'uploaded'",
    )
    .await;
    let database_store = Arc::new(FakeObjectStore::default());
    assert!(
        AttachmentFinalizationHandler::new(app.postgres.clone(), database_store.clone())
            .handle_finalization_requested(finalization_worker_message(
                &database_failure,
                submission_id,
            ))
            .await
            .is_err()
    );
    assert_eq!(
        attachment_status(&app, database_failure.id).await,
        "finalizing"
    );
    assert!(database_store.state.lock().unwrap().deleted.is_empty());
    remove_failure_trigger(&app, "evidence_attachments", "attachment_uploaded_failure").await;
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

async fn worker_test_app() -> TestApp {
    TestApp::builder()
        .workspace("workspace", "Worker handler workspace")
        .with_default_membership()
        .build()
        .await
}

struct UploadedAttachment {
    id: Uuid,
    object_key: String,
}

async fn upload_attachment(
    app: &TestApp,
    workspace_id: Uuid,
    submission_id: Uuid,
    filename: &str,
) -> UploadedAttachment {
    let response = app
        .post(&format!(
            "/workspaces/{workspace_id}/evidence-submissions/{submission_id}/attachments"
        ))
        .multipart(attachment_form(
            b"handler integration attachment",
            filename,
            "text/plain",
            None,
        ))
        .await;
    response.assert_status(StatusCode::ACCEPTED);
    let attachment = response.json::<Value>()["attachment"].clone();

    UploadedAttachment {
        id: Uuid::parse_str(
            attachment["id"]
                .as_str()
                .expect("attachment ID is a string"),
        )
        .expect("attachment ID is a UUID"),
        object_key: attachment["object_key"]
            .as_str()
            .expect("object key is a string")
            .to_owned(),
    }
}

fn scan_worker_message(
    attachment: &UploadedAttachment,
    submission_id: Uuid,
    request_id: Option<Uuid>,
    delivery_attempt: Option<u32>,
) -> WorkerMessage {
    WorkerMessage {
        message_id: Uuid::new_v4().to_string(),
        event_type: ATTACHMENT_SCAN_REQUESTED.to_owned(),
        aggregate_type: "evidence_attachment".to_owned(),
        aggregate_id: attachment.id.to_string(),
        request_id,
        payload: json!({
            "evidence_submission_id": submission_id.to_string(),
            "object_key": attachment.object_key,
        }),
        delivery_attempt,
    }
}

fn finalization_worker_message(
    attachment: &UploadedAttachment,
    submission_id: Uuid,
) -> WorkerMessage {
    WorkerMessage {
        message_id: Uuid::new_v4().to_string(),
        event_type: ATTACHMENT_FINALIZATION_REQUESTED.to_owned(),
        aggregate_type: "evidence_attachment".to_owned(),
        aggregate_id: attachment.id.to_string(),
        request_id: None,
        payload: json!({
            "evidence_submission_id": submission_id.to_string(),
            "object_key": attachment.object_key,
        }),
        delivery_attempt: Some(1),
    }
}

async fn attachment_status(app: &TestApp, attachment_id: Uuid) -> String {
    let client = app.postgres().get().await.expect("connection opens");
    client
        .query_one(
            "SELECT upload_status FROM evidence_attachments WHERE id = $1",
            &[&attachment_id],
        )
        .await
        .expect("attachment status loads")
        .get("upload_status")
}

// Each TestApp owns a dedicated Postgres container, so these triggers are test-local.
async fn install_failure_trigger(
    app: &TestApp,
    table: &str,
    trigger: &str,
    timing: &str,
    condition: &str,
) {
    let client = app.postgres().get().await.expect("connection opens");
    client
        .batch_execute(&format!(
            r#"
CREATE OR REPLACE FUNCTION {trigger}_fn() RETURNS trigger AS $$
BEGIN
    IF {condition} THEN
        RAISE EXCEPTION 'injected {trigger}';
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;
CREATE TRIGGER {trigger}
{timing} ON {table}
FOR EACH ROW EXECUTE FUNCTION {trigger}_fn();
"#
        ))
        .await
        .expect("failure trigger installs");
}

async fn remove_failure_trigger(app: &TestApp, table: &str, trigger: &str) {
    let client = app.postgres().get().await.expect("connection opens");
    client
        .batch_execute(&format!(
            "DROP TRIGGER {trigger} ON {table}; DROP FUNCTION {trigger}_fn();"
        ))
        .await
        .expect("failure trigger is removed");
}

struct FakeScanner {
    result: Result<MalwareScanResult, MalwareScanError>,
}

impl FakeScanner {
    fn outcome(outcome: MalwareScanOutcome) -> Self {
        Self {
            result: Ok(MalwareScanResult {
                scanner_name: "fake".to_owned(),
                scanner_version: Some("1".to_owned()),
                outcome,
            }),
        }
    }

    fn error() -> Self {
        Self {
            result: Err(MalwareScanError::Unavailable {
                reason: "offline".to_owned(),
            }),
        }
    }
}

#[async_trait]
impl MalwareScanner for FakeScanner {
    async fn scan_object(
        &self,
        _request: ScanObjectRequest,
    ) -> Result<MalwareScanResult, MalwareScanError> {
        self.result.clone()
    }
}

#[derive(Default)]
struct FakeObjectStore {
    state: Mutex<FakeObjectStoreState>,
}

#[derive(Default)]
struct FakeObjectStoreState {
    copied: Vec<(String, String)>,
    deleted: Vec<String>,
    copy_fails: bool,
    delete_fails: bool,
}

impl FakeObjectStore {
    fn copy_failure() -> Self {
        Self {
            state: Mutex::new(FakeObjectStoreState {
                copy_fails: true,
                ..Default::default()
            }),
        }
    }

    fn delete_failure() -> Self {
        Self {
            state: Mutex::new(FakeObjectStoreState {
                delete_fails: true,
                ..Default::default()
            }),
        }
    }
}

#[async_trait]
impl ObjectStore for FakeObjectStore {
    async fn put_object<S>(
        &self,
        _request: PutObjectRequest<S>,
    ) -> Result<ObjectMetadata, StorageError>
    where
        S: Stream<Item = Result<Bytes, StorageError>> + Send,
    {
        unreachable!()
    }

    async fn get_object(&self, _key: ObjectKey) -> Result<ObjectStream, StorageError> {
        unreachable!()
    }

    async fn head_object(&self, _key: ObjectKey) -> Result<ObjectMetadata, StorageError> {
        unreachable!()
    }

    async fn copy_object(
        &self,
        source: ObjectKey,
        destination: ObjectKey,
    ) -> Result<ObjectMetadata, StorageError> {
        let mut state = self.state.lock().unwrap();
        state
            .copied
            .push((source.to_string(), destination.to_string()));
        if state.copy_fails {
            return Err(StorageError::UnsupportedBackend);
        }
        Ok(ObjectMetadata {
            key: destination,
            content_type: "text/plain".to_owned(),
            content_length: 30,
            sha256: "checksum".to_owned(),
        })
    }

    async fn delete_object(&self, key: ObjectKey) -> Result<(), StorageError> {
        let mut state = self.state.lock().unwrap();
        state.deleted.push(key.to_string());
        if state.delete_fails {
            return Err(StorageError::UnsupportedBackend);
        }
        Ok(())
    }
}
