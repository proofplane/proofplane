use std::{sync::Arc, time::Duration as StdDuration};

use axum::http::StatusCode;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use chrono::{Duration, Utc};
use futures_util::StreamExt;
use proofplane::{
    handlers::{
        document_finalization::DocumentFinalizationHandler, document_scan::DocumentScanHandler,
    },
    object_storage::{FilesystemObjectStore, ObjectKey, ObjectStore, StorageError},
    repository::OutboxMessage,
    scanner::ClamAvMalwareScanner,
    worker::{WorkerMessage, DOCUMENT_FINALIZATION_REQUESTED, DOCUMENT_SCAN_REQUESTED},
};
use serde_json::{json, Value};
use uuid::Uuid;

use super::support::{submit_evidence_file, TestApp};

const EICAR: &[u8] = b"X5O!P%@AP[4\\PZX54(P^)7CC)7}$EICAR-STANDARD-ANTIVIRUS-TEST-FILE!$H+H*";

#[tokio::test]
async fn document_worker_handlers_are_idempotent_for_duplicate_deliveries() {
    let app = TestApp::builder()
        .with_clamav()
        .workspace("workspace", "Worker idempotency workspace")
        .with_default_membership()
        .build()
        .await;
    let workspace_id = app.workspace_id("workspace");
    let evidence_id = create_submission(&app, workspace_id).await;
    let uploaded = submit_evidence_file(
        &app,
        workspace_id,
        evidence_id,
        "2026-01-01T00:00:00Z",
        "2026-01-31T23:59:59Z",
        "artifact.txt",
        b"clean document",
    )
    .await;
    let submission_id = uuid_field(&uploaded, "submission_id");
    let document = &uploaded["document"];
    let document_id = document["id"].as_str().expect("document ID is a string");
    let document_uuid = Uuid::parse_str(document_id).expect("document ID is a UUID");
    let quarantine_key = document_object_key(&app, document_uuid).await;

    let worker = app.worker_server().await;
    let scan_message = outbox_message(&app, DOCUMENT_SCAN_REQUESTED, document_id).await;

    deliver_twice(&worker, &scan_message).await;

    let finalization_messages =
        outbox_messages(&app, DOCUMENT_FINALIZATION_REQUESTED, document_id).await;
    assert_eq!(
        finalization_messages.len(),
        1,
        "duplicate scan delivery must enqueue finalization once"
    );

    deliver_twice(&worker, &finalization_messages[0]).await;

    assert_eq!(document_status(&app, document_uuid).await, "uploaded");

    let final_key = document_object_key(&app, document_uuid).await;
    assert_eq!(
        final_key,
        format!(
            "workspaces/{workspace_id}/evidence-submissions/{submission_id}/documents/{document_id}/artifact.txt"
        )
    );
    assert!(app
        .object_storage_root()
        .join("objects")
        .join(&final_key)
        .exists());
    assert!(!app
        .object_storage_root()
        .join("objects")
        .join(&quarantine_key)
        .exists());

    assert_eq!(
        outbox_messages(&app, DOCUMENT_FINALIZATION_REQUESTED, document_id)
            .await
            .len(),
        1,
        "duplicate finalization delivery must not create additional work"
    );
}

