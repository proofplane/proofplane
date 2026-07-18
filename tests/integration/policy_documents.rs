use std::{sync::Arc, time::Duration as StdDuration};

use axum::http::StatusCode;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use bytes::Bytes;
use chrono::{Duration, Utc};
use futures_util::stream;
use proofplane::{
    domain::{CreatePolicyPayload, DocumentId, PolicyId},
    handlers::document_scan::DocumentScanHandler,
    object_storage::{FilesystemObjectStore, ObjectKey, ObjectStore},
    repository::{ArchiveDocumentResult, CreatePolicyDocumentResult, OutboxMessage},
    scanner::ClamAvMalwareScanner,
    services::{
        policies::PolicyService,
        policy_documents::{PolicyDocumentService, UploadPolicyDocumentPayload},
    },
    worker::{WorkerMessage, DOCUMENT_FINALIZATION_REQUESTED, DOCUMENT_SCAN_REQUESTED},
};
use serde_json::{json, Value};
use uuid::Uuid;

use super::support::{capture_audit_logs, TestApp};

const EICAR: &[u8] = b"X5O!P%@AP[4\\PZX54(P^)7CC)7}$EICAR-STANDARD-ANTIVIRUS-TEST-FILE!$H+H*";

#[tokio::test]
async fn policy_document_create_race_cleans_loser_and_terminal_archive_allows_replacement() {
    let app = TestApp::builder()
        .workspace("workspace", "Policy document workspace")
        .with_default_membership()
        .build()
        .await;
    let workspace_id = app.workspace_id("workspace");
    let connection = app.agent_connection_context(workspace_id);
    let policy_id = create_policy(&app, workspace_id, "Document policy").await;
    let service = policy_document_service(&app).await;
    let first = stage(&service, &connection, policy_id, "first.txt", b"first").await;
    let second = stage(&service, &connection, policy_id, "second.txt", b"second").await;
    let first_key = first.object_key.clone();
    let second_key = second.object_key.clone();

    let first_service = service.clone();
    let second_service = service.clone();
    let (first_result, second_result) = tokio::join!(
        first_service.create(&connection, Uuid::new_v4(), policy_id, first),
        second_service.create(&connection, Uuid::new_v4(), policy_id, second),
    );
    let results = [
        first_result.expect("first create resolves"),
        second_result.expect("second create resolves"),
    ];
    assert_eq!(
        results
            .iter()
            .filter(|result| matches!(result, CreatePolicyDocumentResult::Created(_)))
            .count(),
        1
    );
    assert_eq!(
        results
            .iter()
            .filter(|result| matches!(result, CreatePolicyDocumentResult::DocumentExists))
            .count(),
        1
    );
    assert_eq!(active_document_count(&app, policy_id).await, 1);
    assert_eq!(
        outbox_messages(&app, DOCUMENT_SCAN_REQUESTED, None)
            .await
            .len(),
        1
    );
    let first_exists = stored_object_path(&app, &first_key).exists();
    let second_exists = stored_object_path(&app, &second_key).exists();
    assert_ne!(
        first_exists, second_exists,
        "only the winning staged object remains"
    );

    let document = service
        .get_current(&connection, policy_id)
        .await
        .expect("current document reads")
        .expect("current document exists");
    assert_eq!(
        service
            .archive(&connection, Uuid::new_v4(), policy_id, document.id(),)
            .await
            .expect("pending archive resolves"),
        ArchiveDocumentResult::NotTerminal
    );
    set_document_status(&app, document.id(), "failed").await;
    let (archive_result, audits) = capture_audit_logs(|request_id| {
        service.archive(&connection, request_id, policy_id, document.id())
    })
    .await;
    assert_eq!(
        archive_result.expect("terminal archive resolves"),
        ArchiveDocumentResult::Archived
    );
    assert_eq!(audits.len(), 1);
    assert_eq!(
        audits[0]["fields"]["event_name"],
        "policy_document.archived"
    );
    assert!(service
        .get_current(&connection, policy_id)
        .await
        .expect("archived current document reads")
        .is_none());

    let replacement = stage(
        &service,
        &connection,
        policy_id,
        "replacement.txt",
        b"replacement",
    )
    .await;
    let (replacement_result, audits) = capture_audit_logs(|request_id| {
        service.create(&connection, request_id, policy_id, replacement)
    })
    .await;
    assert!(matches!(
        replacement_result.expect("replacement creates"),
        CreatePolicyDocumentResult::Created(_)
    ));
    assert_eq!(audits.len(), 1);
    assert_eq!(
        audits[0]["fields"]["event_name"],
        "policy_document.accepted"
    );
    let serialized_audit = audits[0].to_string();
    for sensitive in ["replacement.txt", "checksum", "object_key"] {
        assert!(!serialized_audit.contains(sensitive));
    }
}

