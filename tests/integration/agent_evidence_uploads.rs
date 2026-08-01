use std::{fs, path::PathBuf};

use axum::http::StatusCode;
use chrono::{TimeZone, Utc};
use proofplane::domain::{AgentEvidenceUploadDeclaration, CoverageWindow};
use secrecy::ExposeSecret;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::support::TestApp;

#[tokio::test]
async fn valid_machine_stream_creates_pending_submission_and_scan_work() {
    let app = upload_app().await;
    let workspace_id = app.workspace_id("workspace");
    let evidence_id = create_evidence(&app, workspace_id).await;
    let content = b"machine-provided evidence";
    let coverage = coverage();
    let issued = app
        .agent_evidence_upload_grant_service()
        .issue(
            &app.agent_connection_context(workspace_id),
            evidence_id.into(),
            coverage,
            declaration(content),
        )
        .await
        .expect("machine upload grant issues");

    let response = app
        .server()
        .put(&format!("/agent-evidence-uploads/{}", issued.grant.id()))
        .add_header(
            "authorization",
            format!("Proofplane-Upload {}", issued.credential.expose_secret()),
        )
        .add_header("content-type", "application/pdf")
        .add_header("content-length", content.len().to_string())
        .bytes(content.as_slice().into())
        .await;

    response.assert_status(StatusCode::CREATED);
    let body: Value = response.json();
    assert_eq!(
        body["submission_id"],
        issued.grant.submission_id().to_string()
    );
    assert_eq!(body["upload_status"], "pending");
    let document_id = body["document_id"]
        .as_str()
        .and_then(|value| Uuid::parse_str(value).ok())
        .expect("document ID is returned");

    let row = app
        .postgres()
        .get()
        .await
        .expect("database opens")
        .query_one(
            r#"
SELECT
    s.evidence_id,
    s.valid_from,
    s.valid_until,
    s.submitted_by_agent_connection_id,
    d.created_by_user_id,
    d.filename,
    d.content_type,
    d.content_length,
    d.object_key,
    d.checksum_sha256,
    d.upload_status,
    g.completed_at,
    g.document_id,
    count(o.id) AS outbox_count,
    max(o.payload::text) AS outbox_payload
FROM evidence_submissions s
JOIN documents d
  ON d.owner_type = 'evidence_submission'
 AND d.owner_id = s.id
JOIN agent_evidence_upload_grants g ON g.submission_id = s.id
LEFT JOIN outbox_messages o
  ON o.aggregate_id = d.id::text
 AND o.event_type = 'document.scan_requested'
WHERE s.id = $1
GROUP BY s.id, d.id, g.id
"#,
            &[&Uuid::from(issued.grant.submission_id())],
        )
        .await
        .expect("machine submission loads");

    assert_eq!(row.get::<_, Uuid>("evidence_id"), evidence_id);
    assert_eq!(
        row.get::<_, chrono::DateTime<Utc>>("valid_from"),
        coverage.valid_from
    );
    assert_eq!(
        row.get::<_, chrono::DateTime<Utc>>("valid_until"),
        coverage.valid_until
    );
    assert_eq!(
        row.get::<_, Uuid>("submitted_by_agent_connection_id"),
        app.api_token_id()
    );
    assert_eq!(row.get::<_, Uuid>("created_by_user_id"), app.user_id());
    assert_eq!(row.get::<_, String>("filename"), "access-review.pdf");
    assert_eq!(row.get::<_, String>("content_type"), "application/pdf");
    assert_eq!(row.get::<_, i64>("content_length"), content.len() as i64);
    assert_eq!(row.get::<_, String>("checksum_sha256"), sha256(content));
    assert_eq!(row.get::<_, String>("upload_status"), "pending");
    assert!(row
        .get::<_, Option<chrono::DateTime<Utc>>>("completed_at")
        .is_some());
    assert_eq!(row.get::<_, Option<Uuid>>("document_id"), Some(document_id));
    assert_eq!(row.get::<_, i64>("outbox_count"), 1);
    let outbox_payload: Value = serde_json::from_str(
        &row.get::<_, Option<String>>("outbox_payload")
            .expect("scan outbox payload exists"),
    )
    .expect("scan outbox payload is JSON");
    assert_eq!(
        outbox_payload["evidence_submission_id"],
        issued.grant.submission_id().to_string()
    );
    assert_eq!(
        outbox_payload["object_key"],
        row.get::<_, String>("object_key")
    );
    let completed = app
        .postgres()
        .agent_evidence_upload_grants()
        .get(issued.grant.id(), issued.grant.workspace_id())
        .await
        .expect("completed grant loads")
        .expect("completed grant exists");
    assert_eq!(completed.document_id().map(Uuid::from), Some(document_id));
    assert!(completed.completed_at().is_some());
}

