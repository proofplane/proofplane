use std::{sync::Arc, time::Duration as StdDuration};

use axum::http::StatusCode;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use chrono::{Duration, SecondsFormat, Utc};
use proofplane::{
    handlers::{
        attachment_finalization::AttachmentFinalizationHandler,
        attachment_scan::AttachmentScanHandler,
    },
    object_storage::{FilesystemObjectStore, ObjectKey, ObjectStore, ObjectStream, StorageError},
    repository::OutboxMessage,
    scanner::ClamAvMalwareScanner,
    worker::{WorkerMessage, ATTACHMENT_FINALIZATION_REQUESTED, ATTACHMENT_SCAN_REQUESTED},
};
use serde_json::{json, Value};
use uuid::Uuid;

use super::support::{attachment_form, TestApp};

const EICAR: &[u8] = b"X5O!P%@AP[4\\PZX54(P^)7CC)7}$EICAR-STANDARD-ANTIVIRUS-TEST-FILE!$H+H*";

#[tokio::test]
async fn attachment_worker_handlers_are_idempotent_for_duplicate_deliveries() {
    let app = TestApp::builder()
        .with_clamav()
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
    let clean_handler =
        AttachmentScanHandler::new(app.postgres.clone(), scanner_for(&app).await, 5);
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

    let malicious =
        upload_attachment_with_content(&app, workspace_id, submission_id, "malicious.txt", EICAR)
            .await;
    AttachmentScanHandler::new(app.postgres.clone(), scanner_for(&app).await, 5)
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
        scanner_with_address(&app, clamd_error_address().await, StdDuration::from_secs(1)).await,
        5,
    )
    .handle_scan_requested(scan_worker_message(&failed, submission_id, None, Some(1)))
    .await
    .expect("failed scan outcome is persisted");
    assert_eq!(attachment_status(&app, failed.id).await, "failed");

    let retry = upload_attachment(&app, workspace_id, submission_id, "retry.txt").await;
    let retry_handler = AttachmentScanHandler::new(
        app.postgres.clone(),
        scanner_with_address(
            &app,
            unavailable_address().await,
            StdDuration::from_millis(100),
        )
        .await,
        5,
    );
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

    let timed_out = upload_attachment(&app, workspace_id, submission_id, "timeout.txt").await;
    let timeout_handler = AttachmentScanHandler::new(
        app.postgres.clone(),
        scanner_with_address(
            &app,
            hanging_clamd_address().await,
            StdDuration::from_millis(50),
        )
        .await,
        5,
    );
    assert!(timeout_handler
        .handle_scan_requested(scan_worker_message(
            &timed_out,
            submission_id,
            None,
            Some(1),
        ))
        .await
        .is_err());
    assert_eq!(attachment_status(&app, timed_out.id).await, "pending");
}

