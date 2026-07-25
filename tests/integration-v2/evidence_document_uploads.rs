use axum_test::multipart::{MultipartForm, Part};
use http::StatusCode;
use proofplane::{
    domain::WorkspacePermission,
    routes::request_context::REQUEST_ID_HEADER,
    worker::{DOCUMENT_FINALIZATION_REQUESTED, DOCUMENT_SCAN_REQUESTED},
};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::support::{
    documents::upload_form,
    evidence_documents::{archive_path, download_path, VALID_FROM, VALID_UNTIL},
    harness,
    http::{local_path, request_cookie},
    mcp::McpClient,
    oauth::authorize_agent_connection,
    scenario::ScenarioBuilder,
};

#[tokio::test]
async fn grant_url_redeems_once_and_opens_a_scoped_session() {
    let app = harness::app().await;

    let scenario = ScenarioBuilder::new(&app)
        .with_user("auth0|upload-redeem")
        .with_workspace("auth0|upload-redeem", "Redeem")
        .with_evidence("Redeem", "Quarterly access review")
        .build()
        .await;

    let token = authorize_agent_connection(
        &app,
        "auth0|upload-redeem",
        "Claude",
        &WorkspacePermission::ALL,
    )
    .await;
    let client = McpClient::connect(app.mcp_server(), &token).await;

    let evidence_id = scenario
        .workspace("Redeem")
        .evidence("Quarterly access review")
        .id;

    let grant = client
        .call_tool(
            "manage_evidence_submissions",
            json!({
                "evidence_id": evidence_id,
                "valid_from": VALID_FROM,
                "valid_until": VALID_UNTIL,
            }),
        )
        .await;
    assert_eq!(grant["evidence_id"], evidence_id.to_string());
    assert_eq!(grant["valid_from"], VALID_FROM);
    assert_eq!(grant["valid_until"], VALID_UNTIL);
    assert_eq!(grant["url_secret_type"], "bearer_secret");
    assert_eq!(grant["intended_use"], "human_browser_evidence_upload");

    let grant_path = local_path(grant["url"].as_str().expect("grant url is a string"));
    let redeemed = app.app_server().get(&grant_path).await;
    redeemed.assert_status(StatusCode::SEE_OTHER);
    assert_eq!(redeemed.header("location"), "/evidence-document-uploads");

    let set_cookie_header = redeemed.header("set-cookie");
    let cookie = set_cookie_header.to_str().expect("cookie header is text");
    assert!(cookie.contains("HttpOnly"));
    assert!(cookie.contains("SameSite=Lax"));
    assert!(cookie.contains("Path=/evidence-document-uploads"));
    assert!(cookie.contains("Secure"));

    // The grant is a one-shot bearer secret: replaying it reveals nothing.
    app.app_server()
        .get(&grant_path)
        .await
        .assert_status_not_found();

    app.app_server()
        .get("/evidence-document-uploads")
        .add_header("cookie", request_cookie(cookie))
        .await
        .assert_status_ok();
}

