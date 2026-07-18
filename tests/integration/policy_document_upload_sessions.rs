use axum::http::StatusCode;
use axum_test::multipart::{MultipartForm, Part};
use chrono::{Duration, Utc};
use proofplane::{
    authentication::paseto::{PolicyUploadSessionEncryptor, RegisteredClaims},
    config::{PasetoUploadGrantConfig, PasetoUploadGrantKey},
    domain::CreatePolicyPayload,
    object_storage::{FilesystemObjectStore, ObjectKey, ObjectStore},
    routes::request_context::REQUEST_ID_HEADER,
    services::{policies::PolicyService, policy_upload_sessions::POLICY_UPLOAD_SESSION_AUDIENCE},
};
use secrecy::SecretString;
use serde::Serialize;
use uuid::Uuid;

use super::support::{capture_audit_logs, TestApp};

#[tokio::test]
async fn policy_session_page_uploads_one_file_and_suppresses_replacement_upload() {
    let app = policy_app().await;
    let workspace_id = app.workspace_id("workspace");
    let policy_id = create_policy(&app, workspace_id, "Security <policy>").await;
    let cookie = policy_session_cookie(&app, workspace_id, policy_id, 300);

    let empty = app
        .server()
        .get("/policy-document-uploads")
        .add_header("cookie", cookie.clone())
        .await;
    empty.assert_status_ok();
    let body = empty.text();
    assert!(body.contains("Security &lt;policy&gt;"));
    assert!(body.contains("name=\"file\" type=\"file\" required"));
    assert!(body.contains("prefers-reduced-motion"));

    let uploaded = app
        .server()
        .post("/policy-document-uploads/files")
        .add_header("cookie", cookie.clone())
        .multipart(browser_upload_form(
            b"policy bytes",
            "security-final.txt",
            "text/plain",
        ))
        .await;
    uploaded.assert_status(StatusCode::SEE_OTHER);
    assert_eq!(uploaded.header("location"), "/policy-document-uploads");

    let row = app
        .postgres()
        .get()
        .await
        .expect("policy document database opens")
        .query_one(
            r#"
SELECT d.id, d.filename, d.upload_status, d.checksum_crc32c, count(o.id) AS outbox_count
FROM documents d
LEFT JOIN outbox_messages o ON o.aggregate_id = d.id::text
WHERE d.owner_type = 'policy' AND d.owner_id = $1 AND d.archived = false
GROUP BY d.id
"#,
            &[&policy_id],
        )
        .await
        .expect("policy document reads");
    assert_eq!(row.get::<_, String>("filename"), "security-final.txt");
    assert_eq!(row.get::<_, String>("upload_status"), "pending");
    assert!(!row.get::<_, String>("checksum_crc32c").is_empty());
    assert_eq!(row.get::<_, i64>("outbox_count"), 1);

    let current = app
        .server()
        .get("/policy-document-uploads")
        .add_header("cookie", cookie.clone())
        .await;
    current.assert_status_ok();
    let body = current.text();
    assert!(body.contains("security-final.txt"));
    assert!(body.contains("Uploading"));
    assert!(!body.contains("pending"));
    assert!(!body.contains("name=\"file\""));
    assert!(!body.contains("Download policy document"));
    assert!(!body.contains("Archive policy document"));

    app.server()
        .post("/policy-document-uploads/files")
        .add_header("cookie", cookie.clone())
        .multipart(browser_upload_form(b"second", "second.txt", "text/plain"))
        .await
        .assert_status(StatusCode::CONFLICT);

    let document_id = row.get::<_, Uuid>("id");
    app.server()
        .post(&format!(
            "/policy-document-uploads/files/{document_id}/archive"
        ))
        .add_header("cookie", cookie)
        .await
        .assert_status(StatusCode::CONFLICT);
}

