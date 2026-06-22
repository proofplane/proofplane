use axum::http::StatusCode;
use axum_test::multipart::{MultipartForm, Part};
use bytes::Bytes;
use chrono::{Duration, SecondsFormat, Utc};
use proofplane::{pubsub::MESSAGE_BUS_TOPIC, worker::ATTACHMENT_SCAN_REQUESTED};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::support::{
    attachment_form, attachment_form_with_digest, content_digest_header, crc32c_base64, file_part,
    set_submission_received_at, upload_attachment, TestApp,
};

#[tokio::test]
async fn create_returns_the_submission() {
    let app = TestApp::builder()
        .workspace("workspace", "Submission create workspace")
        .with_default_membership()
        .build()
        .await;
    let workspace_id = app.workspace_id("workspace");
    let request = app
        .create_evidence_request(workspace_id, &evidence_request("Submission target"))
        .await;
    let evidence_request_id = created_id(&request);
    let body = evidence_submission();

    let response = app
        .post(&collection_path(workspace_id, evidence_request_id))
        .json(&body)
        .await;

    response.assert_status_ok();
    let created: Value = response.json();
    assert_uuid(&created["id"]);
    assert_eq!(
        created["evidence_request_id"],
        evidence_request_id.to_string()
    );
    assert_eq!(
        created["submitted_by"],
        serde_json::json!({
            "api_token_id": app.api_token_id().to_string(),
            "user_id": app.user_id().to_string(),
        })
    );
    assert_submission_matches(&created, &body);
    assert_timestamp(&created["received_at"]);
}

#[tokio::test]
async fn submission_context_has_compact_and_direct_visibility() {
    let app = TestApp::builder()
        .workspace("workspace", "Submission context workspace")
        .with_default_membership()
        .build()
        .await;
    let workspace_id = app.workspace_id("workspace");
    let request = app
        .create_evidence_request(workspace_id, &evidence_request("Context target"))
        .await;
    let evidence_request_id = created_id(&request);
    let body = json!({
        "coverage_start_at": "2026-01-01T00:00:00Z",
        "coverage_end_at": "2026-03-31T23:59:59Z",
        "source_system": "okta",
        "collection_method": "api_export",
        "summary": "  Quarterly access review  ",
        "description": "  Reviewer decisions and exceptions.  "
    });

    let created = app
        .post(&collection_path(workspace_id, evidence_request_id))
        .json(&body)
        .await
        .json::<Value>();
    let submission_id = created_id(&created);

    assert_eq!(created["summary"], "Quarterly access review");
    assert!(created.get("description").is_none());

    let direct = app
        .get(&item_path(workspace_id, submission_id))
        .await
        .json::<Value>();
    assert_eq!(direct["submission"]["summary"], "Quarterly access review");
    assert_eq!(
        direct["submission"]["description"],
        "Reviewer decisions and exceptions."
    );

    let latest = app
        .get(&latest_path(workspace_id, evidence_request_id))
        .await
        .json::<Value>();
    assert_eq!(latest["submission"]["summary"], "Quarterly access review");
    assert!(latest["submission"].get("description").is_none());
}

#[tokio::test]
async fn omitted_and_null_submission_context_is_absent_from_responses() {
    let app = TestApp::builder()
        .workspace("workspace", "Absent submission context workspace")
        .with_default_membership()
        .build()
        .await;
    let workspace_id = app.workspace_id("workspace");
    let request = app
        .create_evidence_request(workspace_id, &evidence_request("Absent context target"))
        .await;
    let evidence_request_id = created_id(&request);
    let mut body = evidence_submission();
    body["summary"] = Value::Null;

    let created = app
        .post(&collection_path(workspace_id, evidence_request_id))
        .json(&body)
        .await
        .json::<Value>();
    assert!(created.get("summary").is_none());
    assert!(created.get("description").is_none());

    let direct = app
        .get(&item_path(workspace_id, created_id(&created)))
        .await
        .json::<Value>();
    assert!(direct["submission"].get("summary").is_none());
    assert!(direct["submission"].get("description").is_none());
}