#[tokio::test]
async fn each_uploaded_file_becomes_its_own_submission_with_the_grant_coverage() {
    let app = harness::app().await;

    let scenario = ScenarioBuilder::new(&app)
        .with_user("auth0|upload-per-file")
        .with_workspace("auth0|upload-per-file", "Per File")
        .with_evidence("Per File", "Access review exports")
        .build()
        .await;

    let token = authorize_agent_connection(
        &app,
        "auth0|upload-per-file",
        "Claude",
        &WorkspacePermission::ALL,
    )
    .await;
    let client = McpClient::connect(app.mcp_server(), &token).await;

    let evidence_id = scenario
        .workspace("Per File")
        .evidence("Access review exports")
        .id;

    let grant = client
        .call_tool(
            "manage_evidence_submissions",
            json!({
                "evidence_id": evidence_id,
                "valid_from": VALID_FROM,
                "valid_until": VALID_UNTIL,
            }),
        )
        .await;
    let redeemed = app
        .app_server()
        .get(&local_path(
            grant["url"].as_str().expect("grant URL is a string"),
        ))
        .await;
    redeemed.assert_status(StatusCode::SEE_OTHER);
    let cookie = request_cookie(
        redeemed
            .header("set-cookie")
            .to_str()
            .expect("cookie header is text"),
    );
    let mut first_events = app.pipeline_events().subscribe();
    let mut second_events = app.pipeline_events().subscribe();

    for bytes in [b"first report".as_slice(), b"second report".as_slice()] {
        app.app_server()
            .post("/evidence-document-uploads/files")
            .add_header("cookie", cookie.clone())
            .multipart(upload_form(bytes, "report.txt"))
            .await
            // Make sure we redirect back to the file upload page instead of loading
            // a response from the form upload. Also prevents a refresh of the page
            // from doing another POST request which would create another submission.
            .assert_status(StatusCode::SEE_OTHER);
    }

    let submissions = client
        .call_tool(
            "list_evidence_submissions",
            json!({ "evidence_id": evidence_id }),
        )
        .await["submissions"]
        .as_array()
        .expect("submissions is an array")
        .clone();

    assert_eq!(submissions.len(), 2);
    assert_ne!(
        submissions[0]["submission"]["id"],
        submissions[1]["submission"]["id"]
    );
    for (submission, events) in submissions
        .iter()
        .zip([&mut first_events, &mut second_events])
    {
        assert_eq!(submission["document"]["filename"], "report.txt");
        assert_eq!(
            submission["submission"]["evidence_id"],
            evidence_id.to_string()
        );
        assert_eq!(submission["submission"]["valid_from"], VALID_FROM);
        assert_eq!(submission["submission"]["valid_until"], VALID_UNTIL);
        let document_id = submission["document"]["id"]
            .as_str()
            .expect("document id is a string");
        assert_eq!(
            events
                .await_event(DOCUMENT_SCAN_REQUESTED, document_id)
                .await,
            StatusCode::NO_CONTENT
        );
        assert_eq!(
            events
                .await_event(DOCUMENT_FINALIZATION_REQUESTED, document_id)
                .await,
            StatusCode::NO_CONTENT
        );
    }

    let page = app
        .app_server()
        .get("/evidence-document-uploads")
        .add_header("cookie", cookie)
        .await;
    page.assert_status_ok();

    let html = &page.text();
    let start = html.find("<tbody>").expect("page has a file table") + "<tbody>".len();
    let end = html.find("</tbody>").expect("file table closes");

    let file_rows = html[start..end]
        .split_inclusive("</tr>")
        .map(str::to_owned)
        .collect::<Vec<String>>();

    assert_eq!(
        file_rows,
        submissions
            .iter()
            .map(uploaded_file_row)
            .collect::<Vec<String>>()
    );
}

#[tokio::test]
async fn concurrent_uploads_create_distinct_submissions() {
    let app = harness::app().await;

    let scenario = ScenarioBuilder::new(&app)
        .with_user("auth0|upload-concurrent")
        .with_workspace("auth0|upload-concurrent", "Concurrent")
        .with_evidence("Concurrent", "Concurrent evidence")
        .build()
        .await;

    let token = authorize_agent_connection(
        &app,
        "auth0|upload-concurrent",
        "Claude",
        &WorkspacePermission::ALL,
    )
    .await;
    let client = McpClient::connect(app.mcp_server(), &token).await;

    let evidence_id = scenario
        .workspace("Concurrent")
        .evidence("Concurrent evidence")
        .id;
    let grant = client
        .call_tool(
            "manage_evidence_submissions",
            json!({
                "evidence_id": evidence_id,
                "valid_from": VALID_FROM,
                "valid_until": VALID_UNTIL,
            }),
        )
        .await;
    let redeemed = app
        .app_server()
        .get(&local_path(
            grant["url"].as_str().expect("grant URL is a string"),
        ))
        .await;
    redeemed.assert_status(StatusCode::SEE_OTHER);
    let cookie = request_cookie(
        redeemed
            .header("set-cookie")
            .to_str()
            .expect("cookie header is text"),
    );
    let mut first_events = app.pipeline_events().subscribe();
    let mut second_events = app.pipeline_events().subscribe();

    let first = app
        .app_server()
        .post("/evidence-document-uploads/files")
        .add_header("cookie", cookie.clone())
        .multipart(upload_form(b"one", "one.txt"));
    let second = app
        .app_server()
        .post("/evidence-document-uploads/files")
        .add_header("cookie", cookie)
        .multipart(upload_form(b"two", "two.txt"));
    let (first, second) = tokio::join!(first, second);
    first.assert_status(StatusCode::SEE_OTHER);
    second.assert_status(StatusCode::SEE_OTHER);

    let submissions = client
        .call_tool(
            "list_evidence_submissions",
            json!({ "evidence_id": evidence_id }),
        )
        .await["submissions"]
        .as_array()
        .expect("submissions is an array")
        .clone();

    assert_eq!(submissions.len(), 2);
    assert_ne!(
        submissions[0]["submission"]["id"],
        submissions[1]["submission"]["id"]
    );

    let mut filenames = submissions
        .iter()
        .map(|submission| {
            submission["document"]["filename"]
                .as_str()
                .expect("filename is a string")
        })
        .collect::<Vec<_>>();
    filenames.sort_unstable();
    assert_eq!(filenames, ["one.txt", "two.txt"]);

    for (filename, events) in [
        ("one.txt", &mut first_events),
        ("two.txt", &mut second_events),
    ] {
        let document_id = submissions
            .iter()
            .find(|submission| submission["document"]["filename"] == filename)
            .expect("uploaded filename has a submission")["document"]["id"]
            .as_str()
            .expect("document id is a string");
        assert_eq!(
            events
                .await_event(DOCUMENT_SCAN_REQUESTED, document_id)
                .await,
            StatusCode::NO_CONTENT
        );
        assert_eq!(
            events
                .await_event(DOCUMENT_FINALIZATION_REQUESTED, document_id)
                .await,
            StatusCode::NO_CONTENT
        );
    }

    let settled = client
        .call_tool(
            "list_evidence_submissions",
            json!({ "evidence_id": evidence_id }),
        )
        .await["submissions"]
        .as_array()
        .expect("submissions is an array")
        .clone();
    assert_eq!(settled.len(), 2);
    for submission in settled {
        assert_eq!(submission["document"]["upload_status"], "uploaded");
    }
}

