use std::time::{Duration as StdDuration, SystemTime};

use axum::http::StatusCode;
use jwtk::{
    hmac::{HmacAlgorithm, HmacKey},
    sign, HeaderAndClaims,
};
use proofplane::domain::{ActorKind, CreateActorPayload, WorkspaceId};
use proofplane::routes::authentication::{ACTOR_ID_HEADER, API_KEY_HEADER};
use proofplane::{
    domain::WorkspacePermission,
    object_storage::{FilesystemObjectStore, ObjectKey, ObjectStore},
};
use serde::Serialize;
use serde_json::Value;
use uuid::Uuid;

use super::support::{upload_attachment, TestApp, INTEGRATION_ACTOR_ID};

const SIGNING_SECRET: &[u8] = b"0123456789abcdef0123456789abcdef";
const ISSUER: &str = "https://api.proofplane.test/";
const AUDIENCE: &str = "proofplane-attachment-download";

#[tokio::test]
async fn uploaded_attachment_grant_streams_reusably_with_safe_headers() {
    let app = grant_test_app().await;
    let workspace_id = app.workspace_id("workspace");
    let submission_id = create_submission(&app, workspace_id).await;
    let content = b"downloadable evidence";
    let attachment = upload_attachment(
        &app,
        workspace_id,
        submission_id,
        "Quarterly evidence (final).txt",
        content,
    )
    .await;
    let attachment_id = attachment_id(&attachment);
    finalize_attachment(&app, workspace_id, submission_id, attachment_id).await;

    let response = app
        .post(&grant_path(workspace_id, submission_id, attachment_id))
        .await;
    response.assert_status_ok();
    let grant = response.json::<Value>();
    assert_eq!(grant["filename"], "Quarterly evidence (final).txt");
    assert_eq!(grant["content_type"], "text/plain");
    assert_eq!(grant["content_length"], content.len() as i64);
    let download_path = local_download_path(grant["url"].as_str().expect("URL is a string"));

    let (first, second) = tokio::join!(
        app.server().get(&download_path),
        app.server().get(&download_path)
    );

    for response in [&first, &second] {
        response.assert_status_ok();
        assert_eq!(response.as_bytes().as_ref(), content);
        assert_eq!(response.header("content-type"), "text/plain");
        assert_eq!(
            response.header("content-disposition"),
            "attachment; filename=\"Quarterly evidence (final).txt\""
        );
        assert_eq!(response.header("cache-control"), "private, no-store");
        assert_eq!(response.header("referrer-policy"), "no-referrer");
    }
}

#[tokio::test]
async fn grant_issuance_requires_actor_read_evidence_submissions_permission() {
    let app = TestApp::builder()
        .workspace("workspace", "Download workspace")
        .with_default_membership()
        .workspace("other", "Other workspace")
        .without_membership()
        .build()
        .await;
    let workspace_id = app.workspace_id("workspace");
    let other_workspace_id = app.workspace_id("other");
    let submission_id = create_submission(&app, workspace_id).await;
    let attachment =
        upload_attachment(&app, workspace_id, submission_id, "scoped.txt", b"scoped").await;
    let attachment_id = attachment_id(&attachment);
    finalize_attachment(&app, workspace_id, submission_id, attachment_id).await;
    let path = grant_path(workspace_id, submission_id, attachment_id);

    let (reader_actor_id, reader_api_key) = app
        .issue_actor(
            workspace_id,
            vec![WorkspacePermission::ReadEvidenceSubmissions],
        )
        .await;
    app.server()
        .post(&path)
        .clear_headers()
        .add_header(ACTOR_ID_HEADER, reader_actor_id)
        .add_header(API_KEY_HEADER, reader_api_key)
        .await
        .assert_status_ok();

    let (other_actor_id, other_api_key) = app
        .issue_actor(
            other_workspace_id,
            vec![WorkspacePermission::ReadEvidenceSubmissions],
        )
        .await;
    app.server()
        .post(&path)
        .clear_headers()
        .add_header(ACTOR_ID_HEADER, other_actor_id)
        .add_header(API_KEY_HEADER, other_api_key)
        .await
        .assert_status_not_found();

    let (limited_actor_id, limited_api_key) = app
        .issue_actor(
            workspace_id,
            vec![WorkspacePermission::ReadEvidenceRequests],
        )
        .await;
    app.server()
        .post(&path)
        .clear_headers()
        .add_header(ACTOR_ID_HEADER, &limited_actor_id)
        .add_header(API_KEY_HEADER, &limited_api_key)
        .await
        .assert_status_not_found();

    app.server()
        .post(&path)
        .clear_headers()
        .await
        .assert_status_unauthorized();
    app.server()
        .post(&path)
        .clear_headers()
        .add_header(ACTOR_ID_HEADER, limited_actor_id)
        .add_header(API_KEY_HEADER, "invalid")
        .await
        .assert_status_unauthorized();
}