#[tokio::test]
async fn policy_document_rejects_missing_archived_and_cross_workspace_owners_and_cleans_bytes() {
    let app = TestApp::builder()
        .workspace("workspace", "Policy owner workspace")
        .with_default_membership()
        .workspace("other", "Other policy workspace")
        .with_default_membership()
        .build()
        .await;
    let workspace_id = app.workspace_id("workspace");
    let other_workspace_id = app.workspace_id("other");
    let connection = app.agent_connection_context(workspace_id);
    let service = policy_document_service(&app).await;
    let other_policy = create_policy(&app, other_workspace_id, "Other policy").await;

    for policy_id in [PolicyId::from(Uuid::new_v4()), other_policy] {
        let payload = stage(
            &service,
            &connection,
            policy_id,
            "unavailable.txt",
            b"unavailable",
        )
        .await;
        let key = payload.object_key.clone();
        assert_eq!(
            service
                .create(&connection, Uuid::new_v4(), policy_id, payload)
                .await
                .expect("unavailable policy resolves"),
            CreatePolicyDocumentResult::PolicyNotFound
        );
        assert!(!stored_object_path(&app, &key).exists());
    }

    let archived = create_policy(&app, workspace_id, "Archived policy").await;
    PolicyService::new(app.postgres_arc())
        .archive(connection, archived)
        .await
        .expect("policy archive resolves");
    let payload = stage(&service, &connection, archived, "archived.txt", b"archived").await;
    let key = payload.object_key.clone();
    assert_eq!(
        service
            .create(&connection, Uuid::new_v4(), archived, payload)
            .await
            .expect("archived policy resolves"),
        CreatePolicyDocumentResult::PolicyNotFound
    );
    assert!(!stored_object_path(&app, &key).exists());
}

#[tokio::test]
async fn policy_worker_messages_are_owner_checked_idempotent_and_finalize_in_policy_namespace() {
    let app = TestApp::builder()
        .with_clamav()
        .workspace("workspace", "Policy worker workspace")
        .with_default_membership()
        .build()
        .await;
    let workspace_id = app.workspace_id("workspace");
    let connection = app.agent_connection_context(workspace_id);
    let policy_id = create_policy(&app, workspace_id, "Clean policy").await;
    let other_policy_id = create_policy(&app, workspace_id, "Other policy").await;
    let service = policy_document_service(&app).await;
    let document = create_document(
        &service,
        &connection,
        policy_id,
        "clean.txt",
        b"clean policy document",
    )
    .await;
    let worker = app.worker_server().await;
    let scan = outbox_messages(
        &app,
        DOCUMENT_SCAN_REQUESTED,
        Some(Uuid::from(document.id())),
    )
    .await
    .remove(0);

    let mut wrong_owner = scan.clone();
    wrong_owner.payload["policy_id"] = Value::String(Uuid::from(other_policy_id).to_string());
    deliver(&worker, &wrong_owner).await;
    assert_eq!(document_status(&app, document.id()).await, "pending");

    deliver(&worker, &scan).await;
    deliver(&worker, &scan).await;
    assert_eq!(document_status(&app, document.id()).await, "finalizing");
    let finalizations = outbox_messages(
        &app,
        DOCUMENT_FINALIZATION_REQUESTED,
        Some(Uuid::from(document.id())),
    )
    .await;
    assert_eq!(finalizations.len(), 1);
    deliver(&worker, &finalizations[0]).await;
    deliver(&worker, &finalizations[0]).await;
    assert_eq!(document_status(&app, document.id()).await, "uploaded");
    let final_key = document_object_key(&app, document.id()).await;
    assert_eq!(
        final_key,
        format!(
            "workspaces/{workspace_id}/policies/{policy_id}/documents/{}/clean.txt",
            Uuid::from(document.id())
        )
    );
    assert!(stored_object_path(&app, &final_key).exists());

    let malicious_policy = create_policy(&app, workspace_id, "Malicious policy").await;
    let malicious = create_document(
        &service,
        &connection,
        malicious_policy,
        "malicious.txt",
        EICAR,
    )
    .await;
    let malicious_scan = outbox_messages(
        &app,
        DOCUMENT_SCAN_REQUESTED,
        Some(Uuid::from(malicious.id())),
    )
    .await
    .remove(0);
    deliver(&worker, &malicious_scan).await;
    assert_eq!(
        document_status(&app, malicious.id()).await,
        "contains_virus"
    );
}