#[tokio::test]
async fn upload_session_rejects_invalid_forms() {
    let app = harness::app().await;

    let scenario = ScenarioBuilder::new(&app)
        .with_user("auth0|upload-invalid-forms")
        .with_workspace("auth0|upload-invalid-forms", "Invalid Forms")
        .with_evidence("Invalid Forms", "Form validation")
        .build()
        .await;

    let token = authorize_agent_connection(
        &app,
        "auth0|upload-invalid-forms",
        "Claude",
        &WorkspacePermission::ALL,
    )
    .await;
    let client = McpClient::connect(app.mcp_server(), &token).await;

    let evidence_id = scenario
        .workspace("Invalid Forms")
        .evidence("Form validation")
        .id;

    // Without a usable session the page is not merely forbidden, it is absent.
    app.app_server()
        .get("/evidence-document-uploads")
        .await
        .assert_status_not_found();
    app.app_server()
        .get("/evidence-document-uploads")
        .add_header("cookie", "proofplane_document_upload_session=not-a-token")
        .await
        .assert_status_not_found();
    app.app_server()
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
        let grant = client
            .call_tool(
                "manage_evidence_submissions",
                json!({
                    "evidence_id": evidence_id,
                    "valid_from": VALID_FROM,
                    "valid_until": VALID_UNTIL,
                }),
            )
            .await;
        let redeemed = app
            .app_server()
            .get(&local_path(
                grant["url"].as_str().expect("grant URL is a string"),
            ))
            .await;
        redeemed.assert_status(StatusCode::SEE_OTHER);
        let cookie = request_cookie(
            redeemed
                .header("set-cookie")
                .to_str()
                .expect("cookie header is text"),
        );
        app.app_server()
            .post("/evidence-document-uploads/files")
            .add_header("cookie", cookie)
            .multipart(form)
            .await
            .assert_status_bad_request();
    }

    let submissions = client
        .call_tool(
            "list_evidence_submissions",
            json!({ "evidence_id": evidence_id }),
        )
        .await["submissions"]
        .as_array()
        .expect("submissions is an array")
        .clone();

    assert!(submissions.is_empty());
}