#[tokio::test]
async fn unavailable_machine_authority_always_returns_the_concealed_response() {
    let app = upload_app().await;
    let workspace_id = app.workspace_id("workspace");
    let evidence_id = create_evidence(&app, workspace_id).await;
    let content = b"machine-provided evidence";
    let issued = app
        .agent_evidence_upload_grant_service()
        .issue(
            &app.agent_connection_context(workspace_id),
            evidence_id.into(),
            coverage(),
            declaration(content),
        )
        .await
        .expect("machine upload grant issues");
    let path = format!("/agent-evidence-uploads/{}", issued.grant.id());
    let token = issued.credential.expose_secret();

    let missing = app
        .server()
        .put(&path)
        .add_header("content-type", "application/pdf")
        .add_header("content-length", content.len().to_string())
        .bytes(content.as_slice().into())
        .await;
    let wrong_scheme = app
        .server()
        .put(&path)
        .add_header("authorization", format!("Bearer {token}"))
        .add_header("content-type", "application/pdf")
        .add_header("content-length", content.len().to_string())
        .bytes(content.as_slice().into())
        .await;
    let wrong_id = app
        .server()
        .put(&format!("/agent-evidence-uploads/{}", Uuid::new_v4()))
        .add_header("authorization", format!("Proofplane-Upload {token}"))
        .add_header("content-type", "application/pdf")
        .add_header("content-length", content.len().to_string())
        .bytes(content.as_slice().into())
        .await;
    let malformed_id = app
        .server()
        .put("/agent-evidence-uploads/not-a-uuid")
        .add_header("authorization", format!("Proofplane-Upload {token}"))
        .add_header("content-type", "application/pdf")
        .add_header("content-length", content.len().to_string())
        .bytes(content.as_slice().into())
        .await;
    let tampered = app
        .server()
        .put(&path)
        .add_header(
            "authorization",
            format!("Proofplane-Upload {}", tamper(token)),
        )
        .add_header("content-type", "application/pdf")
        .add_header("content-length", content.len().to_string())
        .bytes(content.as_slice().into())
        .await;
    app.postgres()
        .get()
        .await
        .expect("database opens")
        .execute(
            "UPDATE agent_evidence_upload_grants SET issued_at = now() - interval '10 minutes', expires_at = now() - interval '5 minutes' WHERE id = $1",
            &[&Uuid::from(issued.grant.id())],
        )
        .await
        .expect("grant expires");
    let expired = app
        .server()
        .put(&path)
        .add_header("authorization", format!("Proofplane-Upload {token}"))
        .add_header("content-type", "application/pdf")
        .add_header("content-length", content.len().to_string())
        .bytes(content.as_slice().into())
        .await;

    let expected = missing.text();
    for response in [
        missing,
        wrong_scheme,
        wrong_id,
        malformed_id,
        tampered,
        expired,
    ] {
        response.assert_status_not_found();
        assert_eq!(response.text(), expected);
    }
    assert_upload_incomplete(&app, issued.grant.submission_id().into()).await;
    assert!(files_under(app.object_storage_root()).is_empty());
}