#[tokio::test]
async fn attachment_scan_handler_rolls_back_update_and_outbox_failures() {
    let app = worker_test_app().await;
    let workspace_id = app.workspace_id("workspace");
    let submission_id = create_submission(&app, workspace_id).await;
    let handler = AttachmentScanHandler::new(app.postgres.clone(), scanner_for(&app).await, 5);

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
    let scanner = AttachmentScanHandler::new(app.postgres.clone(), scanner_for(&app).await, 5);

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
    let store = filesystem_object_store(&app).await;
    let successful_quarantine = stored_object(&store, &successful.object_key).await;
    let successful_final_key = final_object_key(workspace_id, submission_id, &successful);
    AttachmentFinalizationHandler::new(app.postgres.clone(), store.clone())
        .handle_finalization_requested(finalization_worker_message(&successful, submission_id))
        .await
        .expect("finalization succeeds");
    assert_stored_object_matches(
        stored_object(&store, successful_final_key.as_str()).await,
        &successful_quarantine,
        &successful_final_key,
    );
    assert_object_missing(&store, &successful.object_key).await;

    AttachmentFinalizationHandler::new(app.postgres.clone(), store.clone())
        .handle_finalization_requested(finalization_worker_message(&successful, submission_id))
        .await
        .expect("duplicate finalization is acknowledged");
    assert_eq!(attachment_status(&app, successful.id).await, "uploaded");
    assert_stored_object_matches(
        stored_object(&store, successful_final_key.as_str()).await,
        &successful_quarantine,
        &successful_final_key,
    );
    assert_object_missing(&store, &successful.object_key).await;

    #[cfg(unix)]
    {
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
        let delete_failure_quarantine = stored_object(&store, &delete_failure.object_key).await;
        let delete_failure_final_key =
            final_object_key(workspace_id, submission_id, &delete_failure);
        let quarantine_parent = object_path(app.object_storage_root(), &delete_failure.object_key)
            .parent()
            .expect("quarantine object has a parent")
            .to_path_buf();
        let permission_guard = ReadOnlyDirectoryGuard::new(quarantine_parent);

        AttachmentFinalizationHandler::new(app.postgres.clone(), store.clone())
            .handle_finalization_requested(finalization_worker_message(
                &delete_failure,
                submission_id,
            ))
            .await
            .expect("delete failure is best effort");
        assert_stored_object_matches(
            stored_object(&store, delete_failure_final_key.as_str()).await,
            &delete_failure_quarantine,
            &delete_failure_final_key,
        );
        assert!(object_path(app.object_storage_root(), &delete_failure.object_key).exists());
        assert_eq!(attachment_status(&app, delete_failure.id).await, "uploaded");
        permission_guard.restore();
    }

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
    store
        .delete_object(&object_key(&copy_failure.object_key))
        .await
        .expect("quarantined object is removed to inject copy failure");
    assert!(
        AttachmentFinalizationHandler::new(app.postgres.clone(), store.clone())
            .handle_finalization_requested(finalization_worker_message(
                &copy_failure,
                submission_id,
            ))
            .await
            .is_err()
    );
    assert_eq!(attachment_status(&app, copy_failure.id).await, "finalizing");
    assert_object_missing(
        &store,
        final_object_key(workspace_id, submission_id, &copy_failure).as_str(),
    )
    .await;

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
    let database_failure_quarantine = stored_object(&store, &database_failure.object_key).await;
    let database_failure_final_key =
        final_object_key(workspace_id, submission_id, &database_failure);
    assert!(
        AttachmentFinalizationHandler::new(app.postgres.clone(), store.clone())
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
    assert_stored_object_matches(
        stored_object(&store, database_failure_final_key.as_str()).await,
        &database_failure_quarantine,
        &database_failure_final_key,
    );
    assert_stored_object_matches(
        stored_object(&store, &database_failure.object_key).await,
        &database_failure_quarantine,
        &object_key(&database_failure.object_key),
    );
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
        .with_clamav()
        .workspace("workspace", "Worker handler workspace")
        .with_default_membership()
        .build()
        .await
}

struct UploadedAttachment {
    id: Uuid,
    object_key: String,
    filename: String,
}

async fn upload_attachment(
    app: &TestApp,
    workspace_id: Uuid,
    submission_id: Uuid,
    filename: &str,
) -> UploadedAttachment {
    upload_attachment_with_content(
        app,
        workspace_id,
        submission_id,
        filename,
        b"handler integration attachment",
    )
    .await
}

async fn upload_attachment_with_content(
    app: &TestApp,
    workspace_id: Uuid,
    submission_id: Uuid,
    filename: &str,
    content: &[u8],
) -> UploadedAttachment {
    let response = app
        .post(&format!(
            "/workspaces/{workspace_id}/evidence-submissions/{submission_id}/attachments"
        ))
        .multipart(attachment_form(content, filename, "text/plain", None))
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
        filename: filename.to_owned(),
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

async fn scanner_for(app: &TestApp) -> Arc<ClamAvMalwareScanner> {
    scanner_with_address(app, app.clamav_address(), StdDuration::from_secs(30)).await
}

async fn scanner_with_address(
    app: &TestApp,
    address: std::net::SocketAddr,
    scan_timeout: StdDuration,
) -> Arc<ClamAvMalwareScanner> {
    let object_store = Arc::new(
        proofplane::object_storage::FilesystemObjectStore::new(app.object_storage_root())
            .await
            .expect("filesystem object store initializes"),
    );
    Arc::new(ClamAvMalwareScanner::new(
        object_store,
        address,
        StdDuration::from_millis(100),
        scan_timeout,
    ))
}

async fn unavailable_address() -> std::net::SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("unused address binds");
    let address = listener.local_addr().expect("unused address is available");
    drop(listener);
    address
}

async fn clamd_error_address() -> std::net::SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("clamd error server binds");
    let address = listener
        .local_addr()
        .expect("clamd error address is available");
    tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.expect("scanner connects");
        let mut command = [0_u8; 10];
        tokio::io::AsyncReadExt::read_exact(&mut stream, &mut command)
            .await
            .expect("INSTREAM command reads");
        assert_eq!(&command, b"zINSTREAM\0");

        loop {
            let mut length = [0_u8; 4];
            tokio::io::AsyncReadExt::read_exact(&mut stream, &mut length)
                .await
                .expect("INSTREAM chunk length reads");
            let length = u32::from_be_bytes(length) as usize;
            if length == 0 {
                break;
            }
            let mut chunk = vec![0_u8; length];
            tokio::io::AsyncReadExt::read_exact(&mut stream, &mut chunk)
                .await
                .expect("INSTREAM chunk reads");
        }
        tokio::io::AsyncWriteExt::write_all(&mut stream, b"stream: injected scan failure ERROR\0")
            .await
            .expect("scan error response writes");
    });
    address
}

