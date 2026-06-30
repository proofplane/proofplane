use axum_test::multipart::{MultipartForm, Part};
use chrono::{Duration, SecondsFormat, Utc};
use proofplane::{
    authentication::{
        paseto::{
            RegisteredClaims, UploadGrantDecryptor, UploadGrantEncryptor, UploadSessionEncryptor,
        },
        ApiTokenContext,
    },
    config::{PasetoUploadGrantConfig, PasetoUploadGrantKey},
    domain::{EvidenceSubmissionId, WorkspaceId, WorkspacePermission, WorkspacePermissions},
    object_storage::{FilesystemObjectStore, ObjectKey, ObjectStore},
    services::{
        attachment_upload_grants::{AttachmentUploadGrantService, UploadGrantError},
        upload_sessions::UPLOAD_SESSION_AUDIENCE,
    },
};
use secrecy::SecretString;
use serde::Serialize;
use serde_json::{json, Value};
use url::Url;
use uuid::Uuid;

use super::support::{upload_attachment, TestApp};

#[tokio::test]
async fn upload_grant_issue_persists_workspace_submission_issuer_and_expiry() {
    let app = upload_grant_app().await;
    let workspace_id = app.workspace_id("workspace");
    let submission_id = create_submission(&app, workspace_id).await;
    let service = upload_grant_service(&app);

    let issued = service
        .issue(&api_token_context(&app, workspace_id), submission_id.into())
        .await
        .expect("upload grant issues");

    assert_eq!(
        issued.submission_id,
        EvidenceSubmissionId::from(submission_id)
    );
    assert_eq!(issued.audit.workspace_id, WorkspaceId::from(workspace_id));
    assert_eq!(
        issued.audit.issued_by_user_id.to_string(),
        app.user_id().to_string()
    );
    assert!(issued
        .url
        .as_str()
        .starts_with("https://api.proofplane.test/evidence-attachment-uploads?token=v4.local."));

    let rows = app
        .postgres()
        .get()
        .await
        .expect("connection opens")
        .query(
            r#"
SELECT workspace_id, evidence_submission_id, issued_by_user_id, issued_via_api_token_id,
       issued_at, expires_at, redeemed_at
FROM attachment_upload_grants
"#,
            &[],
        )
        .await
        .expect("grant row reads");
    assert_eq!(rows.len(), 1);
    let row = &rows[0];
    assert_eq!(row.get::<_, Uuid>("workspace_id"), workspace_id);
    assert_eq!(row.get::<_, Uuid>("evidence_submission_id"), submission_id);
    assert_eq!(row.get::<_, Uuid>("issued_by_user_id"), app.user_id());
    assert_eq!(
        row.get::<_, Uuid>("issued_via_api_token_id"),
        app.api_token_id()
    );
    assert!(
        row.get::<_, chrono::DateTime<Utc>>("expires_at")
            > row.get::<_, chrono::DateTime<Utc>>("issued_at")
    );
    assert!(row
        .get::<_, Option<chrono::DateTime<Utc>>>("redeemed_at")
        .is_none());
}

#[tokio::test]
async fn upload_grant_issue_for_missing_or_cross_workspace_submission_is_unavailable() {
    let app = upload_grant_app().await;
    let workspace_id = app.workspace_id("workspace");
    let other_workspace_id = app.workspace_id("other");
    let submission_id = create_submission(&app, workspace_id).await;
    let service = upload_grant_service(&app);

    let missing = service
        .issue(
            &api_token_context(&app, workspace_id),
            EvidenceSubmissionId::from(Uuid::new_v4()),
        )
        .await;
    assert!(matches!(missing, Err(UploadGrantError::Unavailable)));

    let cross_workspace = service
        .issue(
            &api_token_context(&app, other_workspace_id),
            EvidenceSubmissionId::from(submission_id),
        )
        .await;
    assert!(matches!(
        cross_workspace,
        Err(UploadGrantError::Unavailable)
    ));

    let count = app
        .postgres()
        .get()
        .await
        .expect("connection opens")
        .query_one("SELECT count(*) FROM attachment_upload_grants", &[])
        .await
        .expect("grant count reads")
        .get::<_, i64>(0);
    assert_eq!(count, 0);
}

