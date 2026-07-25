use axum_test::TestResponse;
use http::StatusCode;
use proofplane::{
    domain::WorkspacePermission,
    routes::request_context::REQUEST_ID_HEADER,
    worker::{DOCUMENT_FINALIZATION_REQUESTED, DOCUMENT_SCAN_REQUESTED},
};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::support::{
    agent_connections::get_agent_connection_id_for,
    clamd::{EICAR, ERROR_TRIGGER},
    documents::upload_form,
    evidence_documents::{archive_path, download_path, VALID_FROM, VALID_UNTIL},
    harness,
    http::{local_path, request_cookie},
    json::{assert_rfc3339, object_keys},
    mcp::McpClient,
    oauth::authorize_agent_connection,
    scenario::{types::TestEvidenceSubmission, ScenarioBuilder},
};

#[tokio::test]
async fn missing_empty_and_malformed_download_tokens_share_not_found_response() {
    let app = harness::app().await;

    for path in [
        "/document-downloads",
        "/document-downloads?token=",
        "/document-downloads?token=malformed",
    ] {
        let response = app.app_server().get(path).await;
        assert_not_found(&response);
    }
}

#[tokio::test]
async fn clean_document_settles_and_reusable_download_grant_streams_safe_headers() {
    let app = harness::app().await;
    let subject = "auth0|download-clean";

    let scenario = ScenarioBuilder::new(&app)
        .with_user(subject)
        .with_workspace(subject, "Download Clean")
        .with_evidence("Download Clean", "Downloadable evidence")
        .build()
        .await;
    let user_id = scenario.user(subject).id;
    let workspace = scenario.workspace("Download Clean");
    let workspace_id = workspace.id;

    let token =
        authorize_agent_connection(&app, subject, "Download Claude", &WorkspacePermission::ALL)
            .await;
    let connection_id = get_agent_connection_id_for(&app, subject, "Download Claude").await;
    let client = McpClient::connect(app.mcp_server(), &token).await;
    let evidence_id = workspace.evidence("Downloadable evidence").id;
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
    let content = b"downloadable evidence";
    let filename = "Quarterly evidence (final).txt";

    app.app_server()
        .post("/evidence-document-uploads/files")
        .add_header("cookie", cookie.clone())
        .multipart(upload_form(content, filename))
        .await
        .assert_status(StatusCode::SEE_OTHER);

    let submitted = client
        .call_tool(
            "list_evidence_submissions",
            json!({ "evidence_id": evidence_id }),
        )
        .await["submissions"]
        .as_array()
        .expect("submissions is an array")
        .first()
        .expect("uploaded submission is listed")
        .clone();
    let submission = TestEvidenceSubmission::from_mcp(&submitted);
    let document_id = submission.document_id.to_string();
    assert_eq!(
        events
            .await_event(DOCUMENT_SCAN_REQUESTED, &document_id)
            .await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        events
            .await_event(DOCUMENT_FINALIZATION_REQUESTED, &document_id)
            .await,
        StatusCode::NO_CONTENT
    );

    let issued = app
        .app_server()
        .get(&download_path(&submitted))
        .add_header("cookie", cookie.clone())
        .await;
    issued.assert_status(StatusCode::SEE_OTHER);
    let path = local_path(
        issued
            .header("location")
            .to_str()
            .expect("download grant location is text"),
    );

    let tampered = app.app_server().get(&format!("{path}tampered")).await;
    assert_not_found(&tampered);

    let ((first, request_id), logs) = app
        .capture_audit_logs(async |request_id| {
            let response = app
                .app_server()
                .get(&path)
                .add_header(REQUEST_ID_HEADER, request_id.to_string())
                .await;
            (response, request_id)
        })
        .await;
    assert_download(&first, content, filename);
    assert_eq!(logs.len(), 1);
    assert_download_audit_event(
        &logs[0],
        ExpectedDownloadAudit {
            user_id,
            connection_id,
            workspace_id,
            request_id,
            submission_id: submission.id,
            document_id: submission.document_id,
        },
    );
    let second = app.app_server().get(&path).await;
    assert_download(&second, content, filename);

    app.app_server()
        .post(&archive_path(&submitted))
        .add_header("cookie", cookie)
        .await
        .assert_status(StatusCode::SEE_OTHER);

    let archived = app.app_server().get(&path).await;
    assert_not_found(&archived);
}

