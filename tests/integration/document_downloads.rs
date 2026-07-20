use proofplane::{
    domain::{EvidenceSubmissionId, WorkspaceId},
    object_storage::{FilesystemObjectStore, ObjectKey, ObjectStore},
    routes::request_context::REQUEST_ID_HEADER,
    services::document_downloads::{DownloadError, DownloadGrantIssuer},
};
use serde_json::{json, Value};
use url::Url;
use uuid::Uuid;

use super::support::{capture_audit_logs, submit_evidence_file, TestApp};

#[tokio::test]
async fn uploaded_document_download_grant_is_reusable_and_streams_safe_headers() {
    let app = download_app().await;
    let workspace_id = app.workspace_id("workspace");
    let evidence_id = create_evidence(&app, workspace_id).await;
    let content = b"downloadable evidence";
    let uploaded = submit_evidence_file(
        &app,
        workspace_id,
        evidence_id,
        "2026-01-01T00:00:00Z",
        "2026-03-31T23:59:59Z",
        "Quarterly evidence (final).txt",
        content,
    )
    .await;
    let submission_id = uuid_field(&uploaded, "submission_id");
    let document_id = uuid_field(&uploaded["document"], "id");
    let final_key = finalize_document(&app, workspace_id, submission_id, document_id).await;

    let grant = app
        .document_download_service()
        .issue(
            WorkspaceId::from(workspace_id),
            app.user_id().into(),
            DownloadGrantIssuer::AgentConnection(app.api_token_id().into()),
            EvidenceSubmissionId::from(submission_id),
            document_id.into(),
        )
        .await
        .expect("download grant issues");
    assert_eq!(grant.filename, "Quarterly evidence (final).txt");
    assert_eq!(grant.content_type, "text/plain");
    assert_eq!(grant.content_length, content.len() as i64);
    let path = local_path(&grant.url);
    let token = token_from_url(&grant.url);

    let request = app.server().get(&path);
    let (first, logs) = capture_audit_logs(|request_id| async move {
        request
            .add_header(REQUEST_ID_HEADER, request_id.to_string())
            .await
    })
    .await;
    assert_download(&first, content, "Quarterly evidence (final).txt");
    let second = app.server().get(&path).await;
    assert_download(&second, content, "Quarterly evidence (final).txt");

    let logs = serde_json::to_string(&logs).expect("audit logs serialize");
    assert!(logs.contains("evidence_document_download_grant.redeemed"));
    assert!(logs.contains(&workspace_id.to_string()));
    assert!(logs.contains(&submission_id.to_string()));
    assert!(logs.contains(&document_id.to_string()));
    assert!(!logs.contains(&token));
    assert!(!logs.contains(final_key.as_str()));
    assert!(!logs.contains("downloadable evidence"));
}