#[tokio::test]
async fn upload_grant_redeems_once_and_marks_redeemed_at() {
    let app = upload_grant_app().await;
    let workspace_id = app.workspace_id("workspace");
    let submission_id = create_submission(&app, workspace_id).await;
    let service = upload_grant_service(&app);
    let issued = service
        .issue(&api_token_context(&app, workspace_id), submission_id.into())
        .await
        .expect("upload grant issues");
    let token = token_from_url(&issued.url);

    let redeemed = service
        .redeem(&token)
        .await
        .expect("upload grant redeems once");

    assert_eq!(redeemed.workspace_id, WorkspaceId::from(workspace_id));
    assert_eq!(
        redeemed.submission_id,
        EvidenceSubmissionId::from(submission_id)
    );
    assert_eq!(
        redeemed.issued_by_user_id.to_string(),
        app.user_id().to_string()
    );
    assert_eq!(
        redeemed.issued_via_api_token_id.to_string(),
        app.api_token_id().to_string()
    );

    let second = service.redeem(&token).await;
    assert!(matches!(second, Err(UploadGrantError::Unavailable)));

    let redeemed_at = app
        .postgres()
        .get()
        .await
        .expect("connection opens")
        .query_one("SELECT redeemed_at FROM attachment_upload_grants", &[])
        .await
        .expect("grant row reads")
        .get::<_, Option<chrono::DateTime<Utc>>>("redeemed_at");
    assert!(redeemed_at.is_some());
}

#[tokio::test]
async fn upload_grant_redeem_unavailable_cases_are_generic() {
    let app = upload_grant_app().await;
    let workspace_id = app.workspace_id("workspace");
    let service = upload_grant_service(&app);

    assert_unavailable(service.redeem("not-a-token").await);

    let expired_token = issue_grant_token(&app, &service, workspace_id).await;
    app.postgres()
        .get()
        .await
        .expect("connection opens")
        .execute(
            "UPDATE attachment_upload_grants SET issued_at = now() - interval '10 minutes', expires_at = now() - interval '5 minutes'",
            &[],
        )
        .await
        .expect("grant expiry updates");
    assert_unavailable(service.redeem(&expired_token).await);

    let missing_row_token = issue_grant_token(&app, &service, workspace_id).await;
    app.postgres()
        .get()
        .await
        .expect("connection opens")
        .execute("DELETE FROM attachment_upload_grants", &[])
        .await
        .expect("grant rows delete");
    assert_unavailable(service.redeem(&missing_row_token).await);
}

#[tokio::test]
async fn existing_download_grants_remain_reusable() {
    let app = upload_grant_app().await;
    let workspace_id = app.workspace_id("workspace");
    let submission_id = create_submission(&app, workspace_id).await;
    let attachment = super::support::upload_attachment(
        &app,
        workspace_id,
        submission_id,
        "download-still-reusable.txt",
        b"download",
    )
    .await;
    let attachment_id = Uuid::parse_str(attachment["id"].as_str().expect("attachment id"))
        .expect("attachment id parses");
    finalize_attachment(&app, workspace_id, submission_id, attachment_id).await;

    let grant = app
        .post(&format!(
            "/workspaces/{workspace_id}/evidence-submissions/{submission_id}/attachments/{attachment_id}/download-grants"
        ))
        .await;
    grant.assert_status_ok();
    let url = grant.json::<Value>()["url"]
        .as_str()
        .expect("download URL")
        .to_owned();
    let path = Url::parse(&url)
        .expect("download URL parses")
        .path()
        .to_owned()
        + "?"
        + Url::parse(&url)
            .expect("download URL parses")
            .query()
            .expect("download token query");

    app.get(&path).await.assert_status_ok();
    app.get(&path).await.assert_status_ok();
}

#[tokio::test]
async fn upload_session_redeems_grant_sets_cookie_and_marks_redeemed() {
    let app = upload_grant_app().await;
    let workspace_id = app.workspace_id("workspace");
    let submission_id = create_submission(&app, workspace_id).await;
    let service = upload_grant_service(&app);
    let issued = service
        .issue(&api_token_context(&app, workspace_id), submission_id.into())
        .await
        .expect("upload grant issues");

    let response = app.server().get(&upload_path(&issued.url)).await;
    response.assert_status(axum::http::StatusCode::SEE_OTHER);
    assert_eq!(
        response.header("location").to_str().expect("location"),
        "/evidence-attachment-uploads"
    );
    let set_cookie = response.header("set-cookie");
    let cookie = set_cookie.to_str().expect("set-cookie");

    assert!(cookie.starts_with("proofplane_attachment_upload_session=v4.local."));
    assert!(cookie.contains("; HttpOnly"));
    assert!(cookie.contains("; SameSite=Lax"));
    assert!(cookie.contains("; Path=/evidence-attachment-uploads"));
    let max_age = cookie_max_age(cookie);
    assert!((1..=300).contains(&max_age));
    assert!(cookie.contains("; Secure"));
    let body = app
        .server()
        .get("/evidence-attachment-uploads")
        .add_header("cookie", cookie)
        .await
        .text();
    assert!(body.contains("Evidence attachment management"));
    assert!(body.contains("Current attachments"));

    let redeemed_at = app
        .postgres()
        .get()
        .await
        .expect("connection opens")
        .query_one("SELECT redeemed_at FROM attachment_upload_grants", &[])
        .await
        .expect("grant row reads")
        .get::<_, Option<chrono::DateTime<Utc>>>("redeemed_at");
    assert!(redeemed_at.is_some());
}