#[tokio::test]
async fn invalid_submission_context_is_rejected_without_persistence() {
    let app = TestApp::builder()
        .workspace("workspace", "Invalid submission context workspace")
        .with_default_membership()
        .build()
        .await;
    let workspace_id = app.workspace_id("workspace");
    let request = app
        .create_evidence_request(workspace_id, &evidence_request("Invalid context target"))
        .await;
    let evidence_request_id = created_id(&request);

    for (field, value) in [
        ("summary", " \t ".to_owned()),
        ("summary", "é".repeat(501)),
        ("description", "é".repeat(4_001)),
    ] {
        let mut body = evidence_submission();
        body[field] = Value::String(value);
        app.post(&collection_path(workspace_id, evidence_request_id))
            .json(&body)
            .await
            .assert_status_bad_request();
    }

    let client = app.postgres().get().await.expect("connection opens");
    let count: i64 = client
        .query_one(
            "SELECT count(*) FROM evidence_submissions WHERE evidence_request_id = $1",
            &[&evidence_request_id],
        )
        .await
        .expect("submission count loads")
        .get(0);
    assert_eq!(count, 0);
}

#[tokio::test]
async fn get_returns_submission_detail_with_empty_attachments() {
    let app = TestApp::builder()
        .workspace("workspace", "Submission detail workspace")
        .with_default_membership()
        .build()
        .await;
    let workspace_id = app.workspace_id("workspace");
    let request = app
        .create_evidence_request(workspace_id, &evidence_request("Detail target"))
        .await;
    let evidence_request_id = created_id(&request);
    let created = app
        .post(&collection_path(workspace_id, evidence_request_id))
        .json(&evidence_submission())
        .await
        .json::<Value>();
    let submission_id = created_id(&created);

    let response = app.get(&item_path(workspace_id, submission_id)).await;

    response.assert_status_ok();
    let detail: Value = response.json();
    assert_eq!(detail["submission"], created);
    assert_eq!(detail["attachments"], json!([]));
}

#[tokio::test]
async fn latest_returns_newest_submission_with_attachments() {
    let app = TestApp::builder()
        .workspace("workspace", "Latest submission workspace")
        .with_default_membership()
        .build()
        .await;
    let workspace_id = app.workspace_id("workspace");
    let request = app
        .create_evidence_request(workspace_id, &evidence_request("Latest target"))
        .await;
    let evidence_request_id = created_id(&request);
    let first = app
        .post(&collection_path(workspace_id, evidence_request_id))
        .json(&evidence_submission())
        .await
        .json::<Value>();
    let latest = app
        .post(&collection_path(workspace_id, evidence_request_id))
        .json(&json!({
            "coverage_start_at": "2026-04-01T00:00:00Z",
            "coverage_end_at": "2026-06-30T23:59:59Z",
            "source_system": "github",
            "collection_method": "api_export"
        }))
        .await
        .json::<Value>();
    let first_id = created_id(&first);
    let latest_id = created_id(&latest);
    set_submission_received_at(app.postgres(), first_id, Utc::now() - Duration::days(1)).await;
    set_submission_received_at(app.postgres(), latest_id, Utc::now()).await;

    let z_attachment =
        upload_attachment(&app, workspace_id, latest_id, "z-last.txt", b"z-last.txt").await;
    let a_attachment =
        upload_attachment(&app, workspace_id, latest_id, "a-first.txt", b"a-first.txt").await;

    let response = app
        .get(&latest_path(workspace_id, evidence_request_id))
        .await;

    response.assert_status_ok();
    let detail = response.json::<Value>();
    assert_eq!(detail["submission"]["id"], latest_id.to_string());
    assert_eq!(detail["attachments"], json!([a_attachment, z_attachment]));
}