#[tokio::test]
async fn policy_scan_retries_then_fails_and_missing_quarantine_is_terminal() {
    let app = TestApp::builder()
        .workspace("workspace", "Policy retry workspace")
        .with_default_membership()
        .build()
        .await;
    let workspace_id = app.workspace_id("workspace");
    let connection = app.agent_connection_context(workspace_id);
    let policy_id = create_policy(&app, workspace_id, "Retry policy").await;
    let store = Arc::new(
        FilesystemObjectStore::new(app.object_storage_root())
            .await
            .expect("retry object store initializes"),
    );
    let service = PolicyDocumentService::new(app.postgres_arc(), store.clone());
    let retry = create_document(
        &service,
        &connection,
        policy_id,
        "retry.txt",
        b"retry policy document",
    )
    .await;
    let retry_outbox = outbox_messages(&app, DOCUMENT_SCAN_REQUESTED, Some(Uuid::from(retry.id())))
        .await
        .remove(0);
    let handler = DocumentScanHandler::new(
        app.postgres_arc(),
        store.clone(),
        Arc::new(ClamAvMalwareScanner::new(
            unavailable_address().await,
            StdDuration::from_millis(100),
            StdDuration::from_millis(100),
        )),
        2,
    );
    let mut retry_message = worker_message(&retry_outbox, 1);
    assert!(handler
        .handle_scan_requested(retry_message.clone())
        .await
        .is_err());
    assert_eq!(document_status(&app, retry.id()).await, "pending");
    retry_message.delivery_attempt = Some(2);
    handler
        .handle_scan_requested(retry_message)
        .await
        .expect("final retry persists failure");
    assert_eq!(document_status(&app, retry.id()).await, "failed");

    let missing_policy = create_policy(&app, workspace_id, "Missing object policy").await;
    let missing = create_document(
        &service,
        &connection,
        missing_policy,
        "missing.txt",
        b"missing policy document",
    )
    .await;
    let missing_key = document_object_key(&app, missing.id()).await;
    store
        .delete_object(&ObjectKey::parse(missing_key).expect("quarantine key parses"))
        .await
        .expect("quarantine object deletes");
    let missing_outbox = outbox_messages(
        &app,
        DOCUMENT_SCAN_REQUESTED,
        Some(Uuid::from(missing.id())),
    )
    .await
    .remove(0);
    handler
        .handle_scan_requested(worker_message(&missing_outbox, 1))
        .await
        .expect("missing object is terminal");
    assert_eq!(document_status(&app, missing.id()).await, "failed");

    let mismatch_policy = create_policy(&app, workspace_id, "Metadata mismatch policy").await;
    let mismatch = create_document(
        &service,
        &connection,
        mismatch_policy,
        "mismatch.txt",
        b"metadata mismatch",
    )
    .await;
    let mismatch_key = document_object_key(&app, mismatch.id()).await;
    let sidecar = metadata_path(app.object_storage_root(), &mismatch_key);
    let mut metadata: Value = serde_json::from_slice(
        &tokio::fs::read(&sidecar)
            .await
            .expect("policy object metadata reads"),
    )
    .expect("policy object metadata parses");
    metadata["content_type"] = Value::String("application/octet-stream".to_owned());
    tokio::fs::write(
        &sidecar,
        serde_json::to_vec_pretty(&metadata).expect("policy object metadata serializes"),
    )
    .await
    .expect("policy object metadata writes");
    let mismatch_outbox = outbox_messages(
        &app,
        DOCUMENT_SCAN_REQUESTED,
        Some(Uuid::from(mismatch.id())),
    )
    .await
    .remove(0);
    assert!(handler
        .handle_scan_requested(worker_message(&mismatch_outbox, 1))
        .await
        .is_err());
    assert_eq!(document_status(&app, mismatch.id()).await, "pending");
    handler
        .handle_scan_requested(worker_message(&mismatch_outbox, 2))
        .await
        .expect("final metadata mismatch persists failure");
    assert_eq!(document_status(&app, mismatch.id()).await, "failed");
}

async fn policy_document_service(app: &TestApp) -> PolicyDocumentService {
    let store = Arc::new(
        FilesystemObjectStore::new(app.object_storage_root())
            .await
            .expect("policy document object store initializes"),
    );
    PolicyDocumentService::new(app.postgres_arc(), store)
}

async fn create_policy(app: &TestApp, workspace_id: Uuid, name: &str) -> PolicyId {
    PolicyService::new(app.postgres_arc())
        .create(
            app.agent_connection_context(workspace_id),
            CreatePolicyPayload {
                name: name.to_owned(),
                description: None,
                control_ids: vec![],
            },
        )
        .await
        .expect("policy creates")
        .policy
        .id
}