#[tokio::test]
async fn upload_session_grant_url_cannot_be_opened_twice() {
    let app = upload_grant_app().await;
    let workspace_id = app.workspace_id("workspace");
    let submission_id = create_submission(&app, workspace_id).await;
    let issued = upload_grant_service(&app)
        .issue(&api_token_context(&app, workspace_id), submission_id.into())
        .await
        .expect("upload grant issues");
    let path = upload_path(&issued.url);

    app.server()
        .get(&path)
        .await
        .assert_status(axum::http::StatusCode::SEE_OTHER);
    let second = app.get(&path).await;

    second.assert_status(axum::http::StatusCode::NOT_FOUND);
    assert_eq!(second.maybe_header("set-cookie"), None);
}

#[tokio::test]
async fn upload_session_cookie_loads_only_scoped_submission_inventory() {
    let app = upload_grant_app().await;
    let workspace_id = app.workspace_id("workspace");
    let submission_id = create_submission(&app, workspace_id).await;
    let other_submission_id = create_submission(&app, workspace_id).await;
    upload_attachment(&app, workspace_id, submission_id, "scoped.txt", b"scoped").await;
    upload_attachment(
        &app,
        workspace_id,
        other_submission_id,
        "other.txt",
        b"other",
    )
    .await;
    let issued = upload_grant_service(&app)
        .issue(&api_token_context(&app, workspace_id), submission_id.into())
        .await
        .expect("upload grant issues");
    let redeemed = app.server().get(&upload_path(&issued.url)).await;
    redeemed.assert_status(axum::http::StatusCode::SEE_OTHER);
    let cookie = redeemed.header("set-cookie");

    let opened = app
        .get("/evidence-attachment-uploads")
        .add_header("cookie", cookie)
        .await;
    opened.assert_status_ok();
    let body = opened.text();

    assert!(body.contains("scoped.txt"));
    assert!(!body.contains("other.txt"));
}

