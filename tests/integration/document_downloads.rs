use std::time::{Duration as StdDuration, SystemTime};

use axum::http::StatusCode;
use chrono::{SecondsFormat, Utc};
use jwtk::{
    hmac::{HmacAlgorithm, HmacKey},
    sign, HeaderAndClaims,
};
use pasetors::{
    keys::SymmetricKey,
    version4::{LocalToken, V4},
};
use proofplane::authentication::paseto::{DownloadGrantEncryptor, RegisteredClaims};
use proofplane::config::{PasetoDownloadConfig, PasetoDownloadKey};
use proofplane::routes::authentication::AUTHORIZATION_HEADER;
use proofplane::{
    domain::WorkspacePermission,
    object_storage::{FilesystemObjectStore, ObjectStore},
};
use secrecy::SecretString;
use serde::Serialize;
use serde_json::Value;
use uuid::Uuid;

use super::support::{finalize_document, upload_document, TestApp, INTEGRATION_API_TOKEN_ID};

const SIGNING_SECRET: &[u8] = b"0123456789abcdef0123456789abcdef";
const ISSUER: &str = "https://api.proofplane.test/";
const AUDIENCE: &str = "proofplane-document-download";
const DOWNLOAD_KEY_ID: &str = "integration-download-001";
const DOWNLOAD_KEY: &str = "k4.local.mKj2EzeLOuNBNlHNX6oLl76yopCc1K9YvWQVIo1xYEs";
const DOWNLOAD_IMPLICIT_ASSERTION: &[u8] = b"proofplane:document-download:v1";

#[tokio::test]
async fn uploaded_document_grant_streams_reusably_with_safe_headers() {
    let app = grant_test_app().await;
    let workspace_id = app.workspace_id("workspace");
    let submission_id = create_submission(&app, workspace_id).await;
    let content = b"downloadable evidence";
    let document = upload_document(
        &app,
        workspace_id,
        submission_id,
        "Quarterly evidence (final).txt",
        content,
    )
    .await;
    let document_id = document_id(&document);
    finalize_document(&app, workspace_id, submission_id, document_id).await;

    let response = app
        .post(&grant_path(workspace_id, submission_id, document_id))
        .await;
    response.assert_status_ok();
    let grant = response.json::<Value>();
    assert_eq!(grant["filename"], "Quarterly evidence (final).txt");
    assert_eq!(grant["content_type"], "text/plain");
    assert_eq!(grant["content_length"], content.len() as i64);
    let url = grant["url"].as_str().expect("URL is a string");
    let grant_token = download_token(url);
    assert!(grant_token.starts_with("v4.local."));
    for hidden in [
        workspace_id,
        submission_id,
        document_id,
        app.user_id(),
        app.api_token_id(),
    ] {
        assert!(!grant_token.contains(&hidden.to_string()));
    }
    let download_path = local_download_path(url);

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
            "document; filename=\"Quarterly evidence (final).txt\""
        );
        assert_eq!(response.header("cache-control"), "private, no-store");
        assert_eq!(response.header("referrer-policy"), "no-referrer");
    }
}

#[tokio::test]
async fn grant_issuance_requires_api_token_read_evidence_submissions_permission() {
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
    let document =
        upload_document(&app, workspace_id, submission_id, "scoped.txt", b"scoped").await;
    let document_id = document_id(&document);
    finalize_document(&app, workspace_id, submission_id, document_id).await;
    let path = grant_path(workspace_id, submission_id, document_id);

    let reader = app
        .issue_api_token(
            workspace_id,
            vec![WorkspacePermission::ReadEvidenceSubmissions],
        )
        .await;
    app.server()
        .post(&path)
        .clear_headers()
        .add_header(AUTHORIZATION_HEADER, format!("Bearer {}", reader.raw_token))
        .await
        .assert_status_ok();

    let other = app
        .issue_api_token(
            other_workspace_id,
            vec![WorkspacePermission::ReadEvidenceSubmissions],
        )
        .await;
    app.server()
        .post(&path)
        .clear_headers()
        .add_header(AUTHORIZATION_HEADER, format!("Bearer {}", other.raw_token))
        .await
        .assert_status_not_found();

    let limited = app
        .issue_api_token(
            workspace_id,
            vec![WorkspacePermission::ReadEvidenceRequests],
        )
        .await;
    app.server()
        .post(&path)
        .clear_headers()
        .add_header(
            AUTHORIZATION_HEADER,
            format!("Bearer {}", limited.raw_token),
        )
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
        .add_header(AUTHORIZATION_HEADER, "Bearer invalid")
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
    let document =
        upload_document(&app, workspace_id, submission_id, "pending.txt", b"pending").await;
    let document_id = document_id(&document);
    let path = grant_path(workspace_id, submission_id, document_id);

    let pending = app.post(&path).await;
    pending.assert_status(StatusCode::CONFLICT);
    assert_eq!(
        pending.json::<Value>()["error"]["code"],
        "document_not_ready"
    );

    set_document_status(&app, document_id, "finalizing").await;
    app.post(&path).await.assert_status(StatusCode::CONFLICT);

    set_document_status(&app, document_id, "contains_virus").await;
    app.post(&path).await.assert_status_not_found();
    set_document_status(&app, document_id, "failed").await;
    app.post(&path).await.assert_status_not_found();

    app.post(&grant_path(
        other_workspace_id,
        submission_id,
        document_id,
    ))
    .await
    .assert_status_not_found();
    app.post(&grant_path(workspace_id, Uuid::new_v4(), document_id))
        .await
        .assert_status_not_found();
    app.post(&grant_path(
        ungranted_workspace_id,
        submission_id,
        document_id,
    ))
    .await
    .assert_status_not_found();
    app.server()
        .post(&path)
        .clear_headers()
        .await
        .assert_status_unauthorized();
}

