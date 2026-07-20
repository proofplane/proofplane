use axum::http::StatusCode;
use axum_test::multipart::{MultipartForm, Part};
use proofplane::{
    domain::{CoverageWindow, EvidenceId},
    services::document_upload_grants::UploadGrantError,
};
use serde_json::{json, Value};
use url::Url;
use uuid::Uuid;

use super::support::TestApp;

#[tokio::test]
async fn upload_grant_redeems_once_and_session_creates_one_submission_per_file() {
    let app = upload_app().await;
    let workspace_id = app.workspace_id("workspace");
    let evidence_id = create_evidence(&app, workspace_id, "Quarterly access review").await;
    let coverage = coverage();
    let grant = app
        .document_upload_grant_service()
        .issue(
            &app.agent_connection_context(workspace_id),
            EvidenceId::from(evidence_id),
            coverage,
        )
        .await
        .expect("upload grant issues");
    assert_eq!(grant.evidence_id, evidence_id.into());
    assert_eq!(grant.coverage, coverage);

    let grant_path = local_path(&grant.url);
    let redeemed = app.server().get(&grant_path).await;
    redeemed.assert_status(StatusCode::SEE_OTHER);
    assert_eq!(redeemed.header("location"), "/evidence-document-uploads");
    let set_cookie_header = redeemed.header("set-cookie");
    let set_cookie = set_cookie_header.to_str().expect("cookie header");
    assert!(set_cookie.contains("HttpOnly"));
    assert!(set_cookie.contains("SameSite=Lax"));
    assert!(set_cookie.contains("Path=/evidence-document-uploads"));
    assert!(set_cookie.contains("Secure"));
    let cookie = request_cookie(set_cookie);

    app.server()
        .get(&grant_path)
        .await
        .assert_status_not_found();
    app.server()
        .get("/evidence-document-uploads")
        .add_header("cookie", cookie.clone())
        .await
        .assert_status_ok();

    for bytes in [b"first report".as_slice(), b"second report".as_slice()] {
        app.server()
            .post("/evidence-document-uploads/files")
            .add_header("cookie", cookie.clone())
            .multipart(upload_form(bytes, "report.txt"))
            .await
            .assert_status(StatusCode::SEE_OTHER);
    }

    let rows = submission_documents(&app, evidence_id).await;
    assert_eq!(rows.len(), 2);
    assert_ne!(rows[0].submission_id, rows[1].submission_id);
    for row in &rows {
        assert_eq!(row.filename, "report.txt");
        assert_eq!(row.valid_from, coverage.valid_from);
        assert_eq!(row.valid_until, coverage.valid_until);
        assert_eq!(document_count(&app, row.submission_id).await, 1);
    }
    assert!(grant_redeemed(&app, evidence_id).await);

    let page = app
        .server()
        .get("/evidence-document-uploads")
        .add_header("cookie", cookie)
        .await;
    page.assert_status_ok();
    let body = page.text();
    assert_eq!(body.matches("report.txt").count(), 2);
    assert!(!body.contains("object_key"));
}

#[tokio::test]
async fn concurrent_browser_uploads_create_distinct_single_document_submissions() {
    let app = upload_app().await;
    let workspace_id = app.workspace_id("workspace");
    let evidence_id = create_evidence(&app, workspace_id, "Concurrent evidence").await;
    let cookie = upload_session_cookie(&app, workspace_id, evidence_id).await;

    let first = app
        .server()
        .post("/evidence-document-uploads/files")
        .add_header("cookie", cookie.clone())
        .multipart(upload_form(b"one", "one.txt"));
    let second = app
        .server()
        .post("/evidence-document-uploads/files")
        .add_header("cookie", cookie)
        .multipart(upload_form(b"two", "two.txt"));
    let (first, second) = tokio::join!(first, second);
    first.assert_status(StatusCode::SEE_OTHER);
    second.assert_status(StatusCode::SEE_OTHER);

    let rows = submission_documents(&app, evidence_id).await;
    assert_eq!(rows.len(), 2);
    assert_eq!(
        rows.iter()
            .map(|row| row.filename.as_str())
            .collect::<Vec<_>>(),
        ["one.txt", "two.txt"]
    );
    assert_ne!(rows[0].submission_id, rows[1].submission_id);
    for row in rows {
        assert_eq!(document_count(&app, row.submission_id).await, 1);
    }
}