#[tokio::test]
async fn terminal_policy_document_downloads_archives_and_allows_reupload() {
    let app = policy_app().await;
    let workspace_id = app.workspace_id("workspace");
    let policy_id = create_policy(&app, workspace_id, "Download policy").await;
    let cookie = policy_session_cookie(&app, workspace_id, policy_id, 300);
    let document_id = upload_through_browser(
        &app,
        &cookie,
        b"downloadable policy bytes",
        "download-policy.txt",
    )
    .await;
    finalize_policy_document(&app, workspace_id, policy_id, document_id).await;

    let page = app
        .server()
        .get("/policy-document-uploads")
        .add_header("cookie", cookie.clone())
        .await;
    page.assert_status_ok();
    assert!(page.text().contains("Download policy document"));
    assert!(page.text().contains("Archive policy document"));

    let request = app
        .server()
        .get(&format!(
            "/policy-document-uploads/files/{document_id}/download"
        ))
        .add_header("cookie", cookie.clone());
    let (download, audits) = capture_audit_logs(|request_id| async move {
        request
            .add_header(REQUEST_ID_HEADER, request_id.to_string())
            .await
    })
    .await;
    download.assert_status_ok();
    assert_eq!(download.as_bytes().as_ref(), b"downloadable policy bytes");
    assert_eq!(download.header("content-type"), "text/plain");
    assert_eq!(
        download.header("content-disposition"),
        "document; filename=\"download-policy.txt\""
    );
    assert_eq!(download.header("cache-control"), "private, no-store");
    assert_eq!(download.header("referrer-policy"), "no-referrer");
    let audits = serde_json::to_string(&audits).expect("audit records serialize");
    assert!(audits.contains("policy_document.downloaded"));
    assert!(audits.contains(&policy_id.to_string()));
    assert!(audits.contains(&document_id.to_string()));
    for sensitive in [
        "download-policy.txt",
        "downloadable policy bytes",
        "object_key",
        "checksum",
    ] {
        assert!(!audits.contains(sensitive));
    }

    let archived = app
        .server()
        .post(&format!(
            "/policy-document-uploads/files/{document_id}/archive"
        ))
        .add_header("cookie", cookie.clone())
        .await;
    archived.assert_status(StatusCode::SEE_OTHER);
    assert_eq!(archived.header("location"), "/policy-document-uploads");

    app.server()
        .get(&format!(
            "/policy-document-uploads/files/{document_id}/download"
        ))
        .add_header("cookie", cookie.clone())
        .await
        .assert_status(StatusCode::NOT_FOUND);
    let empty = app
        .server()
        .get("/policy-document-uploads")
        .add_header("cookie", cookie.clone())
        .await;
    assert!(empty.text().contains("Upload document"));

    app.server()
        .post("/policy-document-uploads/files")
        .add_header("cookie", cookie)
        .multipart(browser_upload_form(
            b"replacement",
            "replacement.txt",
            "text/plain",
        ))
        .await
        .assert_status(StatusCode::SEE_OTHER);
    assert_eq!(active_document_count(&app, policy_id).await, 1);
}

#[tokio::test]
async fn policy_session_rejects_invalid_uploads_and_conceals_unavailable_resources() {
    let app = policy_app().await;
    let workspace_id = app.workspace_id("workspace");
    let other_workspace_id = app.workspace_id("other");
    let policy_id = create_policy(&app, workspace_id, "Validation policy").await;
    let other_policy_id = create_policy(&app, other_workspace_id, "Other policy").await;
    let cookie = policy_session_cookie(&app, workspace_id, policy_id, 300);

    for form in [
        MultipartForm::new().add_part("note", Part::text("not a file")),
        MultipartForm::new()
            .add_part(
                "file",
                Part::bytes(b"one".to_vec())
                    .file_name("one.txt")
                    .mime_type("text/plain"),
            )
            .add_part(
                "file",
                Part::bytes(b"two".to_vec())
                    .file_name("two.txt")
                    .mime_type("text/plain"),
            ),
        browser_upload_form(b"invalid", "path/file.txt", "text/plain"),
    ] {
        app.server()
            .post("/policy-document-uploads/files")
            .add_header("cookie", cookie.clone())
            .multipart(form)
            .await
            .assert_status(StatusCode::BAD_REQUEST);
    }
    assert_eq!(active_document_count(&app, policy_id).await, 0);

    let expired_cookie = policy_session_cookie(&app, workspace_id, policy_id, 1);
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    for unavailable_cookie in [
        None,
        Some("proofplane_policy_document_upload_session=not-a-token".to_owned()),
        Some(expired_cookie),
        Some(policy_session_cookie(
            &app,
            workspace_id,
            other_policy_id,
            300,
        )),
    ] {
        let mut request = app.server().get("/policy-document-uploads");
        if let Some(cookie) = unavailable_cookie {
            request = request.add_header("cookie", cookie);
        }
        let response = request.await;
        response.assert_status(StatusCode::NOT_FOUND);
        assert!(response
            .text()
            .contains("This policy document link is no longer available"));
        assert!(!response.text().contains("Validation policy"));
        assert!(!response.text().contains("Other policy"));
    }

    PolicyService::new(app.postgres_arc())
        .archive(app.agent_connection_context(workspace_id), policy_id.into())
        .await
        .expect("policy archives");
    app.server()
        .get("/policy-document-uploads")
        .add_header("cookie", cookie)
        .await
        .assert_status(StatusCode::NOT_FOUND);
}