#[tokio::test]
async fn download_grants_conceal_invalid_scope_tokens_and_terminal_documents() {
    let app = TestApp::builder()
        .workspace("workspace", "Download workspace")
        .with_default_membership()
        .workspace("other", "Other workspace")
        .with_default_membership()
        .build()
        .await;
    let workspace_id = app.workspace_id("workspace");
    let other_workspace_id = app.workspace_id("other");
    let evidence_id = create_evidence(&app, workspace_id).await;
    let uploaded = submit_evidence_file(
        &app,
        workspace_id,
        evidence_id,
        "2026-01-01T00:00:00Z",
        "2026-01-31T23:59:59Z",
        "scoped.txt",
        b"scoped",
    )
    .await;
    let submission_id = uuid_field(&uploaded, "submission_id");
    let document_id = uuid_field(&uploaded["document"], "id");
    let service = app.document_download_service();

    assert!(matches!(
        service
            .issue(
                workspace_id.into(),
                app.user_id().into(),
                DownloadGrantIssuer::AgentConnection(app.api_token_id().into()),
                submission_id.into(),
                document_id.into(),
            )
            .await,
        Err(DownloadError::NotReady)
    ));
    finalize_document(&app, workspace_id, submission_id, document_id).await;
    let grant = service
        .issue(
            workspace_id.into(),
            app.user_id().into(),
            DownloadGrantIssuer::AgentConnection(app.api_token_id().into()),
            submission_id.into(),
            document_id.into(),
        )
        .await
        .expect("eligible grant issues");
    let path = local_path(&grant.url);

    assert!(matches!(
        service
            .issue(
                other_workspace_id.into(),
                app.user_id().into(),
                DownloadGrantIssuer::AgentConnection(app.api_token_id().into()),
                submission_id.into(),
                document_id.into(),
            )
            .await,
        Err(DownloadError::NotFound)
    ));
    assert!(matches!(
        service
            .issue(
                workspace_id.into(),
                app.user_id().into(),
                DownloadGrantIssuer::AgentConnection(app.api_token_id().into()),
                Uuid::new_v4().into(),
                document_id.into(),
            )
            .await,
        Err(DownloadError::NotFound)
    ));

    for invalid in [
        "/document-downloads",
        "/document-downloads?other=value",
        "/document-downloads?token=",
        "/document-downloads?token=a&token=b",
        "/document-downloads?token=malformed",
    ] {
        app.server().get(invalid).await.assert_status_not_found();
    }
    app.server()
        .get(&tamper_last_character(&path))
        .await
        .assert_status_not_found();

    set_document_status(&app, document_id, "contains_virus").await;
    app.server().get(&path).await.assert_status_not_found();
    set_document_status(&app, document_id, "uploaded").await;
    archive_document(&app, document_id).await;
    app.server().get(&path).await.assert_status_not_found();
    assert!(matches!(
        service
            .issue(
                workspace_id.into(),
                app.user_id().into(),
                DownloadGrantIssuer::AgentConnection(app.api_token_id().into()),
                submission_id.into(),
                document_id.into(),
            )
            .await,
        Err(DownloadError::NotFound)
    ));
}

#[tokio::test]
async fn download_metadata_mismatch_is_internal_and_storage_details_stay_private() {
    let app = download_app().await;
    let workspace_id = app.workspace_id("workspace");
    let evidence_id = create_evidence(&app, workspace_id).await;
    let uploaded = submit_evidence_file(
        &app,
        workspace_id,
        evidence_id,
        "2026-04-01T00:00:00Z",
        "2026-06-30T23:59:59Z",
        "metadata.txt",
        b"metadata",
    )
    .await;
    let submission_id = uuid_field(&uploaded, "submission_id");
    let document_id = uuid_field(&uploaded["document"], "id");
    let final_key = finalize_document(&app, workspace_id, submission_id, document_id).await;
    let service = app.document_download_service();
    let grant = service
        .issue(
            workspace_id.into(),
            app.user_id().into(),
            DownloadGrantIssuer::AgentConnection(app.api_token_id().into()),
            submission_id.into(),
            document_id.into(),
        )
        .await
        .expect("download grant issues before metadata changes");

    let metadata_path = app
        .object_storage_root()
        .join("metadata")
        .join(format!("{}.json", final_key.as_str()));
    let mut metadata: Value =
        serde_json::from_slice(&std::fs::read(&metadata_path).expect("object metadata reads"))
            .expect("object metadata parses");
    metadata["sha256"] = Value::String("0".repeat(64));
    std::fs::write(
        metadata_path,
        serde_json::to_vec_pretty(&metadata).expect("object metadata serializes"),
    )
    .expect("object metadata updates");

    app.server()
        .get(&local_path(&grant.url))
        .await
        .assert_status_internal_server_error();
    assert!(matches!(
        service
            .issue(
                workspace_id.into(),
                app.user_id().into(),
                DownloadGrantIssuer::AgentConnection(app.api_token_id().into()),
                submission_id.into(),
                document_id.into(),
            )
            .await,
        Err(DownloadError::MetadataMismatch)
    ));

    let public_error = app.server().get(&local_path(&grant.url)).await.text();
    assert!(!public_error.contains(final_key.as_str()));
    assert!(!public_error.contains("sha256"));
}