#[tokio::test]
async fn pending_document_cannot_be_archived_until_the_parked_scan_is_released() {
    let app = harness::app().await;
    let subject = "auth0|upload-pending-archive";

    let scenario = ScenarioBuilder::new(&app)
        .with_user(subject)
        .with_workspace(subject, "Pending Archive")
        .with_evidence("Pending Archive", "Pending archive evidence")
        .build()
        .await;

    let token = authorize_agent_connection(
        &app,
        subject,
        "Pending Archive Claude",
        &WorkspacePermission::ALL,
    )
    .await;
    let client = McpClient::connect(app.mcp_server(), &token).await;
    let evidence_id = scenario
        .workspace("Pending Archive")
        .evidence("Pending archive evidence")
        .id;
    let grant = client
        .call_tool(
            "manage_evidence_submissions",
            json!({
                "evidence_id": evidence_id,
                "valid_from": VALID_FROM,
                "valid_until": VALID_UNTIL,
            }),
        )
        .await;
    let redeemed = app
        .app_server()
        .get(&local_path(
            grant["url"].as_str().expect("grant URL is a string"),
        ))
        .await;
    redeemed.assert_status(StatusCode::SEE_OTHER);
    let cookie = request_cookie(
        redeemed
            .header("set-cookie")
            .to_str()
            .expect("cookie header is text"),
    );

    let request_id = Uuid::new_v4();
    let mut gate = app
        .pipeline_controls()
        .hold(DOCUMENT_SCAN_REQUESTED, request_id);
    let mut events = app.pipeline_events().subscribe();
    app.app_server()
        .post("/evidence-document-uploads/files")
        .add_header("cookie", cookie.clone())
        .add_header(REQUEST_ID_HEADER, request_id.to_string())
        .multipart(upload_form(b"pending archive", "pending-archive.txt"))
        .await
        .assert_status(StatusCode::SEE_OTHER);

    let interception = gate.await_interception().await;
    let submission = client
        .call_tool(
            "list_evidence_submissions",
            json!({ "evidence_id": evidence_id }),
        )
        .await["submissions"]
        .as_array()
        .expect("submissions is an array")
        .first()
        .expect("pending submission is listed")
        .clone();
    let document_id = submission["document"]["id"]
        .as_str()
        .expect("document id is a string");
    assert_eq!(interception.aggregate_id, document_id);
    assert_eq!(submission["document"]["upload_status"], "pending");

    let archive = app
        .app_server()
        .post(&archive_path(&submission))
        .add_header("cookie", cookie)
        .await;
    archive.assert_status(StatusCode::CONFLICT);
    assert!(archive.text().contains(
        r#"<section class="notice" role="alert"><strong>Archive failed: this document is not ready to archive</strong><p>Review the message above, then try again.</p></section>"#
    ));

    gate.release();
    assert_eq!(
        events
            .await_event(DOCUMENT_SCAN_REQUESTED, document_id)
            .await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        events
            .await_event(DOCUMENT_FINALIZATION_REQUESTED, document_id)
            .await,
        StatusCode::NO_CONTENT
    );
    let settled = client
        .call_tool(
            "list_evidence_submissions",
            json!({ "evidence_id": evidence_id }),
        )
        .await;
    assert_eq!(
        settled["submissions"][0]["document"]["upload_status"],
        "uploaded"
    );
}

#[tokio::test]
async fn finalizing_document_renders_scanning_and_rejects_download_until_released() {
    let app = harness::app().await;
    let subject = "auth0|upload-finalizing";

    let scenario = ScenarioBuilder::new(&app)
        .with_user(subject)
        .with_workspace(subject, "Finalizing State")
        .with_evidence("Finalizing State", "Finalizing evidence")
        .build()
        .await;

    let token = authorize_agent_connection(
        &app,
        subject,
        "Finalizing Claude",
        &WorkspacePermission::ALL,
    )
    .await;
    let client = McpClient::connect(app.mcp_server(), &token).await;
    let evidence_id = scenario
        .workspace("Finalizing State")
        .evidence("Finalizing evidence")
        .id;
    let grant = client
        .call_tool(
            "manage_evidence_submissions",
            json!({
                "evidence_id": evidence_id,
                "valid_from": VALID_FROM,
                "valid_until": VALID_UNTIL,
            }),
        )
        .await;
    let redeemed = app
        .app_server()
        .get(&local_path(
            grant["url"].as_str().expect("grant URL is a string"),
        ))
        .await;
    redeemed.assert_status(StatusCode::SEE_OTHER);
    let cookie = request_cookie(
        redeemed
            .header("set-cookie")
            .to_str()
            .expect("cookie header is text"),
    );

    let parked_request_id = Uuid::new_v4();
    let mut gate = app
        .pipeline_controls()
        .hold(DOCUMENT_FINALIZATION_REQUESTED, parked_request_id);
    let mut parked_events = app.pipeline_events().subscribe();
    app.app_server()
        .post("/evidence-document-uploads/files")
        .add_header("cookie", cookie.clone())
        .add_header(REQUEST_ID_HEADER, parked_request_id.to_string())
        .multipart(upload_form(b"parked document", "parked.txt"))
        .await
        .assert_status(StatusCode::SEE_OTHER);

    let interception = gate.await_interception().await;
    let finalizing_read = client
        .call_tool(
            "list_evidence_submissions",
            json!({ "evidence_id": evidence_id }),
        )
        .await;
    let finalizing = finalizing_read["submissions"]
        .as_array()
        .expect("submissions is an array")
        .iter()
        .find(|submission| submission["document"]["filename"] == "parked.txt")
        .expect("parked submission is listed")
        .clone();
    let parked_document_id = finalizing["document"]["id"]
        .as_str()
        .expect("document id is a string");
    assert_eq!(interception.aggregate_id, parked_document_id);
    assert_eq!(finalizing["document"]["upload_status"], "finalizing");

    let page = app
        .app_server()
        .get("/evidence-document-uploads")
        .add_header("cookie", cookie.clone())
        .await;
    page.assert_status_ok();
    let html = page.text();
    let start = html.find("<tbody>").expect("page has a file table") + "<tbody>".len();
    let end = html.find("</tbody>").expect("file table closes");
    assert_eq!(
        &html[start..end],
        processing_file_row(&finalizing, "Scanning")
    );

    let download = app
        .app_server()
        .get(&download_path(&finalizing))
        .add_header("cookie", cookie.clone())
        .await;
    download.assert_status(StatusCode::CONFLICT);
    assert_eq!(
        download.json::<Value>(),
        json!({
            "error": {
                "code": "document_not_ready",
                "message": "document is not ready for download",
                "details": [],
            }
        })
    );

    // Dropping a scoped gate must release an already parked real delivery.
    drop(gate);
    assert_eq!(
        parked_events
            .await_event(DOCUMENT_FINALIZATION_REQUESTED, parked_document_id)
            .await,
        StatusCode::NO_CONTENT
    );
    let settled = client
        .call_tool(
            "list_evidence_submissions",
            json!({ "evidence_id": evidence_id }),
        )
        .await;
    assert_eq!(
        settled["submissions"]
            .as_array()
            .expect("submissions is an array")
            .iter()
            .find(|submission| submission["document"]["id"] == parked_document_id)
            .expect("parked submission remains listed")["document"]["upload_status"],
        "uploaded"
    );
}