#[tokio::test]
async fn latest_returns_not_found_for_inaccessible_or_never_submitted_requests() {
    let app = TestApp::builder()
        .workspace("workspace", "Latest owner workspace")
        .with_default_membership()
        .workspace("other_workspace", "Latest other workspace")
        .with_default_membership()
        .workspace("ungranted_workspace", "Latest ungranted workspace")
        .without_membership()
        .build()
        .await;
    let workspace_id = app.workspace_id("workspace");
    let other_workspace_id = app.workspace_id("other_workspace");
    let ungranted_workspace_id = app.workspace_id("ungranted_workspace");
    let request = app
        .create_evidence_request(workspace_id, &evidence_request("Never submitted target"))
        .await;
    let evidence_request_id = created_id(&request);

    app.get(&latest_path(workspace_id, Uuid::new_v4()))
        .await
        .assert_status_not_found();
    app.get(&latest_path(workspace_id, evidence_request_id))
        .await
        .assert_status_not_found();
    app.get(&latest_path(other_workspace_id, evidence_request_id))
        .await
        .assert_status_not_found();
    app.server()
        .get(&latest_path(ungranted_workspace_id, evidence_request_id))
        .await
        .assert_status_not_found();
}

#[tokio::test]
async fn upload_attachment_returns_accepted_and_get_includes_attachment() {
    let app = TestApp::builder()
        .workspace("workspace", "Attachment upload workspace")
        .with_default_membership()
        .build()
        .await;
    let workspace_id = app.workspace_id("workspace");
    let submission_id = create_submission(&app, workspace_id).await;
    let bytes = br#"{"ok":true}"#;
    let request_id = Uuid::new_v4();

    let response = app
        .post(&attachment_collection_path(workspace_id, submission_id))
        .add_header("x-request-id", request_id.to_string())
        .multipart(attachment_form(
            bytes,
            "Quarterly evidence (final).json",
            "application/json",
            None,
        ))
        .await;

    assert_eq!(response.status_code(), StatusCode::ACCEPTED);
    let body = response.json::<Value>();
    assert_uuid(&body["attachment"]["id"]);
    assert_eq!(
        body["attachment"]["evidence_submission_id"],
        submission_id.to_string()
    );
    assert_eq!(
        body["attachment"]["filename"],
        "Quarterly evidence (final).json"
    );
    assert_eq!(body["attachment"]["content_type"], "application/json");
    assert_eq!(body["attachment"]["content_length"], bytes.len() as i64);
    assert_eq!(
        body["attachment"]["checksum_sha256"],
        hex::encode(Sha256::digest(bytes))
    );
    assert_eq!(body["attachment"]["checksum_crc32c"], crc32c_base64(bytes));
    assert_eq!(body["attachment"]["upload_status"], "pending");
    let attachment_id = Uuid::parse_str(
        body["attachment"]["id"]
            .as_str()
            .expect("attachment ID is a string"),
    )
    .expect("attachment ID is a UUID");
    let client = app.postgres().get().await.expect("connection opens");
    let object_key: String = client
        .query_one(
            "SELECT object_key FROM evidence_attachments WHERE id = $1",
            &[&attachment_id],
        )
        .await
        .expect("attachment object key loads")
        .get("object_key");
    assert!(object_key.starts_with(&format!(
        "workspaces/{workspace_id}/quarantine/evidence-submissions/{submission_id}/attachments/"
    )));
    let object_path = app.object_storage_root().join("objects").join(&object_key);
    assert_eq!(
        std::fs::read(object_path).expect("quarantine object exists"),
        bytes
    );

    let detail = app
        .get(&item_path(workspace_id, submission_id))
        .await
        .json::<Value>();
    assert_eq!(detail["attachments"], json!([body["attachment"]]));

    let outbox_messages = app
        .postgres()
        .list_due_outbox_messages(Utc::now() + Duration::seconds(1), 10)
        .await
        .expect("outbox messages list");
    assert_eq!(outbox_messages.len(), 1);
    let scan_request = &outbox_messages[0];
    assert_eq!(scan_request.topic.as_str(), MESSAGE_BUS_TOPIC);
    assert_eq!(scan_request.event_type, ATTACHMENT_SCAN_REQUESTED);
    assert_eq!(scan_request.aggregate_type, "evidence_attachment");
    assert_eq!(
        scan_request.aggregate_id,
        body["attachment"]["id"].as_str().expect("id is a string")
    );
    assert_eq!(
        scan_request.payload,
        json!({
            "evidence_submission_id": submission_id.to_string(),
            "object_key": object_key,
        })
    );
    assert_eq!(scan_request.request_id, Some(request_id));
}