#[tokio::test]
async fn document_scan_handler_coordinates_concrete_postgres_outcomes_and_retries() {
    let app = worker_test_app().await;
    let workspace_id = app.workspace_id("workspace");
    let submission_id = create_submission(&app, workspace_id).await;

    let clean = upload_document(&app, workspace_id, submission_id, "clean.txt").await;
    let request_id = Uuid::new_v4();
    let clean_handler = DocumentScanHandler::new(
        app.postgres.clone(),
        filesystem_object_store(&app).await,
        scanner_for(&app).await,
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

    assert_eq!(document_status(&app, clean.id).await, "finalizing");
    let finalization_messages =
        outbox_messages(&app, DOCUMENT_FINALIZATION_REQUESTED, &clean.id.to_string()).await;
    assert_eq!(finalization_messages.len(), 1);
    assert_eq!(finalization_messages[0].request_id, Some(request_id));

    let malicious =
        upload_document_with_content(&app, workspace_id, submission_id, "malicious.txt", EICAR)
            .await;
    DocumentScanHandler::new(
        app.postgres.clone(),
        filesystem_object_store(&app).await,
        scanner_for(&app).await,
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
    assert_eq!(document_status(&app, malicious.id).await, "contains_virus");

    let failed = upload_document(&app, workspace_id, submission_id, "failed.txt").await;
    DocumentScanHandler::new(
        app.postgres.clone(),
        filesystem_object_store(&app).await,
        scanner_with_address(&app, clamd_error_address().await, StdDuration::from_secs(1)).await,
        5,
    )
    .handle_scan_requested(scan_worker_message(&failed, submission_id, None, Some(1)))
    .await
    .expect("failed scan outcome is persisted");
    assert_eq!(document_status(&app, failed.id).await, "failed");

    let retry = upload_document(&app, workspace_id, submission_id, "retry.txt").await;
    let retry_handler = DocumentScanHandler::new(
        app.postgres.clone(),
        filesystem_object_store(&app).await,
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
    assert_eq!(document_status(&app, retry.id).await, "pending");
    retry_handler
        .handle_scan_requested(scan_worker_message(&retry, submission_id, None, Some(5)))
        .await
        .expect("final delivery persists failure");
    assert_eq!(document_status(&app, retry.id).await, "failed");

    let timed_out = upload_document(&app, workspace_id, submission_id, "timeout.txt").await;
    let timeout_handler = DocumentScanHandler::new(
        app.postgres.clone(),
        filesystem_object_store(&app).await,
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
    assert_eq!(document_status(&app, timed_out.id).await, "pending");

    let missing = upload_document(&app, workspace_id, submission_id, "missing.txt").await;
    let store = filesystem_object_store(&app).await;
    store
        .delete_object(&object_key(&missing.object_key))
        .await
        .expect("quarantined object is deleted");
    DocumentScanHandler::new(
        app.postgres.clone(),
        store.clone(),
        scanner_for(&app).await,
        5,
    )
    .handle_scan_requested(scan_worker_message(&missing, submission_id, None, Some(1)))
    .await
    .expect("missing object is a terminal outcome");
    assert_eq!(document_status(&app, missing.id).await, "failed");

    let mismatched =
        upload_document(&app, workspace_id, submission_id, "metadata-mismatch.txt").await;
    let sidecar = metadata_path(app.object_storage_root(), &mismatched.object_key);
    let mut metadata: Value = serde_json::from_slice(
        &tokio::fs::read(&sidecar)
            .await
            .expect("object metadata reads"),
    )
    .expect("object metadata parses");
    metadata["content_type"] = Value::String("application/octet-stream".to_owned());
    tokio::fs::write(
        &sidecar,
        serde_json::to_vec_pretty(&metadata).expect("object metadata serializes"),
    )
    .await
    .expect("mismatched object metadata writes");
    let mismatch_handler =
        DocumentScanHandler::new(app.postgres.clone(), store, scanner_for(&app).await, 5);
    assert!(mismatch_handler
        .handle_scan_requested(scan_worker_message(
            &mismatched,
            submission_id,
            None,
            Some(1),
        ))
        .await
        .is_err());
    assert_eq!(document_status(&app, mismatched.id).await, "pending");
    mismatch_handler
        .handle_scan_requested(scan_worker_message(
            &mismatched,
            submission_id,
            None,
            Some(5),
        ))
        .await
        .expect("final metadata mismatch is persisted");
    assert_eq!(document_status(&app, mismatched.id).await, "failed");
}

#[tokio::test]
async fn document_scan_handler_rolls_back_update_and_outbox_failures() {
    let app = worker_test_app().await;
    let workspace_id = app.workspace_id("workspace");
    let submission_id = create_submission(&app, workspace_id).await;
    let handler = DocumentScanHandler::new(
        app.postgres.clone(),
        filesystem_object_store(&app).await,
        scanner_for(&app).await,
        5,
    );

    let update_failure =
        upload_document(&app, workspace_id, submission_id, "update-failure.txt").await;
    install_failure_trigger(
        &app,
        "documents",
        "document_update_failure",
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
    assert_eq!(document_status(&app, update_failure.id).await, "pending");
    remove_failure_trigger(&app, "documents", "document_update_failure").await;

    let outbox_failure =
        upload_document(&app, workspace_id, submission_id, "outbox-failure.txt").await;
    install_failure_trigger(
        &app,
        "outbox_messages",
        "document_outbox_failure",
        "BEFORE INSERT",
        "NEW.event_type = 'document.finalization_requested'",
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
    assert_eq!(document_status(&app, outbox_failure.id).await, "pending");
    assert!(outbox_messages(
        &app,
        DOCUMENT_FINALIZATION_REQUESTED,
        &outbox_failure.id.to_string()
    )
    .await
    .is_empty());
    remove_failure_trigger(&app, "outbox_messages", "document_outbox_failure").await;
}

#[tokio::test]
async fn document_finalization_handler_uses_concrete_postgres_and_external_store_failures() {
    let app = worker_test_app().await;
    let workspace_id = app.workspace_id("workspace");
    let submission_id = create_submission(&app, workspace_id).await;
    let scanner = DocumentScanHandler::new(
        app.postgres.clone(),
        filesystem_object_store(&app).await,
        scanner_for(&app).await,
        5,
    );

    let successful = upload_document(&app, workspace_id, submission_id, "successful.txt").await;
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
    DocumentFinalizationHandler::new(app.postgres.clone(), store.clone())
        .handle_finalization_requested(finalization_worker_message(&successful, submission_id))
        .await
        .expect("finalization succeeds");
    assert_stored_object_matches(
        stored_object(&store, successful_final_key.as_str()).await,
        &successful_quarantine,
        &successful_final_key,
    );
    assert_object_missing(&store, &successful.object_key).await;

    DocumentFinalizationHandler::new(app.postgres.clone(), store.clone())
        .handle_finalization_requested(finalization_worker_message(&successful, submission_id))
        .await
        .expect("duplicate finalization is acknowledged");
    assert_eq!(document_status(&app, successful.id).await, "uploaded");
    assert_stored_object_matches(
        stored_object(&store, successful_final_key.as_str()).await,
        &successful_quarantine,
        &successful_final_key,
    );
    assert_object_missing(&store, &successful.object_key).await;

    #[cfg(unix)]
    {
        let delete_failure =
            upload_document(&app, workspace_id, submission_id, "delete-failure.txt").await;
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

        DocumentFinalizationHandler::new(app.postgres.clone(), store.clone())
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
        assert_eq!(document_status(&app, delete_failure.id).await, "uploaded");
        permission_guard.restore();
    }

    let copy_failure = upload_document(&app, workspace_id, submission_id, "copy-failure.txt").await;
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
        DocumentFinalizationHandler::new(app.postgres.clone(), store.clone())
            .handle_finalization_requested(finalization_worker_message(
                &copy_failure,
                submission_id,
            ))
            .await
            .is_err()
    );
    assert_eq!(document_status(&app, copy_failure.id).await, "finalizing");
    assert_object_missing(
        &store,
        final_object_key(workspace_id, submission_id, &copy_failure).as_str(),
    )
    .await;

    let database_failure =
        upload_document(&app, workspace_id, submission_id, "database-failure.txt").await;
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
        "documents",
        "document_uploaded_failure",
        "BEFORE UPDATE",
        "NEW.upload_status = 'uploaded'",
    )
    .await;
    let database_failure_quarantine = stored_object(&store, &database_failure.object_key).await;
    let database_failure_final_key =
        final_object_key(workspace_id, submission_id, &database_failure);
    assert!(
        DocumentFinalizationHandler::new(app.postgres.clone(), store.clone())
            .handle_finalization_requested(finalization_worker_message(
                &database_failure,
                submission_id,
            ))
            .await
            .is_err()
    );
    assert_eq!(
        document_status(&app, database_failure.id).await,
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
    remove_failure_trigger(&app, "documents", "document_uploaded_failure").await;
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
        .create_evidence(
            workspace_id,
            &json!({
                "title": "Worker handler target",
                "description": "Integration fixture",
                "collection_instructions": "Upload the worker integration fixture.",
                "status": "active",
            }),
        )
        .await;
    uuid_field(&request, "id")
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

struct UploadedDocument {
    id: Uuid,
    submission_id: Uuid,
    object_key: String,
    filename: String,
}

async fn upload_document(
    app: &TestApp,
    workspace_id: Uuid,
    evidence_id: Uuid,
    filename: &str,
) -> UploadedDocument {
    upload_document_with_content(
        app,
        workspace_id,
        evidence_id,
        filename,
        b"handler integration document",
    )
    .await
}

async fn upload_document_with_content(
    app: &TestApp,
    workspace_id: Uuid,
    evidence_id: Uuid,
    filename: &str,
    content: &[u8],
) -> UploadedDocument {
    let uploaded = submit_evidence_file(
        app,
        workspace_id,
        evidence_id,
        "2026-01-01T00:00:00Z",
        "2026-01-31T23:59:59Z",
        filename,
        content,
    )
    .await;
    let document = &uploaded["document"];

    UploadedDocument {
        submission_id: uuid_field(&uploaded, "submission_id"),
        id: Uuid::parse_str(document["id"].as_str().expect("document ID is a string"))
            .expect("document ID is a UUID"),
        object_key: document_object_key(
            app,
            Uuid::parse_str(document["id"].as_str().expect("document ID is a string"))
                .expect("document ID is a UUID"),
        )
        .await,
        filename: filename.to_owned(),
    }
}

async fn document_object_key(app: &TestApp, document_id: Uuid) -> String {
    app.postgres()
        .get()
        .await
        .expect("connection opens")
        .query_one(
            "SELECT object_key FROM documents WHERE id = $1",
            &[&document_id],
        )
        .await
        .expect("document object key loads")
        .get("object_key")
}

fn scan_worker_message(
    document: &UploadedDocument,
    _submission_id: Uuid,
    request_id: Option<Uuid>,
    delivery_attempt: Option<u32>,
) -> WorkerMessage {
    WorkerMessage {
        message_id: Uuid::new_v4().to_string(),
        event_type: DOCUMENT_SCAN_REQUESTED.to_owned(),
        aggregate_type: "evidence_document".to_owned(),
        aggregate_id: document.id.to_string(),
        request_id,
        payload: json!({
            "evidence_submission_id": document.submission_id.to_string(),
            "object_key": document.object_key,
        }),
        delivery_attempt,
    }
}

fn finalization_worker_message(document: &UploadedDocument, _submission_id: Uuid) -> WorkerMessage {
    WorkerMessage {
        message_id: Uuid::new_v4().to_string(),
        event_type: DOCUMENT_FINALIZATION_REQUESTED.to_owned(),
        aggregate_type: "evidence_document".to_owned(),
        aggregate_id: document.id.to_string(),
        request_id: None,
        payload: json!({
            "evidence_submission_id": document.submission_id.to_string(),
            "object_key": document.object_key,
        }),
        delivery_attempt: Some(1),
    }
}

async fn document_status(app: &TestApp, document_id: Uuid) -> String {
    let client = app.postgres().get().await.expect("connection opens");
    client
        .query_one(
            "SELECT upload_status FROM documents WHERE id = $1",
            &[&document_id],
        )
        .await
        .expect("document status loads")
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
    _app: &TestApp,
    address: std::net::SocketAddr,
    scan_timeout: StdDuration,
) -> Arc<ClamAvMalwareScanner> {
    Arc::new(ClamAvMalwareScanner::new(
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
    _submission_id: Uuid,
    document: &UploadedDocument,
) -> ObjectKey {
    ObjectKey::parse(format!(
        "workspaces/{workspace_id}/evidence-submissions/{}/documents/{}/{}",
        document.submission_id, document.id, document.filename
    ))
    .expect("final object key is valid")
}

struct StoredObject {
    metadata: proofplane::object_storage::ObjectMetadata,
    bytes: Vec<u8>,
}

async fn stored_object(store: &FilesystemObjectStore, key: &str) -> StoredObject {
    let object = store
        .get_object(&object_key(key))
        .await
        .expect("stored object loads");
    let metadata = object.metadata;
    let bytes = object
        .chunks
        .map(|chunk| chunk.expect("stored object chunk reads"))
        .fold(Vec::new(), |mut bytes, chunk| async move {
            bytes.extend_from_slice(&chunk);
            bytes
        })
        .await;
    StoredObject { metadata, bytes }
}

fn assert_stored_object_matches(
    actual: StoredObject,
    expected: &StoredObject,
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