#[tokio::test]
async fn redemption_conceals_invalid_tokens_and_newly_ineligible_or_missing_objects() {
    let app = grant_test_app().await;
    let workspace_id = app.workspace_id("workspace");
    let submission_id = create_submission(&app, workspace_id).await;
    let document = upload_document(
        &app,
        workspace_id,
        submission_id,
        "artifact.txt",
        b"artifact",
    )
    .await;
    let document_id = document_id(&document);
    let final_key = finalize_document(&app, workspace_id, submission_id, document_id).await;
    let download_path = issue_download_path(&app, workspace_id, submission_id, document_id).await;

    for path in [
        "/document-downloads",
        "/document-downloads?other=value",
        "/document-downloads?token=",
        "/document-downloads?token=a&token=b",
    ] {
        app.server().get(path).await.assert_status_not_found();
    }

    app.server()
        .get("/document-downloads?token=malformed")
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

    let expired = paseto_token(
        2,
        true,
        workspace_id,
        submission_id,
        document_id,
        app.user_id(),
    );
    app.server()
        .get(&format!("/document-downloads?token={expired}"))
        .await
        .assert_status_not_found();
    let unknown_version = paseto_token(
        3,
        false,
        workspace_id,
        submission_id,
        document_id,
        app.user_id(),
    );
    app.server()
        .get(&format!("/document-downloads?token={unknown_version}"))
        .await
        .assert_status_not_found();
    let legacy_jwt = signed_token(1, false, workspace_id, submission_id, document_id);
    app.server()
        .get(&format!("/document-downloads?token={legacy_jwt}"))
        .await
        .assert_status_not_found();
    set_document_status(&app, document_id, "contains_virus").await;
    app.server()
        .get(&download_path)
        .await
        .assert_status_not_found();

    set_document_status(&app, document_id, "uploaded").await;
    let store = FilesystemObjectStore::new(app.object_storage_root())
        .await
        .expect("filesystem store initializes");
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
    let document = upload_document(
        &app,
        workspace_id,
        submission_id,
        "artifact.txt",
        b"artifact",
    )
    .await;
    let document_id = document_id(&document);
    let final_key = finalize_document(&app, workspace_id, submission_id, document_id).await;
    assert!(document.get("object_key").is_none());

    let detail = app
        .get(&format!(
            "/workspaces/{workspace_id}/evidence-submissions/{submission_id}"
        ))
        .await
        .json::<Value>();
    assert!(detail["documents"][0].get("object_key").is_none());

    let download_path = issue_download_path(&app, workspace_id, submission_id, document_id).await;
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
    app.post(&grant_path(workspace_id, submission_id, document_id))
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

async fn issue_download_path(
    app: &TestApp,
    workspace_id: Uuid,
    submission_id: Uuid,
    document_id: Uuid,
) -> String {
    let response = app
        .post(&grant_path(workspace_id, submission_id, document_id))
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

fn download_token(url: &str) -> String {
    let url = url::Url::parse(url).expect("download URL parses");
    url.query_pairs()
        .find_map(|(key, value)| (key == "token").then(|| value.into_owned()))
        .expect("download URL contains token")
}

fn grant_path(workspace_id: Uuid, submission_id: Uuid, document_id: Uuid) -> String {
    format!(
        "/workspaces/{workspace_id}/evidence-submissions/{submission_id}/documents/{document_id}/download-grants"
    )
}

fn document_id(document: &Value) -> Uuid {
    Uuid::parse_str(
        document["id"]
            .as_str()
            .expect("document ID is a string"),
    )
    .expect("document ID is a UUID")
}

#[derive(Serialize)]
struct TestDownloadClaims {
    version: u8,
    workspace_id: String,
    submission_id: String,
    document_id: String,
    issued_by_user_id: String,
    issued_via_api_token_id: String,
}

fn paseto_token(
    version: u8,
    expired: bool,
    workspace_id: Uuid,
    submission_id: Uuid,
    document_id: Uuid,
    issued_by_user_id: Uuid,
) -> String {
    let claims = TestDownloadClaims {
        version,
        workspace_id: workspace_id.to_string(),
        submission_id: submission_id.to_string(),
        document_id: document_id.to_string(),
        issued_by_user_id: issued_by_user_id.to_string(),
        issued_via_api_token_id: INTEGRATION_API_TOKEN_ID.to_owned(),
    };
    if expired {
        return expired_paseto_token(claims, issued_by_user_id);
    }

    DownloadGrantEncryptor::from_config(
        url::Url::parse(ISSUER).expect("issuer parses"),
        AUDIENCE,
        &download_config(),
    )
    .expect("download grant encryptor initializes")
    .encrypt(
        RegisteredClaims {
            subject: issued_by_user_id,
            token_id: Uuid::new_v4(),
            expires_at: Utc::now() + chrono::Duration::minutes(5),
        },
        &claims,
    )
    .expect("test PASETO token encrypts")
    .token
}

fn expired_paseto_token(claims: TestDownloadClaims, subject: Uuid) -> String {
    let key = SymmetricKey::<V4>::try_from(DOWNLOAD_KEY).expect("download key parses");
    let now = Utc::now();
    let payload = serde_json::json!({
        "iss": ISSUER,
        "aud": AUDIENCE,
        "sub": subject.to_string(),
        "jti": Uuid::new_v4().to_string(),
        "iat": (now - chrono::Duration::minutes(10)).to_rfc3339_opts(SecondsFormat::Secs, true),
        "nbf": (now - chrono::Duration::minutes(10)).to_rfc3339_opts(SecondsFormat::Secs, true),
        "exp": (now - chrono::Duration::minutes(5)).to_rfc3339_opts(SecondsFormat::Secs, true),
        "version": claims.version,
        "workspace_id": claims.workspace_id,
        "submission_id": claims.submission_id,
        "document_id": claims.document_id,
        "issued_by_user_id": claims.issued_by_user_id,
        "issued_via_api_token_id": claims.issued_via_api_token_id,
    });
    let footer = serde_json::json!({ "kid": DOWNLOAD_KEY_ID }).to_string();
    LocalToken::encrypt(
        &key,
        payload.to_string().as_bytes(),
        Some(footer.as_bytes()),
        Some(DOWNLOAD_IMPLICIT_ASSERTION),
    )
    .expect("expired PASETO token encrypts")
}

fn download_config() -> PasetoDownloadConfig {
    PasetoDownloadConfig {
        active_key_id: DOWNLOAD_KEY_ID.to_owned(),
        keys: vec![PasetoDownloadKey {
            id: DOWNLOAD_KEY_ID.to_owned(),
            secret: SecretString::from(DOWNLOAD_KEY),
        }],
    }
}

fn signed_token(
    version: u8,
    expired: bool,
    workspace_id: Uuid,
    submission_id: Uuid,
    document_id: Uuid,
) -> String {
    signed_token_for_issuer(
        version,
        expired,
        workspace_id,
        submission_id,
        document_id,
        INTEGRATION_API_TOKEN_ID.to_owned(),
    )
}

fn signed_token_for_issuer(
    version: u8,
    expired: bool,
    workspace_id: Uuid,
    submission_id: Uuid,
    document_id: Uuid,
    issued_by: String,
) -> String {
    let key = HmacKey::from_bytes(SIGNING_SECRET, HmacAlgorithm::HS256);
    let mut claims = HeaderAndClaims::with_claims(TestDownloadClaims {
        version,
        workspace_id: workspace_id.to_string(),
        submission_id: submission_id.to_string(),
        document_id: document_id.to_string(),
        issued_by_user_id: issued_by,
        issued_via_api_token_id: INTEGRATION_API_TOKEN_ID.to_owned(),
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