#[tokio::test]
async fn upload_attachment_returns_not_found_for_missing_cross_workspace_or_ungranted_submission() {
    let app = TestApp::builder()
        .workspace("workspace", "Attachment owner workspace")
        .with_default_membership()
        .workspace("other_workspace", "Attachment other workspace")
        .with_default_membership()
        .workspace("ungranted_workspace", "Attachment ungranted workspace")
        .without_membership()
        .build()
        .await;
    let workspace_id = app.workspace_id("workspace");
    let other_workspace_id = app.workspace_id("other_workspace");
    let ungranted_workspace_id = app.workspace_id("ungranted_workspace");
    let submission_id = create_submission(&app, workspace_id).await;
    let bytes = b"attachment";

    app.post(&attachment_collection_path(workspace_id, Uuid::new_v4()))
        .multipart(attachment_form(bytes, "artifact.txt", "text/plain", None))
        .await
        .assert_status_not_found();
    app.post(&attachment_collection_path(
        other_workspace_id,
        submission_id,
    ))
    .multipart(attachment_form(bytes, "artifact.txt", "text/plain", None))
    .await
    .assert_status_not_found();
    app.server()
        .post(&attachment_collection_path(
            ungranted_workspace_id,
            submission_id,
        ))
        .multipart(attachment_form(bytes, "artifact.txt", "text/plain", None))
        .await
        .assert_status_not_found();

    assert!(object_files(app.object_storage_root().join("objects")).is_empty());
}

#[tokio::test]
async fn upload_attachment_maps_invalid_multipart_to_bad_request() {
    let app = TestApp::builder()
        .workspace("workspace", "Attachment validation workspace")
        .with_default_membership()
        .build()
        .await;
    let workspace_id = app.workspace_id("workspace");
    let submission_id = create_submission(&app, workspace_id).await;
    let path = attachment_collection_path(workspace_id, submission_id);
    let bytes = b"attachment";

    for form in [
        MultipartForm::new().add_part("checksum_crc32c", Part::text(crc32c_base64(bytes))),
        MultipartForm::new().add_part(
            "file",
            Part::bytes(bytes.as_slice())
                .file_name("artifact.txt")
                .mime_type("text/plain"),
        ),
        attachment_form_with_digest(bytes, "artifact.txt", "text/plain", "crc32c=:not base64:"),
        attachment_form_with_digest(bytes, "artifact.txt", "text/plain", "sha-256=:abcd:"),
        attachment_form_with_digest(
            bytes,
            "artifact.txt",
            "text/plain",
            &content_digest_header(b"different"),
        ),
    ] {
        let response = app.post(&path).multipart(form).await;
        assert_eq!(response.status_code(), StatusCode::BAD_REQUEST);
        assert_eq!(response.json::<Value>()["error"]["code"], "bad_request");
    }

    assert!(object_files(app.object_storage_root().join("objects")).is_empty());
}

#[tokio::test]
async fn upload_attachment_rejects_duplicate_file_and_cleans_staged_object() {
    let app = TestApp::builder()
        .workspace("workspace", "Attachment duplicate workspace")
        .with_default_membership()
        .build()
        .await;
    let workspace_id = app.workspace_id("workspace");
    let submission_id = create_submission(&app, workspace_id).await;
    let uploaded_bytes = b"this was an uploaded attachment";
    let skipped_bytes = b"this should be skipped";
    let form = MultipartForm::new()
        .add_part(
            "file",
            file_part(
                uploaded_bytes,
                "artifact.txt",
                "text/plain",
                &content_digest_header(uploaded_bytes),
            ),
        )
        .add_part(
            "file",
            file_part(
                skipped_bytes,
                "artifact-copy.txt",
                "text/plain",
                &content_digest_header(skipped_bytes),
            ),
        );

    let response = app
        .post(&attachment_collection_path(workspace_id, submission_id))
        .multipart(form)
        .await;

    assert_eq!(response.status_code(), StatusCode::ACCEPTED);

    let files = object_files(app.object_storage_root().join("objects"));
    assert_eq!(files.len(), 1);
    assert_eq!(
        files[0].file_name().and_then(|name| name.to_str()),
        Some("artifact.txt")
    );
    assert_eq!(
        std::fs::read(&files[0]).expect("stored object is readable"),
        uploaded_bytes
    );
}