#[tokio::test]
async fn issuance_uses_read_auth_and_conceals_wrong_scope_and_terminal_statuses() {
    let app = TestApp::builder()
        .workspace("workspace", "Download workspace")
        .with_default_membership()
        .workspace("other", "Other workspace")
        .with_default_membership()
        .workspace("ungranted", "Ungranted workspace")
        .without_membership()
        .build()
        .await;
    let workspace_id = app.workspace_id("workspace");
    let other_workspace_id = app.workspace_id("other");
    let ungranted_workspace_id = app.workspace_id("ungranted");
    let submission_id = create_submission(&app, workspace_id).await;
    let attachment =
        upload_attachment(&app, workspace_id, submission_id, "pending.txt", b"pending").await;
    let attachment_id = attachment_id(&attachment);
    let path = grant_path(workspace_id, submission_id, attachment_id);

    let pending = app.post(&path).await;
    pending.assert_status(StatusCode::CONFLICT);
    assert_eq!(
        pending.json::<Value>()["error"]["code"],
        "attachment_not_ready"
    );

    set_attachment_status(&app, attachment_id, "finalizing").await;
    app.post(&path).await.assert_status(StatusCode::CONFLICT);

    set_attachment_status(&app, attachment_id, "contains_virus").await;
    app.post(&path).await.assert_status_not_found();
    set_attachment_status(&app, attachment_id, "failed").await;
    app.post(&path).await.assert_status_not_found();

    app.post(&grant_path(
        other_workspace_id,
        submission_id,
        attachment_id,
    ))
    .await
    .assert_status_not_found();
    app.post(&grant_path(workspace_id, Uuid::new_v4(), attachment_id))
        .await
        .assert_status_not_found();
    app.post(&grant_path(
        ungranted_workspace_id,
        submission_id,
        attachment_id,
    ))
    .await
    .assert_status_not_found();
    app.server()
        .post(&path)
        .clear_headers()
        .await
        .assert_status_unauthorized();
    app.server()
        .post(&path)
        .clear_headers()
        .add_header(ACTOR_ID_HEADER, app.actor_id())
        .add_header(API_KEY_HEADER, "invalid")
        .await
        .assert_status_unauthorized();
}