async fn policy_app() -> TestApp {
    TestApp::builder()
        .workspace("workspace", "Policy upload workspace")
        .with_default_membership()
        .workspace("other", "Other policy workspace")
        .with_default_membership()
        .build()
        .await
}

async fn create_policy(app: &TestApp, workspace_id: Uuid, name: &str) -> Uuid {
    Uuid::from(
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
            .id,
    )
}

async fn upload_through_browser(
    app: &TestApp,
    cookie: &str,
    content: &[u8],
    filename: &str,
) -> Uuid {
    app.server()
        .post("/policy-document-uploads/files")
        .add_header("cookie", cookie)
        .multipart(browser_upload_form(content, filename, "text/plain"))
        .await
        .assert_status(StatusCode::SEE_OTHER);
    app.postgres()
        .get()
        .await
        .expect("policy document database opens")
        .query_one(
            "SELECT id FROM documents WHERE owner_type = 'policy' AND archived = false ORDER BY created_at DESC LIMIT 1",
            &[],
        )
        .await
        .expect("uploaded policy document reads")
        .get("id")
}

async fn finalize_policy_document(
    app: &TestApp,
    workspace_id: Uuid,
    policy_id: Uuid,
    document_id: Uuid,
) {
    let client = app
        .postgres()
        .get()
        .await
        .expect("policy document database opens");
    let object_key = client
        .query_one(
            "SELECT object_key FROM documents WHERE id = $1",
            &[&document_id],
        )
        .await
        .expect("quarantine key reads")
        .get::<_, String>("object_key");
    let source = ObjectKey::parse(object_key).expect("quarantine key parses");
    let target = ObjectKey::new(
        workspace_id.into(),
        format!("policies/{policy_id}/documents/{document_id}"),
        "download-policy.txt",
    )
    .expect("final key builds");
    let store = FilesystemObjectStore::new(app.object_storage_root())
        .await
        .expect("policy document object store opens");
    store
        .copy_object(&source, &target)
        .await
        .expect("policy document finalizes");
    store
        .delete_object(&source)
        .await
        .expect("quarantine object deletes");
    let final_key = target.to_string();
    client
        .execute(
            "UPDATE documents SET object_key = $2, upload_status = 'uploaded' WHERE id = $1",
            &[&document_id, &final_key],
        )
        .await
        .expect("policy document status updates");
}

async fn active_document_count(app: &TestApp, policy_id: Uuid) -> i64 {
    app.postgres()
        .get()
        .await
        .expect("policy document database opens")
        .query_one(
            "SELECT count(*) FROM documents WHERE owner_type = 'policy' AND owner_id = $1 AND archived = false",
            &[&policy_id],
        )
        .await
        .expect("active policy documents count")
        .get("count")
}

fn browser_upload_form(bytes: &[u8], filename: &str, content_type: &str) -> MultipartForm {
    MultipartForm::new().add_part(
        "file",
        Part::bytes(bytes.to_vec())
            .file_name(filename)
            .mime_type(content_type),
    )
}

fn policy_session_cookie(
    app: &TestApp,
    workspace_id: Uuid,
    policy_id: Uuid,
    ttl_seconds: i64,
) -> String {
    let token = PolicyUploadSessionEncryptor::from_config(
        url::Url::parse("https://api.proofplane.test/").expect("test base URL parses"),
        POLICY_UPLOAD_SESSION_AUDIENCE,
        &upload_grant_config(),
    )
    .expect("policy session encryptor initializes")
    .encrypt(
        RegisteredClaims {
            subject: app.user_id(),
            token_id: Uuid::new_v4(),
            expires_at: Utc::now() + Duration::seconds(ttl_seconds),
        },
        &TestPolicyUploadSessionClaims {
            version: 1,
            workspace_id: workspace_id.to_string(),
            policy_id: policy_id.to_string(),
            issued_by_user_id: app.user_id().to_string(),
            issued_via_agent_connection_id: app.api_token_id().to_string(),
        },
    )
    .expect("policy session token issues")
    .token;
    format!("proofplane_policy_document_upload_session={token}")
}

#[derive(Serialize)]
struct TestPolicyUploadSessionClaims {
    version: u8,
    workspace_id: String,
    policy_id: String,
    issued_by_user_id: String,
    issued_via_agent_connection_id: String,
}

fn upload_grant_config() -> PasetoUploadGrantConfig {
    PasetoUploadGrantConfig {
        active_key_id: "integration-upload-grant-001".to_owned(),
        keys: vec![PasetoUploadGrantKey {
            id: "integration-upload-grant-001".to_owned(),
            secret: SecretString::from("k4.local.cMO6bYZvmIk4f5OppaRjsRYQE0frbAM7qD4cDAO8HxY"),
        }],
    }
}