#[tokio::test]
async fn clean_document_completes_while_another_worker_request_waits_for_clamd() {
    let app = harness::app().await;
    let subject = "auth0|upload-clamd-stall";

    let scenario = ScenarioBuilder::new(&app)
        .with_user(subject)
        .with_workspace(subject, "Clamd Stall")
        .with_evidence("Clamd Stall", "Concurrent scanner evidence")
        .build()
        .await;

    let token = authorize_agent_connection(
        &app,
        subject,
        "Clamd Stall Claude",
        &WorkspacePermission::ALL,
    )
    .await;
    let client = McpClient::connect(app.mcp_server(), &token).await;
    let evidence_id = scenario
        .workspace("Clamd Stall")
        .evidence("Concurrent scanner evidence")
        .id;
    let grant = client
        .call_tool(
            "manage_evidence_submissions",
            json!({
                "evidence_id": evidence_id,
                "valid_from": VALID_FROM,
                "valid_until": VALID_UNTIL,
            }),
        )
        .await;
    let redeemed = app
        .app_server()
        .get(&local_path(
            grant["url"].as_str().expect("grant URL is a string"),
        ))
        .await;
    redeemed.assert_status(StatusCode::SEE_OTHER);
    let cookie = request_cookie(
        redeemed
            .header("set-cookie")
            .to_str()
            .expect("cookie header is text"),
    );

    let mut stalled_scan = app.clamd_controls().hang();
    let stalled_bytes = stalled_scan.document_bytes().to_vec();
    let mut stalled_events = app.pipeline_events().subscribe();
    app.app_server()
        .post("/evidence-document-uploads/files")
        .add_header("cookie", cookie.clone())
        .add_header(REQUEST_ID_HEADER, Uuid::new_v4().to_string())
        .multipart(upload_form(&stalled_bytes, "stalled-scan.txt"))
        .await
        .assert_status(StatusCode::SEE_OTHER);

    // This fires only after the fake has read the complete real INSTREAM request.
    stalled_scan.await_interception().await;
    let stalled_read = client
        .call_tool(
            "list_evidence_submissions",
            json!({ "evidence_id": evidence_id }),
        )
        .await;
    let stalled_document_id = stalled_read["submissions"]
        .as_array()
        .expect("submissions is an array")
        .iter()
        .find(|submission| submission["document"]["filename"] == "stalled-scan.txt")
        .expect("stalled submission is listed")["document"]["id"]
        .as_str()
        .expect("document id is a string")
        .to_owned();
    assert_eq!(
        stalled_read["submissions"]
            .as_array()
            .expect("submissions is an array")
            .iter()
            .find(|submission| submission["document"]["id"] == stalled_document_id)
            .expect("stalled submission remains listed")["document"]["upload_status"],
        "pending"
    );

    let mut clean_events = app.pipeline_events().subscribe();
    app.app_server()
        .post("/evidence-document-uploads/files")
        .add_header("cookie", cookie)
        .add_header(REQUEST_ID_HEADER, Uuid::new_v4().to_string())
        .multipart(upload_form(b"clean concurrent document", "clean-scan.txt"))
        .await
        .assert_status(StatusCode::SEE_OTHER);
    let during_stall = client
        .call_tool(
            "list_evidence_submissions",
            json!({ "evidence_id": evidence_id }),
        )
        .await;
    let clean_document_id = during_stall["submissions"]
        .as_array()
        .expect("submissions is an array")
        .iter()
        .find(|submission| submission["document"]["filename"] == "clean-scan.txt")
        .expect("clean submission is listed")["document"]["id"]
        .as_str()
        .expect("document id is a string")
        .to_owned();

    assert_eq!(
        clean_events
            .await_event(DOCUMENT_SCAN_REQUESTED, &clean_document_id)
            .await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        clean_events
            .await_event(DOCUMENT_FINALIZATION_REQUESTED, &clean_document_id)
            .await,
        StatusCode::NO_CONTENT
    );
    let independently_settled = client
        .call_tool(
            "list_evidence_submissions",
            json!({ "evidence_id": evidence_id }),
        )
        .await;
    let submissions = independently_settled["submissions"]
        .as_array()
        .expect("submissions is an array");
    assert_eq!(
        submissions
            .iter()
            .find(|submission| submission["document"]["id"] == clean_document_id)
            .expect("clean submission remains listed")["document"]["upload_status"],
        "uploaded"
    );
    assert_eq!(
        submissions
            .iter()
            .find(|submission| submission["document"]["id"] == stalled_document_id)
            .expect("stalled submission remains listed")["document"]["upload_status"],
        "pending"
    );

    stalled_scan.release();
    assert_eq!(
        stalled_events
            .await_event(DOCUMENT_SCAN_REQUESTED, &stalled_document_id)
            .await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        stalled_events
            .await_event(DOCUMENT_FINALIZATION_REQUESTED, &stalled_document_id)
            .await,
        StatusCode::NO_CONTENT
    );
    let settled = client
        .call_tool(
            "list_evidence_submissions",
            json!({ "evidence_id": evidence_id }),
        )
        .await;
    assert_eq!(
        settled["submissions"]
            .as_array()
            .expect("submissions is an array")
            .iter()
            .find(|submission| submission["document"]["id"] == stalled_document_id)
            .expect("stalled submission remains listed")["document"]["upload_status"],
        "uploaded"
    );
}