async fn stage(
    service: &PolicyDocumentService,
    connection: &proofplane::services::agent_connections::AgentConnectionContext,
    policy_id: PolicyId,
    filename: &str,
    content: &[u8],
) -> UploadPolicyDocumentPayload {
    service
        .upload(
            connection,
            policy_id,
            filename.to_owned(),
            "text/plain".to_owned(),
            stream::once({
                let bytes = Bytes::copy_from_slice(content);
                async move { Ok(bytes) }
            }),
        )
        .await
        .expect("policy document stages")
}

async fn create_document(
    service: &PolicyDocumentService,
    connection: &proofplane::services::agent_connections::AgentConnectionContext,
    policy_id: PolicyId,
    filename: &str,
    content: &[u8],
) -> proofplane::domain::Document {
    let payload = stage(service, connection, policy_id, filename, content).await;
    match service
        .create(connection, Uuid::new_v4(), policy_id, payload)
        .await
        .expect("policy document creates")
    {
        CreatePolicyDocumentResult::Created(document) => document,
        other => panic!("expected created document, got {other:?}"),
    }
}

async fn outbox_messages(
    app: &TestApp,
    event_type: &str,
    aggregate_id: Option<Uuid>,
) -> Vec<OutboxMessage> {
    app.postgres()
        .list_due_outbox_messages(Utc::now() + Duration::seconds(1), 100)
        .await
        .expect("outbox messages list")
        .into_iter()
        .filter(|message| {
            message.event_type == event_type
                && message.aggregate_type == "policy_document"
                && aggregate_id.is_none_or(|id| message.aggregate_id == id.to_string())
        })
        .collect()
}

async fn deliver(worker: &axum_test::TestServer, message: &OutboxMessage) {
    let data = json!({
        "event_type": message.event_type,
        "aggregate_type": message.aggregate_type,
        "aggregate_id": message.aggregate_id,
        "request_id": message.request_id,
        "payload": message.payload,
    });
    worker
        .post("/pubsub/messages")
        .json(&json!({
            "message": {
                "messageId": format!("outbox-{}", message.id),
                "data": STANDARD.encode(data.to_string()),
            },
            "deliveryAttempt": 1,
        }))
        .await
        .assert_status(StatusCode::NO_CONTENT);
}

fn worker_message(message: &OutboxMessage, delivery_attempt: u32) -> WorkerMessage {
    WorkerMessage {
        message_id: format!("outbox-{}", message.id),
        event_type: message.event_type.clone(),
        aggregate_type: message.aggregate_type.clone(),
        aggregate_id: message.aggregate_id.clone(),
        request_id: message.request_id,
        payload: message.payload.clone(),
        delivery_attempt: Some(delivery_attempt),
    }
}

async fn unavailable_address() -> std::net::SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("ephemeral address binds");
    let address = listener.local_addr().expect("ephemeral address reads");
    drop(listener);
    address
}

async fn active_document_count(app: &TestApp, policy_id: PolicyId) -> i64 {
    app.postgres()
        .get()
        .await
        .expect("connection opens")
        .query_one(
            "SELECT count(*) FROM documents WHERE owner_type = 'policy' AND owner_id = $1 AND archived = false",
            &[&Uuid::from(policy_id)],
        )
        .await
        .expect("document count loads")
        .get(0)
}

async fn set_document_status(app: &TestApp, document_id: DocumentId, status: &str) {
    app.postgres()
        .get()
        .await
        .expect("connection opens")
        .execute(
            "UPDATE documents SET upload_status = $2 WHERE id = $1",
            &[&Uuid::from(document_id), &status],
        )
        .await
        .expect("document status updates");
}

async fn document_status(app: &TestApp, document_id: DocumentId) -> String {
    app.postgres()
        .get()
        .await
        .expect("connection opens")
        .query_one(
            "SELECT upload_status FROM documents WHERE id = $1",
            &[&Uuid::from(document_id)],
        )
        .await
        .expect("document status loads")
        .get(0)
}

async fn document_object_key(app: &TestApp, document_id: DocumentId) -> String {
    app.postgres()
        .get()
        .await
        .expect("connection opens")
        .query_one(
            "SELECT object_key FROM documents WHERE id = $1",
            &[&Uuid::from(document_id)],
        )
        .await
        .expect("document key loads")
        .get(0)
}

fn stored_object_path(app: &TestApp, object_key: &str) -> std::path::PathBuf {
    app.object_storage_root().join("objects").join(object_key)
}

fn metadata_path(root: &std::path::Path, object_key: &str) -> std::path::PathBuf {
    let mut path = object_key
        .split('/')
        .fold(root.join("metadata"), |path, segment| path.join(segment));
    let filename = path
        .file_name()
        .expect("object key has a filename")
        .to_string_lossy();
    path.set_file_name(format!("{filename}.json"));
    path
}