#[tokio::test]
async fn upload_session_cookie_expires_at_original_grant_expiry() {
    let app = upload_grant_app().await;
    let workspace_id = app.workspace_id("workspace");
    let submission_id = create_submission(&app, workspace_id).await;
    let issued = upload_grant_service(&app)
        .issue(&api_token_context(&app, workspace_id), submission_id.into())
        .await
        .expect("upload grant issues");
    app.postgres()
        .get()
        .await
        .expect("connection opens")
        .execute(
            "UPDATE attachment_upload_grants SET expires_at = now() + interval '1 second'",
            &[],
        )
        .await
        .expect("grant expiry updates");

    let redeemed = app.server().get(&upload_path(&issued.url)).await;
    redeemed.assert_status(axum::http::StatusCode::SEE_OTHER);
    let set_cookie = redeemed.header("set-cookie");
    let cookie = set_cookie.to_str().expect("set-cookie");
    assert_eq!(cookie_max_age(cookie), 1);

    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    app.server()
        .get("/evidence-attachment-uploads")
        .add_header("cookie", cookie)
        .await
        .assert_status(axum::http::StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn upload_session_page_with_existing_attachment_keeps_upload_form() {
    let app = upload_grant_app().await;
    let workspace_id = app.workspace_id("workspace");
    let submission_id = create_submission(&app, workspace_id).await;
    upload_attachment(
        &app,
        workspace_id,
        submission_id,
        "already-there.txt",
        b"existing",
    )
    .await;
    let issued = upload_grant_service(&app)
        .issue(&api_token_context(&app, workspace_id), submission_id.into())
        .await
        .expect("upload grant issues");

    let redeemed = app.server().get(&upload_path(&issued.url)).await;
    redeemed.assert_status(axum::http::StatusCode::SEE_OTHER);
    let page = app
        .server()
        .get("/evidence-attachment-uploads")
        .add_header("cookie", redeemed.header("set-cookie"))
        .await;
    page.assert_status_ok();
    let body = page.text();

    assert!(body.contains("already-there.txt"));
    assert!(body.contains(r#"<input id="file""#));
    assert!(body.contains(r#"action="/evidence-attachment-uploads/files""#));
    assert!(body.contains("pending"));
    assert!(!body.contains("Download"));
}

#[tokio::test]
async fn upload_session_post_accepts_native_form_and_enters_attachment_pipeline() {
    let app = upload_grant_app().await;
    let workspace_id = app.workspace_id("workspace");
    let submission_id = create_submission(&app, workspace_id).await;
    let cookie = upload_session_cookie(&app, workspace_id, submission_id).await;

    let response = app
        .server()
        .post("/evidence-attachment-uploads/files")
        .add_header("cookie", cookie.clone())
        .multipart(browser_upload_form(
            b"browser bytes",
            "browser-artifact.txt",
            "text/plain",
        ))
        .await;

    response.assert_status(axum::http::StatusCode::SEE_OTHER);
    assert_eq!(
        response.header("location").to_str().expect("location"),
        "/evidence-attachment-uploads"
    );
    let page = app
        .server()
        .get("/evidence-attachment-uploads")
        .add_header("cookie", cookie.clone())
        .await;
    page.assert_status_ok();
    let body = page.text();
    assert!(!body.contains("countdown"));
    assert!(!body.contains("window.close()"));
    assert!(body.contains("browser-artifact.txt"));
    assert!(body.contains("Current attachments"));
    assert!(body.contains("pending"));
    assert!(!body.contains("Download"));

    let row = app
        .postgres()
        .get()
        .await
        .expect("connection opens")
        .query_one(
            r#"
SELECT a.filename, a.checksum_crc32c, a.upload_status, count(o.id) AS outbox_count
FROM evidence_attachments a
LEFT JOIN outbox_messages o ON o.aggregate_id = a.id::text
WHERE a.evidence_submission_id = $1
GROUP BY a.id
"#,
            &[&submission_id],
        )
        .await
        .expect("attachment row reads");
    assert_eq!(row.get::<_, String>("filename"), "browser-artifact.txt");
    assert_eq!(
        row.get::<_, String>("checksum_crc32c"),
        super::support::crc32c_base64(b"browser bytes")
    );
    assert_eq!(row.get::<_, String>("upload_status"), "pending");
    assert_eq!(row.get::<_, i64>("outbox_count"), 1);

    app.server()
        .get("/evidence-attachment-uploads")
        .add_header("cookie", cookie)
        .await
        .assert_status_ok();
    assert_eq!(attachment_filenames(&app, submission_id).await.len(), 1);
}

#[tokio::test]
async fn upload_session_page_downloads_finalized_attachment() {
    let app = upload_grant_app().await;
    let workspace_id = app.workspace_id("workspace");
    let submission_id = create_submission(&app, workspace_id).await;
    let attachment = upload_attachment(
        &app,
        workspace_id,
        submission_id,
        "ready.txt",
        b"ready bytes",
    )
    .await;
    let attachment_id = Uuid::parse_str(attachment["id"].as_str().expect("attachment id"))
        .expect("attachment id parses");
    finalize_attachment(&app, workspace_id, submission_id, attachment_id).await;
    let cookie = upload_session_cookie(&app, workspace_id, submission_id).await;

    let page = app
        .server()
        .get("/evidence-attachment-uploads")
        .add_header("cookie", cookie.clone())
        .await;
    page.assert_status_ok();
    let body = page.text();
    assert!(body.contains("ready.txt"));
    assert!(body.contains("Download"));

    let redirect = app
        .server()
        .get(&format!(
            "/evidence-attachment-uploads/files/{attachment_id}/download"
        ))
        .add_header("cookie", cookie)
        .await;
    redirect.assert_status(axum::http::StatusCode::SEE_OTHER);
    let location_header = redirect.header("location");
    let location = location_header.to_str().expect("location");
    assert!(location.starts_with("https://api.proofplane.test/attachment-downloads?token="));
}

#[tokio::test]
async fn upload_session_archive_actions_follow_terminal_statuses() {
    let app = upload_grant_app().await;
    let workspace_id = app.workspace_id("workspace");
    let submission_id = create_submission(&app, workspace_id).await;
    let uploaded =
        upload_attachment(&app, workspace_id, submission_id, "uploaded.txt", b"up").await;
    let failed =
        upload_attachment(&app, workspace_id, submission_id, "failed.txt", b"failed").await;
    let malicious =
        upload_attachment(&app, workspace_id, submission_id, "virus.txt", b"virus").await;
    upload_attachment(&app, workspace_id, submission_id, "pending.txt", b"pending").await;
    finalize_attachment(&app, workspace_id, submission_id, attachment_id(&uploaded)).await;
    set_attachment_status(&app, attachment_id(&failed), "failed").await;
    set_attachment_status(&app, attachment_id(&malicious), "contains_virus").await;
    let cookie = upload_session_cookie(&app, workspace_id, submission_id).await;

    let body = app
        .server()
        .get("/evidence-attachment-uploads")
        .add_header("cookie", cookie)
        .await
        .text();

    assert!(body.contains("/archive"));
    assert!(body.contains("confirm('Archive this attachment?')"));
    assert_eq!(body.matches("/archive").count(), 3);
}

#[tokio::test]
async fn upload_session_archives_terminal_attachment_idempotently_and_hides_it() {
    let app = upload_grant_app().await;
    let workspace_id = app.workspace_id("workspace");
    let submission_id = create_submission(&app, workspace_id).await;
    let attachment = upload_attachment(
        &app,
        workspace_id,
        submission_id,
        "archive-me.txt",
        b"ready",
    )
    .await;
    let attachment_id = attachment_id(&attachment);
    finalize_attachment(&app, workspace_id, submission_id, attachment_id).await;
    let cookie = upload_session_cookie(&app, workspace_id, submission_id).await;
    let path = format!("/evidence-attachment-uploads/files/{attachment_id}/archive");

    for _ in 0..2 {
        let response = app
            .server()
            .post(&path)
            .add_header("cookie", cookie.clone())
            .await;
        response.assert_status(axum::http::StatusCode::SEE_OTHER);
        assert_eq!(
            response.header("location").to_str().expect("location"),
            "/evidence-attachment-uploads"
        );
    }

    let body = app
        .server()
        .get("/evidence-attachment-uploads")
        .add_header("cookie", cookie)
        .await
        .text();
    assert!(!body.contains("archive-me.txt"));
    assert_eq!(
        attachment_filenames(&app, submission_id).await,
        vec!["archive-me.txt"]
    );
}

#[tokio::test]
async fn upload_session_archive_rejects_non_terminal_attachment() {
    let app = upload_grant_app().await;
    let workspace_id = app.workspace_id("workspace");
    let submission_id = create_submission(&app, workspace_id).await;
    let attachment = upload_attachment(
        &app,
        workspace_id,
        submission_id,
        "still-pending.txt",
        b"pending",
    )
    .await;
    let cookie = upload_session_cookie(&app, workspace_id, submission_id).await;

    let response = app
        .server()
        .post(&format!(
            "/evidence-attachment-uploads/files/{}/archive",
            attachment_id(&attachment)
        ))
        .add_header("cookie", cookie)
        .await;

    response.assert_status(axum::http::StatusCode::CONFLICT);
    let body = response.text();
    assert!(body.contains("Archive failed: attachment is still processing"));
    assert!(body.contains("still-pending.txt"));
}

#[tokio::test]
async fn archived_attachment_cannot_receive_or_redeem_download_grants() {
    let app = upload_grant_app().await;
    let workspace_id = app.workspace_id("workspace");
    let submission_id = create_submission(&app, workspace_id).await;
    let attachment = upload_attachment(
        &app,
        workspace_id,
        submission_id,
        "download-gone.txt",
        b"ready",
    )
    .await;
    let attachment_id = attachment_id(&attachment);
    finalize_attachment(&app, workspace_id, submission_id, attachment_id).await;
    let grant = app
        .post(&format!(
            "/workspaces/{workspace_id}/evidence-submissions/{submission_id}/attachments/{attachment_id}/download-grants"
        ))
        .await;
    grant.assert_status_ok();
    let url = grant.json::<Value>()["url"]
        .as_str()
        .expect("download URL")
        .to_owned();
    let path = Url::parse(&url)
        .expect("download URL parses")
        .path()
        .to_owned()
        + "?"
        + Url::parse(&url)
            .expect("download URL parses")
            .query()
            .expect("download token query");
    let cookie = upload_session_cookie(&app, workspace_id, submission_id).await;

    app.server()
        .post(&format!(
            "/evidence-attachment-uploads/files/{attachment_id}/archive"
        ))
        .add_header("cookie", cookie)
        .await
        .assert_status(axum::http::StatusCode::SEE_OTHER);

    app.post(&format!(
        "/workspaces/{workspace_id}/evidence-submissions/{submission_id}/attachments/{attachment_id}/download-grants"
    ))
    .await
    .assert_status_not_found();
    app.get(&path).await.assert_status_not_found();
}

#[tokio::test]
async fn upload_session_post_accepts_more_than_one_browser_upload() {
    let app = upload_grant_app().await;
    let workspace_id = app.workspace_id("workspace");
    let submission_id = create_submission(&app, workspace_id).await;
    upload_attachment(&app, workspace_id, submission_id, "report.pdf", b"existing").await;
    let cookie = upload_session_cookie(&app, workspace_id, submission_id).await;

    let response = app
        .server()
        .post("/evidence-attachment-uploads/files")
        .add_header("cookie", cookie.clone())
        .multipart(browser_upload_form(b"new", "report.pdf", "application/pdf"))
        .await;
    response.assert_status(axum::http::StatusCode::SEE_OTHER);

    let filenames = attachment_filenames(&app, submission_id).await;
    assert_eq!(
        filenames,
        vec!["report.pdf".to_owned(), "report.pdf".to_owned()]
    );

    let page = app
        .server()
        .get("/evidence-attachment-uploads")
        .add_header("cookie", cookie)
        .await;
    page.assert_status_ok();
    let body = page.text();
    assert!(body.contains("report.pdf"));
    assert!(body.contains(r#"<input id="file""#));
}

#[tokio::test]
async fn rest_upload_duplicate_filename_behavior_remains_unchanged() {
    let app = upload_grant_app().await;
    let workspace_id = app.workspace_id("workspace");
    let submission_id = create_submission(&app, workspace_id).await;
    upload_attachment(&app, workspace_id, submission_id, "report.pdf", b"existing").await;

    app.post(&attachment_collection_path(workspace_id, submission_id))
        .multipart(super::support::attachment_form(
            b"rest duplicate",
            "report.pdf",
            "application/pdf",
            None,
        ))
        .await
        .assert_status(axum::http::StatusCode::ACCEPTED);

    assert_eq!(
        attachment_filenames(&app, submission_id)
            .await
            .iter()
            .filter(|filename| filename.as_str() == "report.pdf")
            .count(),
        2
    );
}

#[tokio::test]
async fn concurrent_browser_uploads_can_create_multiple_attachments() {
    let app = upload_grant_app().await;
    let workspace_id = app.workspace_id("workspace");
    let submission_id = create_submission(&app, workspace_id).await;
    let cookie = upload_session_cookie(&app, workspace_id, submission_id).await;

    let first = app
        .server()
        .post("/evidence-attachment-uploads/files")
        .add_header("cookie", cookie.clone())
        .multipart(browser_upload_form(b"one", "one.txt", "text/plain"));
    let second = app
        .server()
        .post("/evidence-attachment-uploads/files")
        .add_header("cookie", cookie)
        .multipart(browser_upload_form(b"two", "two.txt", "text/plain"));

    let (first, second) = tokio::join!(first, second);
    let statuses = [first.status_code(), second.status_code()];
    assert_eq!(
        statuses
            .iter()
            .filter(|status| **status == axum::http::StatusCode::SEE_OTHER)
            .count(),
        2
    );
    assert_eq!(
        attachment_filenames(&app, submission_id).await,
        vec!["one.txt".to_owned(), "two.txt".to_owned()]
    );
}

#[tokio::test]
async fn upload_session_post_unavailable_session_returns_generic_page() {
    let app = upload_grant_app().await;
    let response = app
        .server()
        .post("/evidence-attachment-uploads/files")
        .add_header("cookie", "proofplane_attachment_upload_session=not-a-token")
        .multipart(browser_upload_form(b"bytes", "artifact.txt", "text/plain"))
        .await;

    response.assert_status(axum::http::StatusCode::NOT_FOUND);
    assert!(response
        .text()
        .contains("This upload link is no longer available"));
}

#[tokio::test]
async fn upload_session_post_rejects_invalid_or_multiple_file_fields_with_page() {
    let app = upload_grant_app().await;
    let workspace_id = app.workspace_id("workspace");
    let submission_id = create_submission(&app, workspace_id).await;

    for form in [
        MultipartForm::new().add_part("note", Part::text("not a file")),
        browser_upload_form(b"bytes", "path/file.txt", "text/plain"),
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
        let cookie = upload_session_cookie(&app, workspace_id, submission_id).await;
        let response = app
            .server()
            .post("/evidence-attachment-uploads/files")
            .add_header("cookie", cookie)
            .multipart(form)
            .await;

        response.assert_status(axum::http::StatusCode::BAD_REQUEST);
        let body = response.text();
        assert!(body.contains("Upload failed"));
        assert!(body.contains("Current attachments"));
    }

    assert!(attachment_filenames(&app, submission_id).await.is_empty());
}

#[tokio::test]
async fn upload_session_missing_malformed_expired_tampered_or_wrong_purpose_cookie_is_unavailable()
{
    let app = upload_grant_app().await;
    let workspace_id = app.workspace_id("workspace");
    let submission_id = create_submission(&app, workspace_id).await;

    app.get("/evidence-attachment-uploads")
        .await
        .assert_status(axum::http::StatusCode::NOT_FOUND);
    app.get("/evidence-attachment-uploads")
        .add_header("cookie", "proofplane_attachment_upload_session=not-a-token")
        .await
        .assert_status(axum::http::StatusCode::NOT_FOUND);

    let expired = session_cookie(&app, workspace_id, submission_id, 1);
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    app.get("/evidence-attachment-uploads")
        .add_header("cookie", expired)
        .await
        .assert_status(axum::http::StatusCode::NOT_FOUND);

    let tampered = format!(
        "{}x",
        session_cookie(&app, workspace_id, submission_id, 900)
    );
    app.get("/evidence-attachment-uploads")
        .add_header("cookie", tampered)
        .await
        .assert_status(axum::http::StatusCode::NOT_FOUND);

    let wrong_purpose = format!(
        "proofplane_attachment_upload_session={}",
        issue_grant_token(&app, &upload_grant_service(&app), workspace_id).await
    );
    app.get("/evidence-attachment-uploads")
        .add_header("cookie", wrong_purpose)
        .await
        .assert_status(axum::http::StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn upload_session_cookie_secure_attribute_follows_public_api_base_url() {
    let https = upload_grant_app().await;
    let workspace_id = https.workspace_id("workspace");
    let submission_id = create_submission(&https, workspace_id).await;
    let issued = upload_grant_service(&https)
        .issue(
            &api_token_context(&https, workspace_id),
            submission_id.into(),
        )
        .await
        .expect("upload grant issues");
    assert!(https
        .get(&upload_path(&issued.url))
        .await
        .header("set-cookie")
        .to_str()
        .expect("set-cookie")
        .contains("; Secure"));

    let http = TestApp::builder()
        .with_public_api_base_url(Url::parse("http://api.proofplane.test/").unwrap())
        .workspace("workspace", "Upload grant workspace")
        .with_default_membership()
        .build()
        .await;
    let workspace_id = http.workspace_id("workspace");
    let submission_id = create_submission(&http, workspace_id).await;
    let issued = upload_grant_service(&http)
        .issue(
            &api_token_context(&http, workspace_id),
            submission_id.into(),
        )
        .await
        .expect("upload grant issues");
    assert!(!http
        .get(&upload_path(&issued.url))
        .await
        .header("set-cookie")
        .to_str()
        .expect("set-cookie")
        .contains("; Secure"));
}

async fn upload_grant_app() -> TestApp {
    TestApp::builder()
        .workspace("workspace", "Upload grant workspace")
        .with_default_membership()
        .workspace("other", "Other upload grant workspace")
        .with_default_membership()
        .build()
        .await
}

fn upload_grant_service(app: &TestApp) -> AttachmentUploadGrantService {
    let config = upload_grant_config();
    let base_url = app.public_api_base_url().clone();
    AttachmentUploadGrantService::new(
        app.postgres_arc(),
        base_url.clone(),
        UploadGrantEncryptor::from_config(
            base_url.clone(),
            "proofplane-attachment-upload-grant",
            &config,
        )
        .expect("upload grant encryptor initializes"),
        UploadGrantDecryptor::from_config(base_url, "proofplane-attachment-upload-grant", &config)
            .expect("upload grant decryptor initializes"),
    )
}

fn upload_path(url: &Url) -> String {
    let mut path = url.path().to_owned();
    if let Some(query) = url.query() {
        path.push('?');
        path.push_str(query);
    }
    path
}

async fn upload_session_cookie(app: &TestApp, workspace_id: Uuid, submission_id: Uuid) -> String {
    let issued = upload_grant_service(app)
        .issue(&api_token_context(app, workspace_id), submission_id.into())
        .await
        .expect("upload grant issues");
    app.get(&upload_path(&issued.url))
        .await
        .header("set-cookie")
        .to_str()
        .expect("set-cookie")
        .to_owned()
}

fn browser_upload_form(bytes: &[u8], filename: &str, content_type: &str) -> MultipartForm {
    MultipartForm::new().add_part(
        "file",
        Part::bytes(bytes.to_vec())
            .file_name(filename)
            .mime_type(content_type),
    )
}

fn cookie_max_age(cookie: &str) -> i64 {
    cookie
        .split(';')
        .map(str::trim)
        .find_map(|part| part.strip_prefix("Max-Age="))
        .expect("cookie contains Max-Age")
        .parse()
        .expect("Max-Age is an integer")
}

async fn attachment_filenames(app: &TestApp, submission_id: Uuid) -> Vec<String> {
    app.postgres()
        .get()
        .await
        .expect("connection opens")
        .query(
            "SELECT filename FROM evidence_attachments WHERE evidence_submission_id = $1 ORDER BY filename",
            &[&submission_id],
        )
        .await
        .expect("attachment filenames read")
        .into_iter()
        .map(|row| row.get("filename"))
        .collect()
}

fn attachment_id(attachment: &Value) -> Uuid {
    Uuid::parse_str(attachment["id"].as_str().expect("attachment id"))
        .expect("attachment id parses")
}

async fn set_attachment_status(app: &TestApp, attachment_id: Uuid, status: &str) {
    app.postgres()
        .get()
        .await
        .expect("connection opens")
        .execute(
            "UPDATE evidence_attachments SET upload_status = $2 WHERE id = $1",
            &[&attachment_id, &status],
        )
        .await
        .expect("attachment status updates");
}

fn session_cookie(
    app: &TestApp,
    workspace_id: Uuid,
    submission_id: Uuid,
    ttl_seconds: i64,
) -> String {
    let token = UploadSessionEncryptor::from_config(
        app.public_api_base_url().clone(),
        UPLOAD_SESSION_AUDIENCE,
        &upload_grant_config(),
    )
    .expect("upload session encryptor initializes")
    .encrypt(
        RegisteredClaims {
            subject: app.user_id(),
            token_id: Uuid::new_v4(),
            expires_at: Utc::now() + chrono::Duration::seconds(ttl_seconds),
        },
        &TestUploadSessionClaims {
            version: 1,
            workspace_id: workspace_id.to_string(),
            submission_id: submission_id.to_string(),
            issued_by_user_id: app.user_id().to_string(),
            issued_via_api_token_id: app.api_token_id().to_string(),
        },
    )
    .expect("upload session token issues")
    .token;

    format!("proofplane_attachment_upload_session={token}")
}

#[derive(Serialize)]
struct TestUploadSessionClaims {
    version: u8,
    workspace_id: String,
    submission_id: String,
    issued_by_user_id: String,
    issued_via_api_token_id: String,
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

fn api_token_context(app: &TestApp, workspace_id: Uuid) -> ApiTokenContext {
    ApiTokenContext {
        user_id: app.user_id().into(),
        api_token_id: app.api_token_id().into(),
        workspace_id: workspace_id.into(),
        permissions: WorkspacePermissions::from_iter(WorkspacePermission::ALL),
    }
}

async fn issue_grant_token(
    app: &TestApp,
    service: &AttachmentUploadGrantService,
    workspace_id: Uuid,
) -> String {
    let submission_id = create_submission(app, workspace_id).await;
    let issued = service
        .issue(&api_token_context(app, workspace_id), submission_id.into())
        .await
        .expect("upload grant issues");

    token_from_url(&issued.url).to_owned()
}

fn token_from_url(url: &Url) -> String {
    url.query_pairs()
        .find_map(|(key, value)| (key == "token").then(|| value.into_owned()))
        .expect("token query exists")
}

fn assert_unavailable(result: Result<impl std::fmt::Debug, UploadGrantError>) {
    assert!(matches!(result, Err(UploadGrantError::Unavailable)));
}

async fn create_submission(app: &TestApp, workspace_id: Uuid) -> Uuid {
    let request = app
        .create_evidence_request(workspace_id, &evidence_request("Upload grant target"))
        .await;
    let evidence_request_id = created_id(&request);
    let created = app
        .post(&collection_path(workspace_id, evidence_request_id))
        .json(&evidence_submission())
        .await
        .json::<Value>();

    created_id(&created)
}

fn evidence_request(title: &str) -> Value {
    json!({
        "title": title,
        "description": format!("Collect evidence for {title}."),
        "collection_instructions": format!("Upload the artifact for {title}."),
        "cadence": "quarterly",
        "due_at": dynamic_due_at(),
        "schedule_anchor_at": "2026-01-01T00:00:00Z",
        "freshness_window_days": 90,
        "status": "active"
    })
}

fn evidence_submission() -> Value {
    json!({
        "coverage_start_at": "2026-01-01T00:00:00Z",
        "coverage_end_at": "2026-03-31T23:59:59Z",
        "source_system": "okta",
        "collection_method": "api_export"
    })
}

fn dynamic_due_at() -> String {
    (Utc::now() + Duration::days(7)).to_rfc3339_opts(SecondsFormat::Secs, true)
}

fn collection_path(workspace_id: Uuid, evidence_request_id: Uuid) -> String {
    format!("/workspaces/{workspace_id}/evidence-requests/{evidence_request_id}/submissions")
}

fn attachment_collection_path(workspace_id: Uuid, submission_id: Uuid) -> String {
    format!("/workspaces/{workspace_id}/evidence-submissions/{submission_id}/attachments")
}

fn created_id(value: &Value) -> Uuid {
    Uuid::parse_str(value["id"].as_str().expect("id is a string")).expect("id parses")
}

async fn finalize_attachment(
    app: &TestApp,
    workspace_id: Uuid,
    submission_id: Uuid,
    attachment_id: Uuid,
) {
    let client = app.postgres().get().await.expect("connection opens");
    let row = client
        .query_one(
            "SELECT object_key, filename FROM evidence_attachments WHERE id = $1",
            &[&attachment_id],
        )
        .await
        .expect("attachment loads");
    let quarantine_key =
        ObjectKey::parse(row.get::<_, String>("object_key")).expect("quarantine key parses");
    let filename: String = row.get("filename");
    let final_key = ObjectKey::parse(format!(
        "workspaces/{workspace_id}/evidence-submissions/{submission_id}/attachments/{attachment_id}/{filename}"
    ))
    .expect("final key parses");
    let store = FilesystemObjectStore::new(app.object_storage_root())
        .await
        .expect("filesystem store initializes");
    store
        .copy_object(&quarantine_key, &final_key)
        .await
        .expect("attachment copies to final storage");
    client
        .execute(
            "UPDATE evidence_attachments SET object_key = $2, upload_status = 'uploaded' WHERE id = $1",
            &[&attachment_id, &final_key.as_str()],
        )
        .await
        .expect("attachment finalizes");
}