#[tokio::test]
async fn duplicate_scan_delivery_is_acknowledged_without_repeating_lifecycle_work() {
    let app = harness::app().await;
    let subject = "auth0|upload-duplicate-scan";

    let scenario = ScenarioBuilder::new(&app)
        .with_user(subject)
        .with_workspace(subject, "Duplicate Scan")
        .with_evidence("Duplicate Scan", "Duplicate delivery evidence")
        .build()
        .await;

    let token = authorize_agent_connection(
        &app,
        subject,
        "Duplicate Scan Claude",
        &WorkspacePermission::ALL,
    )
    .await;
    let client = McpClient::connect(app.mcp_server(), &token).await;
    let evidence_id = scenario
        .workspace("Duplicate Scan")
        .evidence("Duplicate delivery evidence")
        .id;
    let grant = client
        .call_tool(
            "manage_evidence_submissions",
            json!({
                "evidence_id": evidence_id,
                "valid_from": VALID_FROM,
                "valid_until": VALID_UNTIL,
            }),
        )
        .await;
    let redeemed = app
        .app_server()
        .get(&local_path(
            grant["url"].as_str().expect("grant URL is a string"),
        ))
        .await;
    redeemed.assert_status(StatusCode::SEE_OTHER);
    let cookie = request_cookie(
        redeemed
            .header("set-cookie")
            .to_str()
            .expect("cookie header is text"),
    );

    let mut events = app.pipeline_events().subscribe();
    let (result, logs) = app
        .capture_audit_logs(async |request_id| {
            let mut failure = app
                .pipeline_controls()
                .fail_after_forward_once(DOCUMENT_SCAN_REQUESTED, request_id);
            let uploaded = app
                .app_server()
                .post("/evidence-document-uploads/files")
                .add_header("cookie", cookie)
                .add_header(REQUEST_ID_HEADER, request_id.to_string())
                .multipart(upload_form(b"duplicate scan", "duplicate-scan.txt"))
                .await;
            let first_delivery = failure.await_first_delivery().await;
            let deliveries = failure.await_redelivery().await;
            let finalization_status = events
                .await_event(DOCUMENT_FINALIZATION_REQUESTED, &deliveries[0].aggregate_id)
                .await;
            failure.release();
            (uploaded, first_delivery, deliveries, finalization_status)
        })
        .await;
    let (uploaded, first_delivery, deliveries, finalization_status) = result;
    uploaded.assert_status(StatusCode::SEE_OTHER);
    assert_eq!(first_delivery, deliveries[0]);
    assert_eq!(deliveries[0].message_id, deliveries[1].message_id);
    assert_eq!(deliveries[0].aggregate_id, deliveries[1].aggregate_id);
    assert_eq!(deliveries[0].worker_status, StatusCode::NO_CONTENT);
    assert_eq!(deliveries[1].worker_status, StatusCode::NO_CONTENT);
    assert_eq!(finalization_status, StatusCode::NO_CONTENT);

    let settled = client
        .call_tool(
            "list_evidence_submissions",
            json!({ "evidence_id": evidence_id }),
        )
        .await;
    assert_eq!(
        settled["submissions"]
            .as_array()
            .expect("submissions is an array")
            .len(),
        1
    );
    assert_eq!(
        settled["submissions"][0]["document"]["id"],
        deliveries[0].aggregate_id
    );
    assert_eq!(
        settled["submissions"][0]["document"]["upload_status"],
        "uploaded"
    );

    assert_eq!(logs.len(), 3);
    let lifecycle = logs
        .iter()
        .map(|record| {
            let fields = &record["fields"];
            let metadata: Value = serde_json::from_str(
                fields["metadata"]
                    .as_str()
                    .expect("audit metadata is serialized JSON"),
            )
            .expect("audit metadata parses");
            json!({
                "event_name": fields["event_name"],
                "outcome": fields["outcome"],
                "lifecycle_status": metadata["lifecycle_status"],
            })
        })
        .collect::<Vec<_>>();
    assert_eq!(
        lifecycle,
        [
            json!({
                "event_name": "evidence_document.accepted",
                "outcome": "success",
                "lifecycle_status": "pending",
            }),
            json!({
                "event_name": "evidence_document_scan.completed",
                "outcome": "success",
                "lifecycle_status": "finalizing",
            }),
            json!({
                "event_name": "evidence_document_finalization.completed",
                "outcome": "success",
                "lifecycle_status": "uploaded",
            }),
        ]
    );
}