#[tokio::test]
async fn redemption_conceals_invalid_tokens_and_newly_ineligible_or_missing_objects() {
    let app = grant_test_app().await;
    let workspace_id = app.workspace_id("workspace");
    let submission_id = create_submission(&app, workspace_id).await;
    let attachment = upload_attachment(
        &app,
        workspace_id,
        submission_id,
        "artifact.txt",
        b"artifact",
    )
    .await;
    let attachment_id = attachment_id(&attachment);
    let final_key = finalize_attachment(&app, workspace_id, submission_id, attachment_id).await;
    let download_path = issue_download_path(&app, workspace_id, submission_id, attachment_id).await;

    for path in [
        "/attachment-downloads",
        "/attachment-downloads?other=value",
        "/attachment-downloads?token=",
        "/attachment-downloads?token=a&token=b",
    ] {
        app.server().get(path).await.assert_status_not_found();
    }

    app.server()
        .get("/attachment-downloads?token=malformed")
        .await
        .assert_status_not_found();
    let mut tampered = download_path.clone().into_bytes();
    let signature_byte = tampered.len() - 2;
    tampered[signature_byte] = if tampered[signature_byte] == b'A' {
        b'B'
    } else {
        b'A'
    };
    app.server()
        .get(std::str::from_utf8(&tampered).expect("tampered path is UTF-8"))
        .await
        .assert_status_not_found();

    let expired = signed_token(1, true, workspace_id, submission_id, attachment_id);
    app.server()
        .get(&format!("/attachment-downloads?token={expired}"))
        .await
        .assert_status_not_found();
    let unknown_version = signed_token(2, false, workspace_id, submission_id, attachment_id);
    app.server()
        .get(&format!("/attachment-downloads?token={unknown_version}"))
        .await
        .assert_status_not_found();
    let deleted_issuer = app
        .postgres()
        .create_actor(&CreateActorPayload {
            id: None,
            kind: ActorKind::ServiceAccount,
            display_name: "Deleted Download Issuer".to_owned(),
            workspace_id: WorkspaceId::from(workspace_id),
            created_by_user_id: None,
            permissions: WorkspacePermission::ALL.to_vec(),
        })
        .await
        .expect("deleted issuer creates");
    assert!(app
        .postgres()
        .delete_actor(deleted_issuer.id)
        .await
        .expect("deleted issuer deletes"));
    let deleted_issuer_token = signed_token_for_issuer(
        1,
        false,
        workspace_id,
        submission_id,
        attachment_id,
        deleted_issuer.id.to_string(),
    );
    app.server()
        .get(&format!(
            "/attachment-downloads?token={deleted_issuer_token}"
        ))
        .await
        .assert_status_not_found();

    set_attachment_status(&app, attachment_id, "contains_virus").await;
    app.server()
        .get(&download_path)
        .await
        .assert_status_not_found();

    set_attachment_status(&app, attachment_id, "uploaded").await;
    let store = filesystem_store(&app).await;
    store
        .delete_object(&final_key)
        .await
        .expect("object deletes");
    app.server()
        .get(&download_path)
        .await
        .assert_status_not_found();
}

#[tokio::test]
async fn metadata_mismatch_is_internal_error_and_object_keys_are_never_public() {
    let app = grant_test_app().await;
    let workspace_id = app.workspace_id("workspace");
    let submission_id = create_submission(&app, workspace_id).await;
    let attachment = upload_attachment(
        &app,
        workspace_id,
        submission_id,
        "artifact.txt",
        b"artifact",
    )
    .await;
    let attachment_id = attachment_id(&attachment);
    let final_key = finalize_attachment(&app, workspace_id, submission_id, attachment_id).await;
    assert!(attachment.get("object_key").is_none());

    let detail = app
        .get(&format!(
            "/workspaces/{workspace_id}/evidence-submissions/{submission_id}"
        ))
        .await
        .json::<Value>();
    assert!(detail["attachments"][0].get("object_key").is_none());

    let download_path = issue_download_path(&app, workspace_id, submission_id, attachment_id).await;
    let metadata_path = app
        .object_storage_root()
        .join("metadata")
        .join(format!("{}.json", final_key.as_str()));
    let mut metadata: Value =
        serde_json::from_slice(&std::fs::read(&metadata_path).expect("metadata reads"))
            .expect("metadata parses");
    metadata["sha256"] = Value::String("0".repeat(64));
    std::fs::write(
        metadata_path,
        serde_json::to_vec_pretty(&metadata).expect("metadata serializes"),
    )
    .expect("metadata writes");

    app.server()
        .get(&download_path)
        .await
        .assert_status_internal_server_error();
    app.post(&grant_path(workspace_id, submission_id, attachment_id))
        .await
        .assert_status_internal_server_error();
}

async fn grant_test_app() -> TestApp {
    TestApp::builder()
        .workspace("workspace", "Download workspace")
        .with_default_membership()
        .build()
        .await
}