#[tokio::test]
async fn upload_session_rejects_invalid_forms_and_conceals_unavailable_sessions() {
    let app = upload_app().await;
    let workspace_id = app.workspace_id("workspace");
    let evidence_id = create_evidence(&app, workspace_id, "Form validation").await;

    app.server()
        .get("/evidence-document-uploads")
        .await
        .assert_status_not_found();
    app.server()
        .get("/evidence-document-uploads")
        .add_header("cookie", "proofplane_document_upload_session=not-a-token")
        .await
        .assert_status_not_found();
    app.server()
        .post("/evidence-document-uploads/files")
        .add_header("cookie", "proofplane_document_upload_session=not-a-token")
        .multipart(upload_form(b"bytes", "artifact.txt"))
        .await
        .assert_status_not_found();

    for form in [
        MultipartForm::new().add_part("note", Part::text("not a file")),
        upload_form(b"bytes", "path/file.txt"),
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
    ] {
        let cookie = upload_session_cookie(&app, workspace_id, evidence_id).await;
        app.server()
            .post("/evidence-document-uploads/files")
            .add_header("cookie", cookie)
            .multipart(form)
            .await
            .assert_status_bad_request();
    }
    assert!(submission_documents(&app, evidence_id).await.is_empty());
}

#[tokio::test]
async fn upload_session_archival_is_owner_scoped_and_requires_terminal_status() {
    let app = upload_app().await;
    let workspace_id = app.workspace_id("workspace");
    let evidence_id = create_evidence(&app, workspace_id, "Archive evidence").await;
    let other_evidence_id = create_evidence(&app, workspace_id, "Other evidence").await;
    let cookie = upload_session_cookie(&app, workspace_id, evidence_id).await;
    app.server()
        .post("/evidence-document-uploads/files")
        .add_header("cookie", cookie.clone())
        .multipart(upload_form(b"pending", "pending.txt"))
        .await
        .assert_status(StatusCode::SEE_OTHER);
    let own = submission_documents(&app, evidence_id).await.remove(0);

    let pending_path = archive_path(own.submission_id, own.document_id);
    app.server()
        .post(&pending_path)
        .add_header("cookie", cookie.clone())
        .await
        .assert_status(StatusCode::CONFLICT);
    assert!(!document_archived(&app, own.document_id).await);

    let other_cookie = upload_session_cookie(&app, workspace_id, other_evidence_id).await;
    app.server()
        .post("/evidence-document-uploads/files")
        .add_header("cookie", other_cookie)
        .multipart(upload_form(b"other", "other.txt"))
        .await
        .assert_status(StatusCode::SEE_OTHER);
    let other = submission_documents(&app, other_evidence_id)
        .await
        .remove(0);
    app.server()
        .get(&download_path(other.submission_id, other.document_id))
        .add_header("cookie", cookie.clone())
        .await
        .assert_status_not_found();
    app.server()
        .post(&archive_path(other.submission_id, other.document_id))
        .add_header("cookie", cookie.clone())
        .await
        .assert_status_not_found();

    set_document_status(&app, own.document_id, "failed").await;
    app.server()
        .post(&pending_path)
        .add_header("cookie", cookie.clone())
        .await
        .assert_status(StatusCode::SEE_OTHER);
    assert!(document_archived(&app, own.document_id).await);
    let page = app
        .server()
        .get("/evidence-document-uploads")
        .add_header("cookie", cookie)
        .await;
    page.assert_status_ok();
    assert!(!page.text().contains("pending.txt"));
}

#[tokio::test]
async fn upload_grant_issuance_conceals_missing_and_cross_workspace_evidence() {
    let app = TestApp::builder()
        .workspace("workspace", "Upload workspace")
        .with_default_membership()
        .workspace("other", "Other upload workspace")
        .with_default_membership()
        .build()
        .await;
    let workspace_id = app.workspace_id("workspace");
    let other_workspace_id = app.workspace_id("other");
    let other_evidence_id = create_evidence(&app, other_workspace_id, "Other evidence").await;
    let service = app.document_upload_grant_service();

    for evidence_id in [Uuid::new_v4(), other_evidence_id] {
        assert!(matches!(
            service
                .issue(
                    &app.agent_connection_context(workspace_id),
                    evidence_id.into(),
                    coverage(),
                )
                .await,
            Err(UploadGrantError::Unavailable)
        ));
    }
}

async fn upload_app() -> TestApp {
    TestApp::builder()
        .workspace("workspace", "Upload workspace")
        .with_default_membership()
        .build()
        .await
}