#[tokio::test]
async fn upload_attachment_rejects_unsafe_filenames() {
    let app = TestApp::builder()
        .workspace("workspace", "Attachment filename workspace")
        .with_default_membership()
        .build()
        .await;
    let workspace_id = app.workspace_id("workspace");
    let submission_id = create_submission(&app, workspace_id).await;
    let path = attachment_collection_path(workspace_id, submission_id);
    let bytes = b"attachment";

    let missing = app
        .post(&path)
        .multipart(
            MultipartForm::new().add_part(
                "file",
                Part::bytes(bytes.as_slice())
                    .mime_type("text/plain")
                    .add_header("content-digest", content_digest_header(bytes)),
            ),
        )
        .await;
    assert_eq!(missing.status_code(), StatusCode::BAD_REQUEST);

    for (filename, expected_detail) in [
        (" ", "attachment filename must not be empty"),
        (
            "path/file.txt",
            "attachment filename contains unsupported characters",
        ),
        (
            r"path\file.txt",
            "attachment filename contains unsupported characters",
        ),
        (
            "résumé.txt",
            "attachment filename contains unsupported characters",
        ),
        (".", "attachment filename must not be . or .."),
        ("..", "attachment filename must not be . or .."),
    ] {
        let response = app
            .post(&path)
            .multipart(attachment_form(bytes, filename, "text/plain", None))
            .await;
        assert_eq!(
            response.status_code(),
            StatusCode::BAD_REQUEST,
            "{filename:?}"
        );
        let body = response.json::<Value>();
        assert_eq!(body["error"]["code"], "bad_request", "{filename:?}");
        assert!(
            body["error"]["details"]
                .as_array()
                .expect("details is an array")
                .iter()
                .any(|detail| detail == expected_detail),
            "{filename:?}: {body}"
        );
    }

    for filename_parameter in [r#"file\"name.txt"#, "file\tname.txt"] {
        let boundary = "proofplane-filename-validation";
        let body = format!(
            "--{boundary}\r\n\
             Content-Disposition: form-data; name=\"file\"; filename=\"{filename_parameter}\"\r\n\
             Content-Type: text/plain\r\n\
             Content-Digest: {}\r\n\
             \r\n\
             attachment\r\n\
             --{boundary}--\r\n",
            content_digest_header(bytes)
        );
        let response = app
            .post(&path)
            .add_header(
                "content-type",
                format!("multipart/form-data; boundary={boundary}"),
            )
            .bytes(Bytes::from(body))
            .await;
        assert_eq!(
            response.status_code(),
            StatusCode::BAD_REQUEST,
            "{filename_parameter:?}"
        );
        assert!(response.json::<Value>()["error"]["details"]
            .as_array()
            .expect("details is an array")
            .iter()
            .any(|detail| detail == "attachment filename contains unsupported characters"));
    }

    let overlong = app
        .post(&path)
        .multipart(attachment_form(bytes, &"a".repeat(256), "text/plain", None))
        .await;
    assert_eq!(overlong.status_code(), StatusCode::BAD_REQUEST);
    assert!(overlong.json::<Value>()["error"]["details"]
        .as_array()
        .expect("details is an array")
        .iter()
        .any(|detail| detail == "attachment filename must be at most 255 bytes"));

    assert!(object_files(app.object_storage_root().join("objects")).is_empty());
}