async fn create_submission(app: &TestApp, workspace_id: Uuid) -> Uuid {
    let request = app
        .create_evidence_request(
            workspace_id,
            &serde_json::json!({
                "title": "Download evidence",
                "description": "Download evidence request",
                "collection_instructions": "Upload download evidence.",
                "cadence": "quarterly",
                "due_at": "2099-01-01T00:00:00Z",
                "schedule_anchor_at": "2026-01-01T00:00:00Z",
                "freshness_window_days": 90,
                "status": "active",
            }),
        )
        .await;
    let response = app
        .post(&format!(
            "/workspaces/{workspace_id}/evidence-requests/{}/submissions",
            request["id"].as_str().expect("request ID is a string")
        ))
        .json(&serde_json::json!({
            "coverage_start_at": "2026-01-01T00:00:00Z",
            "coverage_end_at": "2026-01-31T23:59:59Z",
            "source_system": "integration",
            "collection_method": "manual_upload",
        }))
        .await;
    response.assert_status_ok();
    Uuid::parse_str(
        response.json::<Value>()["id"]
            .as_str()
            .expect("submission ID is a string"),
    )
    .expect("submission ID is a UUID")
}

async fn finalize_attachment(
    app: &TestApp,
    workspace_id: Uuid,
    submission_id: Uuid,
    attachment_id: Uuid,
) -> ObjectKey {
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
    let store = filesystem_store(app).await;
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

    final_key
}

async fn filesystem_store(app: &TestApp) -> FilesystemObjectStore {
    FilesystemObjectStore::new(app.object_storage_root())
        .await
        .expect("filesystem store initializes")
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

async fn issue_download_path(
    app: &TestApp,
    workspace_id: Uuid,
    submission_id: Uuid,
    attachment_id: Uuid,
) -> String {
    let response = app
        .post(&grant_path(workspace_id, submission_id, attachment_id))
        .await;
    response.assert_status_ok();
    local_download_path(
        response.json::<Value>()["url"]
            .as_str()
            .expect("download URL is a string"),
    )
}

fn local_download_path(url: &str) -> String {
    let url = url::Url::parse(url).expect("download URL parses");
    format!(
        "{}?{}",
        url.path(),
        url.query().expect("download URL contains a query")
    )
}

fn grant_path(workspace_id: Uuid, submission_id: Uuid, attachment_id: Uuid) -> String {
    format!(
        "/workspaces/{workspace_id}/evidence-submissions/{submission_id}/attachments/{attachment_id}/download-grants"
    )
}

fn attachment_id(attachment: &Value) -> Uuid {
    Uuid::parse_str(
        attachment["id"]
            .as_str()
            .expect("attachment ID is a string"),
    )
    .expect("attachment ID is a UUID")
}

#[derive(Serialize)]
struct TestDownloadClaims {
    version: u8,
    workspace_id: String,
    submission_id: String,
    attachment_id: String,
    issued_by: String,
}

fn signed_token(
    version: u8,
    expired: bool,
    workspace_id: Uuid,
    submission_id: Uuid,
    attachment_id: Uuid,
) -> String {
    signed_token_for_issuer(
        version,
        expired,
        workspace_id,
        submission_id,
        attachment_id,
        INTEGRATION_ACTOR_ID.to_owned(),
    )
}

fn signed_token_for_issuer(
    version: u8,
    expired: bool,
    workspace_id: Uuid,
    submission_id: Uuid,
    attachment_id: Uuid,
    issued_by: String,
) -> String {
    let key = HmacKey::from_bytes(SIGNING_SECRET, HmacAlgorithm::HS256);
    let mut claims = HeaderAndClaims::with_claims(TestDownloadClaims {
        version,
        workspace_id: workspace_id.to_string(),
        submission_id: submission_id.to_string(),
        attachment_id: attachment_id.to_string(),
        issued_by,
    });
    claims
        .set_iss(ISSUER)
        .add_aud(AUDIENCE)
        .set_jti(Uuid::new_v4().to_string());
    if expired {
        let expired_at = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .expect("system time is after epoch")
            - StdDuration::from_secs(60);
        claims.claims_mut().iat = Some(expired_at - StdDuration::from_secs(300));
        claims.claims_mut().exp = Some(expired_at);
    } else {
        claims
            .set_iat_now()
            .set_exp_from_now(StdDuration::from_secs(300));
    }

    sign(&mut claims, &key).expect("test token signs")
}