async fn create_evidence(app: &TestApp, workspace_id: Uuid, title: &str) -> Uuid {
    let evidence = app
        .create_evidence(
            workspace_id,
            &json!({
                "title": title,
                "description": format!("Collect {title}."),
                "collection_instructions": format!("Upload {title}."),
                "status": "active"
            }),
        )
        .await;
    uuid_field(&evidence, "id")
}

async fn upload_session_cookie(app: &TestApp, workspace_id: Uuid, evidence_id: Uuid) -> String {
    let issued = app
        .document_upload_grant_service()
        .issue(
            &app.agent_connection_context(workspace_id),
            evidence_id.into(),
            coverage(),
        )
        .await
        .expect("upload grant issues");
    let response = app.server().get(&local_path(&issued.url)).await;
    response.assert_status(StatusCode::SEE_OTHER);
    request_cookie(
        response
            .header("set-cookie")
            .to_str()
            .expect("cookie header"),
    )
}

fn coverage() -> CoverageWindow {
    CoverageWindow::new(
        "2026-01-01T00:00:00Z".parse().expect("valid_from parses"),
        "2026-03-31T23:59:59Z".parse().expect("valid_until parses"),
    )
    .expect("coverage window is valid")
}

fn local_path(url: &Url) -> String {
    match url.query() {
        Some(query) => format!("{}?{query}", url.path()),
        None => url.path().to_owned(),
    }
}

fn request_cookie(set_cookie: &str) -> String {
    set_cookie
        .split(';')
        .next()
        .expect("set-cookie contains name and value")
        .to_owned()
}

fn upload_form(bytes: &[u8], filename: &str) -> MultipartForm {
    MultipartForm::new().add_part(
        "file",
        Part::bytes(bytes.to_vec())
            .file_name(filename)
            .mime_type("text/plain"),
    )
}

fn archive_path(submission_id: Uuid, document_id: Uuid) -> String {
    format!("/evidence-document-uploads/files/{submission_id}/{document_id}/archive")
}

fn download_path(submission_id: Uuid, document_id: Uuid) -> String {
    format!("/evidence-document-uploads/files/{submission_id}/{document_id}/download")
}

#[derive(Debug)]
struct SubmissionDocument {
    submission_id: Uuid,
    document_id: Uuid,
    filename: String,
    valid_from: chrono::DateTime<chrono::Utc>,
    valid_until: chrono::DateTime<chrono::Utc>,
}

async fn submission_documents(app: &TestApp, evidence_id: Uuid) -> Vec<SubmissionDocument> {
    app.postgres()
        .get()
        .await
        .expect("connection opens")
        .query(
            r#"
SELECT
    s.id AS submission_id,
    s.valid_from,
    s.valid_until,
    d.id AS document_id,
    d.filename
FROM evidence_submissions s
JOIN documents d
  ON d.owner_type = 'evidence_submission'
 AND d.owner_id = s.id
WHERE s.evidence_id = $1
ORDER BY d.filename, s.id
"#,
            &[&evidence_id],
        )
        .await
        .expect("submission documents load")
        .into_iter()
        .map(|row| SubmissionDocument {
            submission_id: row.get("submission_id"),
            document_id: row.get("document_id"),
            filename: row.get("filename"),
            valid_from: row.get("valid_from"),
            valid_until: row.get("valid_until"),
        })
        .collect()
}

async fn document_count(app: &TestApp, submission_id: Uuid) -> i64 {
    app.postgres()
        .get()
        .await
        .expect("connection opens")
        .query_one(
            "SELECT COUNT(*) FROM documents WHERE owner_type = 'evidence_submission' AND owner_id = $1",
            &[&submission_id],
        )
        .await
        .expect("document count loads")
        .get(0)
}

async fn grant_redeemed(app: &TestApp, evidence_id: Uuid) -> bool {
    app.postgres()
        .get()
        .await
        .expect("connection opens")
        .query_one(
            "SELECT redeemed_at IS NOT NULL FROM document_upload_grants WHERE evidence_id = $1",
            &[&evidence_id],
        )
        .await
        .expect("upload grant loads")
        .get(0)
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

async fn document_archived(app: &TestApp, document_id: Uuid) -> bool {
    app.postgres()
        .get()
        .await
        .expect("connection opens")
        .query_one(
            "SELECT archived FROM documents WHERE id = $1",
            &[&document_id],
        )
        .await
        .expect("document archived flag loads")
        .get("archived")
}

fn uuid_field(value: &Value, field: &str) -> Uuid {
    Uuid::parse_str(value[field].as_str().expect("UUID field is a string"))
        .expect("UUID field parses")
}