#[tokio::test]
async fn archival_is_scoped_to_the_session() {
    let app = harness::app().await;

    let scenario = ScenarioBuilder::new(&app)
        .with_user("auth0|upload-archival")
        .with_workspace("auth0|upload-archival", "Archival")
        .with_evidence("Archival", "Archive evidence")
        .with_evidence("Archival", "Other evidence")
        .build()
        .await;

    let token = authorize_agent_connection(
        &app,
        "auth0|upload-archival",
        "Claude",
        &WorkspacePermission::ALL,
    )
    .await;
    let client = McpClient::connect(app.mcp_server(), &token).await;

    let workspace = scenario.workspace("Archival");
    let evidence_id = workspace.evidence("Archive evidence").id;
    let other_evidence_id = workspace.evidence("Other evidence").id;

    let grant = client
        .call_tool(
            "manage_evidence_submissions",
            json!({
                "evidence_id": evidence_id,
                "valid_from": VALID_FROM,
                "valid_until": VALID_UNTIL,
            }),
        )
        .await;
    let redeemed = app
        .app_server()
        .get(&local_path(
            grant["url"].as_str().expect("grant URL is a string"),
        ))
        .await;
    redeemed.assert_status(StatusCode::SEE_OTHER);
    let cookie = request_cookie(
        redeemed
            .header("set-cookie")
            .to_str()
            .expect("cookie header is text"),
    );
    let mut events = app.pipeline_events().subscribe();
    app.app_server()
        .post("/evidence-document-uploads/files")
        .add_header("cookie", cookie.clone())
        .multipart(upload_form(b"pending", "pending.txt"))
        .await
        .assert_status(StatusCode::SEE_OTHER);

    let own = client
        .call_tool(
            "list_evidence_submissions",
            json!({ "evidence_id": evidence_id }),
        )
        .await["submissions"]
        .as_array()
        .expect("submissions is an array")
        .clone()
        .remove(0);

    let own_document_id = own["document"]["id"]
        .as_str()
        .expect("document id is a string");
    assert_eq!(
        events
            .await_event(DOCUMENT_SCAN_REQUESTED, own_document_id)
            .await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        events
            .await_event(DOCUMENT_FINALIZATION_REQUESTED, own_document_id)
            .await,
        StatusCode::NO_CONTENT
    );
    let settled = client
        .call_tool(
            "list_evidence_submissions",
            json!({ "evidence_id": evidence_id }),
        )
        .await["submissions"]
        .as_array()
        .expect("submissions is an array")
        .clone()
        .remove(0);
    assert_eq!(settled["document"]["upload_status"], "uploaded");

    app.app_server()
        .post(&archive_path(&own))
        .add_header("cookie", cookie.clone())
        .await
        .assert_status(StatusCode::SEE_OTHER);

    let other_grant = client
        .call_tool(
            "manage_evidence_submissions",
            json!({
                "evidence_id": other_evidence_id,
                "valid_from": VALID_FROM,
                "valid_until": VALID_UNTIL,
            }),
        )
        .await;
    let other_redeemed = app
        .app_server()
        .get(&local_path(
            other_grant["url"].as_str().expect("grant URL is a string"),
        ))
        .await;
    other_redeemed.assert_status(StatusCode::SEE_OTHER);
    let other_cookie = request_cookie(
        other_redeemed
            .header("set-cookie")
            .to_str()
            .expect("cookie header is text"),
    );
    app.app_server()
        .post("/evidence-document-uploads/files")
        .add_header("cookie", other_cookie)
        .multipart(upload_form(b"other", "other.txt"))
        .await
        .assert_status(StatusCode::SEE_OTHER);

    let other = client
        .call_tool(
            "list_evidence_submissions",
            json!({ "evidence_id": other_evidence_id }),
        )
        .await["submissions"]
        .as_array()
        .expect("submissions is an array")
        .clone()
        .remove(0);

    // One session's cookie says nothing about another session's documents.
    app.app_server()
        .get(&download_path(&other))
        .add_header("cookie", cookie.clone())
        .await
        .assert_status_not_found();
    app.app_server()
        .post(&archive_path(&other))
        .add_header("cookie", cookie)
        .await
        .assert_status_not_found();
}