async fn hanging_clamd_address() -> std::net::SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("hanging clamd server binds");
    let address = listener
        .local_addr()
        .expect("hanging clamd address is available");
    tokio::spawn(async move {
        let (_stream, _) = listener.accept().await.expect("scanner connects");
        tokio::time::sleep(StdDuration::from_secs(1)).await;
    });
    address
}

async fn filesystem_object_store(app: &TestApp) -> Arc<FilesystemObjectStore> {
    Arc::new(
        FilesystemObjectStore::new(app.object_storage_root())
            .await
            .expect("filesystem object store initializes"),
    )
}

fn object_key(value: &str) -> ObjectKey {
    ObjectKey::parse(value).expect("object key is valid")
}

fn final_object_key(
    workspace_id: Uuid,
    submission_id: Uuid,
    attachment: &UploadedAttachment,
) -> ObjectKey {
    ObjectKey::parse(format!(
        "workspaces/{workspace_id}/evidence-submissions/{submission_id}/attachments/{}/{}",
        attachment.id, attachment.filename
    ))
    .expect("final object key is valid")
}

async fn stored_object(store: &FilesystemObjectStore, key: &str) -> ObjectStream {
    store
        .get_object(&object_key(key))
        .await
        .expect("stored object loads")
}

fn assert_stored_object_matches(
    actual: ObjectStream,
    expected: &ObjectStream,
    expected_key: &ObjectKey,
) {
    assert_eq!(actual.bytes, expected.bytes);
    assert_eq!(actual.metadata.key, *expected_key);
    assert_eq!(actual.metadata.content_type, expected.metadata.content_type);
    assert_eq!(
        actual.metadata.content_length,
        expected.metadata.content_length
    );
    assert_eq!(actual.metadata.sha256, expected.metadata.sha256);
}

async fn assert_object_missing(store: &FilesystemObjectStore, key: &str) {
    assert!(!object_path(store.root(), key).exists());
    assert!(!metadata_path(store.root(), key).exists());
    assert!(matches!(
        store.head_object(&object_key(key)).await,
        Err(StorageError::NotFound)
    ));
    assert!(matches!(
        store.get_object(&object_key(key)).await,
        Err(StorageError::NotFound)
    ));
}

fn object_path(root: &std::path::Path, key: &str) -> std::path::PathBuf {
    key.split('/')
        .fold(root.join("objects"), |path, segment| path.join(segment))
}

fn metadata_path(root: &std::path::Path, key: &str) -> std::path::PathBuf {
    let mut path = key
        .split('/')
        .fold(root.join("metadata"), |path, segment| path.join(segment));
    let filename = path
        .file_name()
        .expect("object key has a filename")
        .to_string_lossy();
    path.set_file_name(format!("{filename}.json"));
    path
}

#[cfg(unix)]
struct ReadOnlyDirectoryGuard {
    path: std::path::PathBuf,
    original_permissions: Option<std::fs::Permissions>,
}

#[cfg(unix)]
impl ReadOnlyDirectoryGuard {
    fn new(path: std::path::PathBuf) -> Self {
        use std::os::unix::fs::PermissionsExt;

        let original_permissions = std::fs::metadata(&path)
            .expect("quarantine directory metadata loads")
            .permissions();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o500))
            .expect("quarantine directory becomes read-only");
        Self {
            path,
            original_permissions: Some(original_permissions),
        }
    }

    fn restore(mut self) {
        let original_permissions = self
            .original_permissions
            .take()
            .expect("original permissions are present");
        std::fs::set_permissions(&self.path, original_permissions)
            .expect("quarantine directory permissions restore");
    }
}

#[cfg(unix)]
impl Drop for ReadOnlyDirectoryGuard {
    fn drop(&mut self) {
        if let Some(original_permissions) = self.original_permissions.take() {
            let _ = std::fs::set_permissions(&self.path, original_permissions);
        }
    }
}