#[tokio::test]
async fn upload_attachment_over_limit_returns_payload_too_large() {
    let app = TestApp::builder()
        .with_max_attachment_bytes(128)
        .workspace("workspace", "Attachment limit workspace")
        .with_default_membership()
        .build()
        .await;
    let workspace_id = app.workspace_id("workspace");
    let submission_id = create_submission(&app, workspace_id).await;
    let bytes = vec![b'a'; 256];

    let response = app
        .post(&attachment_collection_path(workspace_id, submission_id))
        .multipart(attachment_form(&bytes, "artifact.txt", "text/plain", None))
        .await;

    assert_eq!(response.status_code(), StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn create_maps_validation_errors_to_bad_request() {
    let app = TestApp::builder()
        .workspace("workspace", "Invalid submission workspace")
        .with_default_membership()
        .build()
        .await;
    let workspace_id = app.workspace_id("workspace");
    let request = app
        .create_evidence_request(workspace_id, &evidence_request("Invalid target"))
        .await;
    let evidence_request_id = created_id(&request);

    let response = app
        .post(&collection_path(workspace_id, evidence_request_id))
        .json(&json!({
            "coverage_start_at": "2026-04-01T00:00:00Z",
            "coverage_end_at": "2026-03-31T23:59:59Z",
            "source_system": " ",
            "collection_method": ""
        }))
        .await;

    assert_eq!(response.status_code(), StatusCode::BAD_REQUEST);
    let body = response.json::<Value>();
    assert_eq!(body["error"]["code"], "bad_request");
    let details = body["error"]["details"]
        .as_array()
        .expect("details is an array");
    assert!(details
        .iter()
        .any(|detail| detail == "source_system must not be empty"));
    assert!(details
        .iter()
        .any(|detail| detail == "collection_method must not be empty"));
    assert!(details.iter().any(
        |detail| detail == "coverage_end_at must be greater than or equal to coverage_start_at"
    ));
}

#[tokio::test]
async fn create_returns_not_found_for_missing_or_cross_workspace_requests() {
    let app = TestApp::builder()
        .workspace("workspace", "Submission owner workspace")
        .with_default_membership()
        .workspace("other_workspace", "Submission other workspace")
        .with_default_membership()
        .build()
        .await;
    let workspace_id = app.workspace_id("workspace");
    let other_workspace_id = app.workspace_id("other_workspace");
    let request = app
        .create_evidence_request(workspace_id, &evidence_request("Owner target"))
        .await;
    let evidence_request_id = created_id(&request);

    app.post(&collection_path(workspace_id, Uuid::new_v4()))
        .json(&evidence_submission())
        .await
        .assert_status_not_found();

    app.post(&collection_path(other_workspace_id, evidence_request_id))
        .json(&evidence_submission())
        .await
        .assert_status_not_found();
}

#[tokio::test]
async fn get_returns_not_found_for_missing_or_cross_workspace_submissions() {
    let app = TestApp::builder()
        .workspace("workspace", "Submission get owner workspace")
        .with_default_membership()
        .workspace("other_workspace", "Submission get other workspace")
        .with_default_membership()
        .build()
        .await;
    let workspace_id = app.workspace_id("workspace");
    let other_workspace_id = app.workspace_id("other_workspace");
    let request = app
        .create_evidence_request(workspace_id, &evidence_request("Get owner target"))
        .await;
    let evidence_request_id = created_id(&request);
    let created = app
        .post(&collection_path(workspace_id, evidence_request_id))
        .json(&evidence_submission())
        .await
        .json::<Value>();
    let submission_id = created_id(&created);

    app.get(&item_path(workspace_id, Uuid::new_v4()))
        .await
        .assert_status_not_found();
    app.get(&item_path(other_workspace_id, submission_id))
        .await
        .assert_status_not_found();
}

#[tokio::test]
async fn ungranted_workspace_returns_not_found_for_submission_routes() {
    let app = TestApp::builder()
        .workspace("granted_workspace", "Granted submission workspace")
        .with_default_membership()
        .workspace("ungranted_workspace", "Ungranted submission workspace")
        .without_membership()
        .build()
        .await;
    let granted_workspace_id = app.workspace_id("granted_workspace");
    let ungranted_workspace_id = app.workspace_id("ungranted_workspace");
    let request = app
        .create_evidence_request(granted_workspace_id, &evidence_request("Protected target"))
        .await;
    let evidence_request_id = created_id(&request);
    let created = app
        .post(&collection_path(granted_workspace_id, evidence_request_id))
        .json(&evidence_submission())
        .await
        .json::<Value>();
    let submission_id = created_id(&created);

    app.server()
        .post(&collection_path(
            ungranted_workspace_id,
            evidence_request_id,
        ))
        .json(&evidence_submission())
        .await
        .assert_status_not_found();
    app.server()
        .get(&item_path(ungranted_workspace_id, submission_id))
        .await
        .assert_status_not_found();
}

#[tokio::test]
async fn unsupported_submission_methods_return_method_not_allowed() {
    let app = TestApp::builder()
        .workspace("workspace", "Unsupported submission method workspace")
        .with_default_membership()
        .build()
        .await;
    let workspace_id = app.workspace_id("workspace");
    let request_id = Uuid::new_v4();
    let submission_id = Uuid::new_v4();

    let create_response = app
        .server()
        .patch(&collection_path(workspace_id, request_id))
        .await;
    assert_method_not_allowed(&create_response.json(), create_response.status_code());

    let get_response = app
        .server()
        .post(&item_path(workspace_id, submission_id))
        .await;
    assert_eq!(get_response.status_code(), StatusCode::METHOD_NOT_ALLOWED);

    let upload_response = app
        .server()
        .get(&attachment_collection_path(workspace_id, submission_id))
        .await;
    assert_eq!(
        upload_response.status_code(),
        StatusCode::METHOD_NOT_ALLOWED
    );
}

async fn create_submission(app: &TestApp, workspace_id: Uuid) -> Uuid {
    let request = app
        .create_evidence_request(workspace_id, &evidence_request("Attachment target"))
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

fn dynamic_due_at() -> String {
    (Utc::now() + Duration::days(7)).to_rfc3339_opts(SecondsFormat::Secs, true)
}

fn evidence_submission() -> Value {
    json!({
        "coverage_start_at": "2026-01-01T00:00:00Z",
        "coverage_end_at": "2026-03-31T23:59:59Z",
        "source_system": "okta",
        "collection_method": "api_export"
    })
}

fn assert_submission_matches(response: &Value, request: &Value) {
    for field in [
        "coverage_start_at",
        "coverage_end_at",
        "source_system",
        "collection_method",
    ] {
        assert_eq!(response[field], request[field], "field {field} differs");
    }
}

fn assert_method_not_allowed(body: &Value, status: StatusCode) {
    assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(body["error"]["code"], "method_not_allowed");
}

fn assert_uuid(value: &Value) {
    Uuid::parse_str(value.as_str().expect("UUID field is a string")).expect("UUID field parses");
}

fn assert_timestamp(value: &Value) {
    assert!(value
        .as_str()
        .is_some_and(|timestamp| timestamp.ends_with('Z')));
}

fn created_id(created: &Value) -> Uuid {
    Uuid::parse_str(created["id"].as_str().expect("created response has an id"))
        .expect("created response id is a UUID")
}

fn collection_path(workspace_id: Uuid, evidence_request_id: Uuid) -> String {
    format!("/workspaces/{workspace_id}/evidence-requests/{evidence_request_id}/submissions")
}

fn latest_path(workspace_id: Uuid, evidence_request_id: Uuid) -> String {
    format!("/workspaces/{workspace_id}/evidence-requests/{evidence_request_id}/submissions/latest")
}

fn item_path(workspace_id: Uuid, submission_id: Uuid) -> String {
    format!("/workspaces/{workspace_id}/evidence-submissions/{submission_id}")
}

fn attachment_collection_path(workspace_id: Uuid, submission_id: Uuid) -> String {
    format!("/workspaces/{workspace_id}/evidence-submissions/{submission_id}/attachments")
}

fn object_files(path: std::path::PathBuf) -> Vec<std::path::PathBuf> {
    let Ok(entries) = std::fs::read_dir(path) else {
        return Vec::new();
    };

    entries
        .flat_map(|entry| {
            let path = entry.expect("object storage entry is readable").path();
            if path.is_dir() {
                object_files(path)
            } else {
                vec![path]
            }
        })
        .collect()
}