#[tokio::test]
async fn grant_issuance_conceals_missing_and_cross_workspace_evidence() {
    let app = harness::app().await;
    let subject = "auth0|upload-owner";
    let stranger = "auth0|upload-stranger";

    let scenario = ScenarioBuilder::new(&app)
        .with_user(subject)
        .with_workspace(subject, "Upload Owner")
        .with_evidence("Upload Owner", "Owned evidence")
        .with_user(stranger)
        .with_workspace(stranger, "Upload Stranger")
        .build()
        .await;

    let outsider_token =
        authorize_agent_connection(&app, stranger, "Stranger Claude", &WorkspacePermission::ALL)
            .await;
    let outsider = McpClient::connect(app.mcp_server(), &outsider_token).await;

    let evidence_id = scenario
        .workspace("Upload Owner")
        .evidence("Owned evidence")
        .id;

    for (case, target) in [
        ("unknown evidence", Uuid::new_v4()),
        ("another workspace's evidence", evidence_id),
    ] {
        let error = outsider
            .call_tool_error(
                "manage_evidence_submissions",
                json!({
                    "evidence_id": target,
                    "valid_from": VALID_FROM,
                    "valid_until": VALID_UNTIL,
                }),
            )
            .await;

        assert_eq!(
            error.data["problem"]["code"], "not_found",
            "{case} is concealed"
        );
    }
}

fn uploaded_file_row(submission: &Value) -> String {
    let filename = submission["document"]["filename"]
        .as_str()
        .expect("filename is a string");
    let size = submission["document"]["content_length"]
        .as_u64()
        .expect("content length is an integer");
    format!(
        concat!(
            r#"<tr><td class="filename" data-label="File">{}</td>"#,
            r#"<td data-label="Size">{} B</td>"#,
            r#"<td data-label="Status"><span class="status">Uploaded</span></td>"#,
            r#"<td data-label="Actions"><div class="actions">"#,
            r#"<a class="button icon-button" href="{}" aria-label="Download document">"#,
            r#"<svg viewBox="0 0 24 24" aria-hidden="true" fill="none" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">"#,
            r#"<path d="M12 3v12"/><path d="m7 10 5 5 5-5"/><path d="M5 21h14"/></svg>"#,
            r#"<span class="sr-only">Download</span></a>"#,
            r#"<form method="post" action="{}" onsubmit="return confirm('Archive this document?');">"#,
            r#"<button class="icon-button danger-button" type="submit" aria-label="Archive document">"#,
            r#"<svg viewBox="0 0 24 24" aria-hidden="true" fill="none" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">"#,
            r#"<path d="M3 6h18"/><path d="M8 6V4h8v2"/><path d="M19 6l-1 14H6L5 6"/><path d="M10 11v5"/><path d="M14 11v5"/></svg>"#,
            r#"<span class="sr-only">Archive</span></button></form></div></td></tr>"#,
        ),
        filename,
        size,
        download_path(submission),
        archive_path(submission),
    )
}

fn processing_file_row(submission: &Value, status: &str) -> String {
    let filename = submission["document"]["filename"]
        .as_str()
        .expect("filename is a string");
    let size = submission["document"]["content_length"]
        .as_u64()
        .expect("content length is an integer");
    format!(
        concat!(
            r#"<tr><td class="filename" data-label="File">{}</td>"#,
            r#"<td data-label="Size">{} B</td>"#,
            r#"<td data-label="Status"><span class="status">{}</span></td>"#,
            r#"<td data-label="Actions"></td></tr>"#,
        ),
        filename, size, status,
    )
}