async fn download_app() -> TestApp {
    TestApp::builder()
        .workspace("workspace", "Download workspace")
        .with_default_membership()
        .build()
        .await
}

async fn create_evidence(app: &TestApp, workspace_id: Uuid) -> Uuid {
    let evidence = app
        .create_evidence(
            workspace_id,
            &json!({
                "title": "Download evidence",
                "description": "Evidence with a downloadable document.",
                "collection_instructions": "Upload the source artifact.",
                "status": "active"
            }),
        )
        .await;
    uuid_field(&evidence, "id")
}

async fn finalize_document(
    app: &TestApp,
    workspace_id: Uuid,
    submission_id: Uuid,
    document_id: Uuid,
) -> ObjectKey {
    let client = app.postgres().get().await.expect("connection opens");
    let row = client
        .query_one(
            "SELECT filename, object_key FROM documents WHERE id = $1",
            &[&document_id],
        )
        .await
        .expect("document reads");
    let filename: String = row.get("filename");
    let quarantine_key =
        ObjectKey::parse(row.get::<_, String>("object_key")).expect("quarantine object key parses");
    let final_key = ObjectKey::new(
        workspace_id.into(),
        format!("evidence-submissions/{submission_id}/documents/{document_id}"),
        filename,
    )
    .expect("final object key builds");
    let store = FilesystemObjectStore::new(app.object_storage_root())
        .await
        .expect("filesystem object store initializes");
    store
        .copy_object(&quarantine_key, &final_key)
        .await
        .expect("document finalizes in object storage");
    client
        .execute(
            "UPDATE documents SET object_key = $2, upload_status = 'uploaded' WHERE id = $1",
            &[&document_id, &final_key.as_str()],
        )
        .await
        .expect("document finalizes in Postgres");
    final_key
}

async fn set_document_status(app: &TestApp, document_id: Uuid, status: &str) {
    app.postgres()
        .get()
        .await
        .expect("connection opens")
        .execute(
            "UPDATE documents SET upload_status = $2 WHERE id = $1",
            &[&document_id, &status],
        )
        .await
        .expect("document status updates");
}

async fn archive_document(app: &TestApp, document_id: Uuid) {
    app.postgres()
        .get()
        .await
        .expect("connection opens")
        .execute(
            "UPDATE documents SET archived = true WHERE id = $1",
            &[&document_id],
        )
        .await
        .expect("document archives");
}

fn local_path(url: &Url) -> String {
    match url.query() {
        Some(query) => format!("{}?{query}", url.path()),
        None => url.path().to_owned(),
    }
}

fn token_from_url(url: &Url) -> String {
    url.query_pairs()
        .find_map(|(name, value)| (name == "token").then(|| value.into_owned()))
        .expect("download URL contains token")
}

fn tamper_last_character(value: &str) -> String {
    let mut tampered = value.as_bytes().to_vec();
    let last = tampered.len() - 1;
    tampered[last] = if tampered[last] == b'A' { b'B' } else { b'A' };
    String::from_utf8(tampered).expect("tampered path remains UTF-8")
}

fn assert_download(response: &axum_test::TestResponse, content: &[u8], filename: &str) {
    response.assert_status_ok();
    assert_eq!(response.as_bytes().as_ref(), content);
    assert_eq!(response.header("content-type"), "text/plain");
    assert_eq!(response.header("content-length"), content.len().to_string());
    assert_eq!(
        response.header("content-disposition"),
        format!("document; filename=\"{filename}\"")
    );
    assert_eq!(response.header("cache-control"), "private, no-store");
    assert_eq!(response.header("referrer-policy"), "no-referrer");
}

fn uuid_field(value: &Value, field: &str) -> Uuid {
    Uuid::parse_str(value[field].as_str().expect("UUID field is a string"))
        .expect("UUID field parses")
}