#[tokio::test]
async fn metadata_mismatches_do_not_complete_the_grant_and_a_valid_retry_succeeds() {
    let app = upload_app().await;
    let workspace_id = app.workspace_id("workspace");
    let evidence_id = create_evidence(&app, workspace_id).await;
    let content = b"machine-provided evidence";
    let issued = app
        .agent_evidence_upload_grant_service()
        .issue(
            &app.agent_connection_context(workspace_id),
            evidence_id.into(),
            coverage(),
            declaration(content),
        )
        .await
        .expect("machine upload grant issues");
    let path = format!("/agent-evidence-uploads/{}", issued.grant.id());
    let authorization = format!("Proofplane-Upload {}", issued.credential.expose_secret());

    let wrong_type = app
        .server()
        .put(&path)
        .add_header("authorization", &authorization)
        .add_header("content-type", "text/plain")
        .add_header("content-length", content.len().to_string())
        .bytes(content.as_slice().into())
        .await;
    wrong_type.assert_status_bad_request();

    let wrong_declared_length = app
        .server()
        .put(&path)
        .add_header("authorization", &authorization)
        .add_header("content-type", "application/pdf")
        .add_header("content-length", (content.len() - 1).to_string())
        .bytes(content.as_slice().into())
        .await;
    wrong_declared_length.assert_status_bad_request();

    let short_body = &content[..content.len() - 1];
    let actual_length_mismatch = app
        .server()
        .put(&path)
        .add_header("authorization", &authorization)
        .add_header("content-type", "application/pdf")
        .add_header("content-length", content.len().to_string())
        .bytes(short_body.to_vec().into())
        .await;
    actual_length_mismatch.assert_status_bad_request();

    let wrong_checksum = vec![b'x'; content.len()];
    let checksum_mismatch = app
        .server()
        .put(&path)
        .add_header("authorization", &authorization)
        .add_header("content-type", "application/pdf")
        .add_header("content-length", wrong_checksum.len().to_string())
        .bytes(wrong_checksum.into())
        .await;
    checksum_mismatch.assert_status_bad_request();

    assert_upload_incomplete(&app, issued.grant.submission_id().into()).await;
    assert!(files_under(app.object_storage_root()).is_empty());

    let retry = app
        .server()
        .put(&path)
        .add_header("authorization", authorization)
        .add_header("content-type", "application/pdf")
        .add_header("content-length", content.len().to_string())
        .bytes(content.as_slice().into())
        .await;
    retry.assert_status(StatusCode::CREATED);
}

#[tokio::test]
async fn configured_limit_rejects_declared_upload_without_durable_or_staged_state() {
    let app = TestApp::builder()
        .without_default_auth()
        .with_max_document_bytes(4)
        .workspace("workspace", "Machine upload workspace")
        .with_default_membership()
        .build()
        .await;
    let workspace_id = app.workspace_id("workspace");
    let evidence_id = create_evidence(&app, workspace_id).await;
    let content = b"large";
    let issued = app
        .agent_evidence_upload_grant_service()
        .issue(
            &app.agent_connection_context(workspace_id),
            evidence_id.into(),
            coverage(),
            declaration(content),
        )
        .await
        .expect("machine upload grant issues");

    let response = upload_request(&app, &issued, content).await;

    response.assert_status(StatusCode::PAYLOAD_TOO_LARGE);
    assert_upload_incomplete(&app, issued.grant.submission_id().into()).await;

    let permitted = b"tiny";
    let permitted_grant = app
        .agent_evidence_upload_grant_service()
        .issue(
            &app.agent_connection_context(workspace_id),
            evidence_id.into(),
            coverage(),
            declaration(permitted),
        )
        .await
        .expect("within-limit machine upload grant issues");
    let streamed_oversize = app
        .server()
        .put(&format!(
            "/agent-evidence-uploads/{}",
            permitted_grant.grant.id()
        ))
        .add_header(
            "authorization",
            format!(
                "Proofplane-Upload {}",
                permitted_grant.credential.expose_secret()
            ),
        )
        .add_header("content-type", "application/pdf")
        .add_header("content-length", permitted.len().to_string())
        .bytes(content.as_slice().into())
        .await;
    streamed_oversize.assert_status(StatusCode::PAYLOAD_TOO_LARGE);
    assert_upload_incomplete(&app, permitted_grant.grant.submission_id().into()).await;
    assert!(files_under(app.object_storage_root()).is_empty());
}

#[tokio::test]
async fn database_failure_rolls_back_completion_and_deletes_quarantine_object() {
    let app = upload_app().await;
    let workspace_id = app.workspace_id("workspace");
    let evidence_id = create_evidence(&app, workspace_id).await;
    let content = b"machine-provided evidence";
    let issued = app
        .agent_evidence_upload_grant_service()
        .issue(
            &app.agent_connection_context(workspace_id),
            evidence_id.into(),
            coverage(),
            declaration(content),
        )
        .await
        .expect("machine upload grant issues");
    app.postgres()
        .get()
        .await
        .expect("database opens")
        .batch_execute(
            r#"
CREATE FUNCTION fail_machine_scan_outbox() RETURNS trigger AS $$
BEGIN
    IF NEW.event_type = 'document.scan_requested' THEN
        RAISE EXCEPTION 'injected scan outbox failure';
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;
CREATE TRIGGER fail_machine_scan_outbox
BEFORE INSERT ON outbox_messages
FOR EACH ROW EXECUTE FUNCTION fail_machine_scan_outbox();
"#,
        )
        .await
        .expect("failure trigger installs");

    let response = upload_request(&app, &issued, content).await;

    response.assert_status_internal_server_error();
    assert_upload_incomplete(&app, issued.grant.submission_id().into()).await;
    assert!(files_under(app.object_storage_root()).is_empty());
}