#[tokio::test]
async fn eicar_document_settles_as_contains_virus_and_conceals_download_grants() {
    let app = harness::app().await;
    let subject = "auth0|download-eicar";

    let scenario = ScenarioBuilder::new(&app)
        .with_user(subject)
        .with_workspace(subject, "Download Eicar")
        .with_evidence("Download Eicar", "Malicious evidence")
        .build()
        .await;

    let token =
        authorize_agent_connection(&app, subject, "Eicar Claude", &WorkspacePermission::ALL).await;
    let client = McpClient::connect(app.mcp_server(), &token).await;
    let evidence_id = scenario
        .workspace("Download Eicar")
        .evidence("Malicious evidence")
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

    app.app_server()
        .post("/evidence-document-uploads/files")
        .add_header("cookie", cookie.clone())
        .multipart(upload_form(EICAR, "eicar.txt"))
        .await
        .assert_status(StatusCode::SEE_OTHER);

    let submitted = client
        .call_tool(
            "list_evidence_submissions",
            json!({ "evidence_id": evidence_id }),
        )
        .await["submissions"]
        .as_array()
        .expect("submissions is an array")
        .first()
        .expect("uploaded submission is listed")
        .clone();
    let document_id = document_id(&submitted);
    assert_eq!(
        events
            .await_event(DOCUMENT_SCAN_REQUESTED, document_id)
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
        .first()
        .expect("uploaded submission is listed")
        .clone();
    assert_eq!(settled["document"]["upload_status"], "contains_virus");
    app.app_server()
        .get(&download_path(&settled))
        .add_header("cookie", cookie)
        .await
        .assert_status_not_found();
}

#[tokio::test]
async fn clamd_error_document_settles_as_failed_and_conceals_download_grants() {
    let app = harness::app().await;
    let subject = "auth0|download-clamd-error";

    let scenario = ScenarioBuilder::new(&app)
        .with_user(subject)
        .with_workspace(subject, "Download Clamd Error")
        .with_evidence("Download Clamd Error", "Failed evidence")
        .build()
        .await;

    let token = authorize_agent_connection(
        &app,
        subject,
        "Clamd Error Claude",
        &WorkspacePermission::ALL,
    )
    .await;
    let client = McpClient::connect(app.mcp_server(), &token).await;
    let evidence_id = scenario
        .workspace("Download Clamd Error")
        .evidence("Failed evidence")
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

    app.app_server()
        .post("/evidence-document-uploads/files")
        .add_header("cookie", cookie.clone())
        .multipart(upload_form(ERROR_TRIGGER, "scanner-error.txt"))
        .await
        .assert_status(StatusCode::SEE_OTHER);

    let submitted = client
        .call_tool(
            "list_evidence_submissions",
            json!({ "evidence_id": evidence_id }),
        )
        .await["submissions"]
        .as_array()
        .expect("submissions is an array")
        .first()
        .expect("uploaded submission is listed")
        .clone();
    let document_id = document_id(&submitted);
    assert_eq!(
        events
            .await_event(DOCUMENT_SCAN_REQUESTED, document_id)
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
        .first()
        .expect("uploaded submission is listed")
        .clone();
    assert_eq!(settled["document"]["upload_status"], "failed");
    app.app_server()
        .get(&download_path(&settled))
        .add_header("cookie", cookie)
        .await
        .assert_status_not_found();
}

fn document_id(submission: &Value) -> &str {
    submission["document"]["id"]
        .as_str()
        .expect("document id is a string")
}

fn assert_download(response: &TestResponse, content: &[u8], filename: &str) {
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

struct ExpectedDownloadAudit {
    user_id: Uuid,
    connection_id: Uuid,
    workspace_id: Uuid,
    request_id: Uuid,
    submission_id: Uuid,
    document_id: Uuid,
}

#[track_caller]
fn assert_download_audit_event(record: &Value, expected: ExpectedDownloadAudit) {
    assert_eq!(
        object_keys(record),
        ["fields", "level", "target", "timestamp"]
            .into_iter()
            .collect()
    );
    assert_eq!(record["level"], "INFO");
    assert_eq!(record["target"], "proofplane::audit");
    assert_rfc3339(&record["timestamp"]);

    let fields = &record["fields"];
    assert_eq!(
        object_keys(fields),
        [
            "actor_type",
            "agent_connection_id",
            "client_type",
            "event_id",
            "event_name",
            "metadata",
            "object_id",
            "object_type",
            "operation",
            "outcome",
            "request_id",
            "type",
            "user_id",
            "workspace_id",
        ]
        .into_iter()
        .collect()
    );
    assert_eq!(fields["type"], "audit_log");
    Uuid::parse_str(fields["event_id"].as_str().expect("event id is a string"))
        .expect("event id is a UUID");
    assert_eq!(
        fields["event_name"],
        "evidence_document_download_grant.redeemed"
    );
    assert_eq!(fields["outcome"], "success");
    assert_eq!(fields["actor_type"], "agent_connection");
    assert_eq!(fields["user_id"], expected.user_id.to_string());
    assert_eq!(
        fields["agent_connection_id"],
        expected.connection_id.to_string()
    );
    assert_eq!(fields["client_type"], "rest");
    assert_eq!(fields["operation"], "redeem_document_download_grant");
    assert_eq!(fields["workspace_id"], expected.workspace_id.to_string());
    assert_eq!(fields["request_id"], expected.request_id.to_string());
    assert_eq!(fields["object_type"], "evidence_document");
    assert_eq!(fields["object_id"], expected.document_id.to_string());
    assert_eq!(
        serde_json::from_str::<Value>(
            fields["metadata"]
                .as_str()
                .expect("audit metadata is serialized JSON"),
        )
        .expect("audit metadata parses"),
        json!({
            "evidence_submission_id": expected.submission_id,
            "evidence_document_id": expected.document_id,
        })
    );
}

fn assert_not_found(response: &TestResponse) {
    response.assert_status_not_found();
    assert_eq!(
        response.json::<Value>(),
        json!({
            "error": {
                "code": "not_found",
                "message": "route not found",
                "details": [],
            }
        })
    );
}