#[tokio::test]
async fn storage_failure_returns_stable_error_without_completing_the_grant() {
    let app = upload_app().await;
    let workspace_id = app.workspace_id("workspace");
    let evidence_id = create_evidence(&app, workspace_id).await;
    let content = b"machine-provided evidence";
    let issued = app
        .agent_evidence_upload_grant_service()
        .issue(
            &app.agent_connection_context(workspace_id),
            evidence_id.into(),
            coverage(),
            declaration(content),
        )
        .await
        .expect("machine upload grant issues");
    fs::write(app.object_storage_root().join("objects"), b"blocked")
        .expect("object directory is blocked by a file");

    let response = upload_request(&app, &issued, content).await;

    response.assert_status_internal_server_error();
    assert_eq!(response.json::<Value>()["error"]["code"], "internal_error");
    assert_upload_incomplete(&app, issued.grant.submission_id().into()).await;
}

async fn upload_app() -> TestApp {
    TestApp::builder()
        .without_default_auth()
        .workspace("workspace", "Machine upload workspace")
        .with_default_membership()
        .build()
        .await
}

async fn create_evidence(app: &TestApp, workspace_id: Uuid) -> Uuid {
    app.create_evidence(
        workspace_id,
        &json!({
            "title": "Machine evidence",
            "description": "Collect machine evidence.",
            "collection_instructions": "Upload machine evidence.",
            "status": "active",
        }),
    )
    .await["id"]
        .as_str()
        .and_then(|id| Uuid::parse_str(id).ok())
        .expect("evidence id")
}

fn coverage() -> CoverageWindow {
    CoverageWindow::new(
        Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
        Utc.with_ymd_and_hms(2026, 3, 31, 23, 59, 59).unwrap(),
    )
    .unwrap()
}

fn declaration(content: &[u8]) -> AgentEvidenceUploadDeclaration {
    AgentEvidenceUploadDeclaration::new(
        "access-review.pdf".to_owned(),
        "application/pdf".to_owned(),
        content.len() as u64,
        Some(sha256(content)),
        25 * 1024 * 1024,
    )
    .into_result()
    .unwrap()
}

fn sha256(content: &[u8]) -> String {
    hex::encode(Sha256::digest(content))
}

fn tamper(token: &str) -> String {
    let mut bytes = token.as_bytes().to_vec();
    let index = bytes.len() / 2;
    bytes[index] = if bytes[index] == b'A' { b'B' } else { b'A' };
    String::from_utf8(bytes).expect("token remains UTF-8")
}

async fn upload_request(
    app: &TestApp,
    issued: &proofplane::services::agent_evidence_upload_grants::IssuedAgentEvidenceUploadGrant,
    content: &[u8],
) -> axum_test::TestResponse {
    app.server()
        .put(&format!("/agent-evidence-uploads/{}", issued.grant.id()))
        .add_header(
            "authorization",
            format!("Proofplane-Upload {}", issued.credential.expose_secret()),
        )
        .add_header("content-type", "application/pdf")
        .add_header("content-length", content.len().to_string())
        .bytes(bytes::Bytes::copy_from_slice(content))
        .await
}

async fn assert_upload_incomplete(app: &TestApp, submission_id: Uuid) {
    let row = app
        .postgres()
        .get()
        .await
        .expect("database opens")
        .query_one(
            r#"
SELECT
    count(s.id) AS submission_count,
    g.completed_at,
    g.document_id
FROM agent_evidence_upload_grants g
LEFT JOIN evidence_submissions s ON s.id = g.submission_id
WHERE g.submission_id = $1
GROUP BY g.id
"#,
            &[&submission_id],
        )
        .await
        .expect("grant state loads");
    assert_eq!(row.get::<_, i64>("submission_count"), 0);
    assert!(row
        .get::<_, Option<chrono::DateTime<Utc>>>("completed_at")
        .is_none());
    assert!(row.get::<_, Option<Uuid>>("document_id").is_none());
}

fn files_under(root: &std::path::Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(root) else {
        return Vec::new();
    };
    entries
        .filter_map(Result::ok)
        .flat_map(|entry| {
            let path = entry.path();
            if path.is_dir() {
                files_under(&path)
            } else {
                vec![path]
            }
        })
        .collect()
}
