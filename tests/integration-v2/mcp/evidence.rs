use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use http::StatusCode;
use proofplane::{
    domain::WorkspacePermission,
    worker::{DOCUMENT_FINALIZATION_REQUESTED, DOCUMENT_SCAN_REQUESTED},
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::support::{
    agent_connections::get_agent_connection_id_for,
    documents::upload_form,
    evidence_documents::{VALID_FROM, VALID_UNTIL},
    harness,
    http::{local_path, request_cookie},
    json::{assert_rfc3339, object_keys},
    mcp::{assert_not_found, assert_validation_error, McpClient},
    oauth::authorize_agent_connection,
    scenario::ScenarioBuilder,
};

#[tokio::test]
async fn evidence_creation_list_and_get_are_complete_ordered_scoped_and_audited() {
    let app = harness::app().await;
    let owner = "auth0|mcp-evidence-owner";
    let foreign = "auth0|mcp-evidence-foreign";

    let scenario = ScenarioBuilder::new(&app)
        .with_user(owner)
        .with_workspace(owner, "MCP Evidence Owner")
        .with_evidence("MCP Evidence Owner", "Zulu workspace fixture")
        .with_user(foreign)
        .with_workspace(foreign, "MCP Evidence Foreign")
        .with_evidence("MCP Evidence Foreign", "Hidden foreign evidence")
        .build()
        .await;
    let owner_user_id = scenario.user(owner).id;
    let owner_workspace_id = scenario.workspace("MCP Evidence Owner").id;

    let owner_token =
        authorize_agent_connection(&app, owner, "Evidence Owner", &WorkspacePermission::ALL).await;

    let owner_connection_id = get_agent_connection_id_for(&app, owner, "Evidence Owner").await;

    let (captured, logs) = app
        .capture_audit_logs(async |request_id| {
            McpClient::connect_with_request_id(app.mcp_server(), &owner_token, request_id)
                .await
                .call_tool(
                    "create_evidence",
                    json!({
                        "title": "Alpha captured evidence",
                        "description": format!("Collect Alpha captured evidence."),
                        "collection_instructions": format!("Upload Alpha captured evidence."),
                    }),
                )
                .await
        })
        .await;
    let captured_id = Uuid::parse_str(
        captured["evidence"]["id"]
            .as_str()
            .expect("evidence id is a string"),
    )
    .expect("evidence id is a UUID");
    assert_evidence_projection(
        &captured["evidence"],
        captured_id,
        owner_workspace_id,
        "Alpha captured evidence",
    );
    assert_eq!(logs.len(), 1);
    assert_mcp_evidence_audit(
        &logs[0],
        "evidence.created",
        "create_evidence",
        owner_user_id,
        owner_connection_id,
        owner_workspace_id,
        captured_id,
    );

    let owner_client = McpClient::connect(app.mcp_server(), &owner_token).await;
    let same_workspace_id = scenario
        .workspace("MCP Evidence Owner")
        .evidence("Zulu workspace fixture")
        .id;
    let foreign_evidence_id = scenario
        .workspace("MCP Evidence Foreign")
        .evidence("Hidden foreign evidence")
        .id;

    let listed = owner_client.call_tool("list_evidence", json!({})).await;
    let listed_evidence = listed["evidence"].as_array().expect("evidence is an array");
    assert_eq!(listed_evidence.len(), 2);
    assert_eq!(listed_evidence[0], captured["evidence"]);
    assert_evidence_projection(
        &listed_evidence[1],
        same_workspace_id,
        owner_workspace_id,
        "Zulu workspace fixture",
    );

    let got = owner_client
        .call_tool("get_evidence", json!({ "evidence_id": captured_id }))
        .await;
    assert_eq!(got, json!({ "evidence": captured["evidence"].clone() }));

    let concealed = owner_client
        .call_tool_error(
            "get_evidence",
            json!({ "evidence_id": foreign_evidence_id }),
        )
        .await;
    assert_not_found(&concealed);
}

#[tokio::test]
async fn evidence_creation_rejects_blank_fields_and_read_only_connections_without_writes_or_audits()
{
    let app = harness::app().await;
    let subject = "auth0|mcp-evidence-create-rejections";

    ScenarioBuilder::new(&app)
        .with_user(subject)
        .with_workspace(subject, "MCP Evidence Create Rejections")
        .build()
        .await;

    let writer_token =
        authorize_agent_connection(&app, subject, "Evidence Writer", &WorkspacePermission::ALL)
            .await;
    let reader_token = authorize_agent_connection(
        &app,
        subject,
        "Evidence Reader",
        &[WorkspacePermission::ReadEvidence],
    )
    .await;

    let ((invalid, denied, listed), logs) = app
        .capture_audit_logs(async |request_id| {
            let writer =
                McpClient::connect_with_request_id(app.mcp_server(), &writer_token, request_id)
                    .await;
            let reader =
                McpClient::connect_with_request_id(app.mcp_server(), &reader_token, request_id)
                    .await;

            let invalid = writer
                .call_tool_error(
                    "create_evidence",
                    json!({
                        "title": "",
                        "description": " ",
                        "collection_instructions": "\t",
                    }),
                )
                .await;
            let denied = reader
                .call_tool_error(
                    "create_evidence",
                    json!({
                        "title": "Denied evidence",
                        "description": format!("Collect Denied evidence."),
                        "collection_instructions": format!("Upload Denied evidence."),
                    }),
                )
                .await;
            let listed = reader.call_tool("list_evidence", json!({})).await;

            (invalid, denied, listed)
        })
        .await;

    assert_validation_error(
        &invalid,
        json!([
            {"field": "title", "message": "title must not be empty"},
            {"field": "description", "message": "description must not be empty"},
            {
                "field": "collection_instructions",
                "message": "collection_instructions must not be empty"
            },
        ]),
    );
    assert_not_found(&denied);
    assert!(logs.is_empty());
    assert_eq!(listed, json!({ "evidence": [] }));
}

#[tokio::test]
async fn upload_grant_and_two_browser_uploads_produce_complete_newest_first_submission_reads() {
    let app = harness::app().await;
    let subject = "auth0|mcp-evidence-submission-reads";

    let scenario = ScenarioBuilder::new(&app)
        .with_user(subject)
        .with_workspace(subject, "MCP Evidence Submission Reads")
        .with_evidence(
            "MCP Evidence Submission Reads",
            "Browser submission evidence",
        )
        .build()
        .await;
    let user_id = scenario.user(subject).id;
    let workspace_id = scenario.workspace("MCP Evidence Submission Reads").id;

    let token = authorize_agent_connection(
        &app,
        subject,
        "Evidence Submission Manager",
        &WorkspacePermission::ALL,
    )
    .await;

    let connection_id =
        get_agent_connection_id_for(&app, subject, "Evidence Submission Manager").await;

    let client = McpClient::connect(app.mcp_server(), &token).await;
    let evidence_id = scenario
        .workspace("MCP Evidence Submission Reads")
        .evidence("Browser submission evidence")
        .id;

    let (grant, logs) = app
        .capture_audit_logs(async |request_id| {
            McpClient::connect_with_request_id(app.mcp_server(), &token, request_id)
                .await
                .call_tool(
                    "manage_evidence_submissions",
                    json!({
                        "evidence_id": evidence_id,
                        "valid_from": VALID_FROM,
                        "valid_until": VALID_UNTIL,
                    }),
                )
                .await
        })
        .await;

    let grant_fields = object_keys(&grant);
    assert_eq!(
        grant_fields,
        [
            "evidence_id",
            "expires_at",
            "intended_use",
            "url",
            "url_secret_type",
            "valid_from",
            "valid_until",
        ]
        .into_iter()
        .collect()
    );
    assert_eq!(grant["evidence_id"], evidence_id.to_string());
    assert_eq!(grant["valid_from"], VALID_FROM);
    assert_eq!(grant["valid_until"], VALID_UNTIL);
    assert_eq!(grant["url_secret_type"], "bearer_secret");
    assert_eq!(grant["intended_use"], "human_browser_evidence_upload");
    assert_rfc3339(&grant["expires_at"]);

    let grant_url = url::Url::parse(grant["url"].as_str().expect("grant URL is a string"))
        .expect("grant URL parses");
    assert_eq!(grant_url.path(), "/evidence-document-uploads");
    let grant_query = grant_url.query_pairs().collect::<Vec<_>>();
    assert_eq!(grant_query.len(), 1);
    assert_eq!(grant_query[0].0, "token");
    assert!(grant_query[0].1.len() > 1);

    assert_eq!(logs.len(), 1);
    assert_mcp_evidence_audit(
        &logs[0],
        "evidence_document_upload_grant.issued",
        "manage_evidence_submissions",
        user_id,
        connection_id,
        workspace_id,
        evidence_id,
    );

    let redeemed = app
        .app_server()
        .get(&local_path(
            grant["url"].as_str().expect("grant URL is a string"),
        ))
        .await;
    redeemed.assert_status(StatusCode::SEE_OTHER);
    assert_eq!(redeemed.header("location"), "/evidence-document-uploads");
    let cookie = request_cookie(
        redeemed
            .header("set-cookie")
            .to_str()
            .expect("cookie header is text"),
    );

    let older_bytes = b"older MCP evidence";
    let newer_bytes = b"newer MCP evidence";
    let mut older_events = app.pipeline_events().subscribe();
    app.app_server()
        .post("/evidence-document-uploads/files")
        .add_header("cookie", cookie.clone())
        .multipart(upload_form(older_bytes, "older.txt"))
        .await
        .assert_status(StatusCode::SEE_OTHER);

    let mut newer_events = app.pipeline_events().subscribe();
    app.app_server()
        .post("/evidence-document-uploads/files")
        .add_header("cookie", cookie)
        .multipart(upload_form(newer_bytes, "newer.txt"))
        .await
        .assert_status(StatusCode::SEE_OTHER);

    let arranged = client
        .call_tool(
            "list_evidence_submissions",
            json!({ "evidence_id": evidence_id }),
        )
        .await;
    let arranged = arranged["submissions"]
        .as_array()
        .expect("submissions is an array");
    assert_eq!(arranged.len(), 2);
    assert_eq!(arranged[0]["document"]["filename"], "newer.txt");
    assert_eq!(arranged[1]["document"]["filename"], "older.txt");

    let older_document_id = arranged[1]["document"]["id"]
        .as_str()
        .expect("older document id is a string")
        .to_owned();
    let newer_document_id = arranged[0]["document"]["id"]
        .as_str()
        .expect("newer document id is a string")
        .to_owned();
    assert_eq!(
        older_events
            .await_event(DOCUMENT_SCAN_REQUESTED, &older_document_id)
            .await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        older_events
            .await_event(DOCUMENT_FINALIZATION_REQUESTED, &older_document_id)
            .await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        newer_events
            .await_event(DOCUMENT_SCAN_REQUESTED, &newer_document_id)
            .await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        newer_events
            .await_event(DOCUMENT_FINALIZATION_REQUESTED, &newer_document_id)
            .await,
        StatusCode::NO_CONTENT
    );

    let listed = client
        .call_tool(
            "list_evidence_submissions",
            json!({ "evidence_id": evidence_id }),
        )
        .await;
    let submissions = listed["submissions"]
        .as_array()
        .expect("submissions is an array");
    assert_eq!(submissions.len(), 2);
    assert_eq!(submissions[0]["document"]["filename"], "newer.txt");
    assert_eq!(submissions[1]["document"]["filename"], "older.txt");
    assert_submission_projection(
        &submissions[0],
        evidence_id,
        user_id,
        connection_id,
        "newer.txt",
        newer_bytes,
    );
    assert_submission_projection(
        &submissions[1],
        evidence_id,
        user_id,
        connection_id,
        "older.txt",
        older_bytes,
    );

    let older_submission_id = submissions[1]["submission"]["id"].clone();
    let direct = client
        .call_tool(
            "get_evidence_submission",
            json!({ "submission_id": older_submission_id }),
        )
        .await;
    assert_eq!(direct, submissions[1]);

    let latest = client
        .call_tool(
            "get_latest_evidence_submission",
            json!({ "evidence_id": evidence_id }),
        )
        .await;
    assert_eq!(latest, submissions[0]);
}

#[tokio::test]
async fn upload_grant_rejections_are_exact_unaudited_and_create_no_observable_submissions() {
    let app = harness::app().await;
    let subject = "auth0|mcp-evidence-grant-rejections";

    let scenario = ScenarioBuilder::new(&app)
        .with_user(subject)
        .with_workspace(subject, "MCP Evidence Grant Rejections")
        .with_evidence("MCP Evidence Grant Rejections", "Grant validation evidence")
        .build()
        .await;

    let manager_token = authorize_agent_connection(
        &app,
        subject,
        "Evidence Grant Manager",
        &WorkspacePermission::ALL,
    )
    .await;
    let reader_token = authorize_agent_connection(
        &app,
        subject,
        "Evidence Submission Reader",
        &[WorkspacePermission::ReadEvidenceSubmissions],
    )
    .await;

    let evidence_id = scenario
        .workspace("MCP Evidence Grant Rejections")
        .evidence("Grant validation evidence")
        .id;

    let ((missing, malformed, inverted, unknown, denied, listed), logs) = app
        .capture_audit_logs(async |request_id| {
            let manager =
                McpClient::connect_with_request_id(app.mcp_server(), &manager_token, request_id)
                    .await;
            let reader =
                McpClient::connect_with_request_id(app.mcp_server(), &reader_token, request_id)
                    .await;

            let missing = manager
                .call_tool_error("manage_evidence_submissions", json!({}))
                .await;
            let malformed = manager
                .call_tool_error(
                    "manage_evidence_submissions",
                    json!({
                        "evidence_id": evidence_id,
                        "valid_from": "not-a-date",
                        "valid_until": VALID_UNTIL,
                    }),
                )
                .await;
            let inverted = manager
                .call_tool_error(
                    "manage_evidence_submissions",
                    json!({
                        "evidence_id": evidence_id,
                        "valid_from": "2026-04-01T00:00:00.000Z",
                        "valid_until": VALID_UNTIL,
                    }),
                )
                .await;
            let unknown = manager
                .call_tool_error(
                    "manage_evidence_submissions",
                    json!({
                        "evidence_id": Uuid::new_v4(),
                        "valid_from": VALID_FROM,
                        "valid_until": VALID_UNTIL,
                    }),
                )
                .await;
            let denied = reader
                .call_tool_error(
                    "manage_evidence_submissions",
                    json!({
                        "evidence_id": evidence_id,
                        "valid_from": VALID_FROM,
                        "valid_until": VALID_UNTIL,
                    }),
                )
                .await;
            let listed = reader
                .call_tool(
                    "list_evidence_submissions",
                    json!({ "evidence_id": evidence_id }),
                )
                .await;

            (missing, malformed, inverted, unknown, denied, listed)
        })
        .await;

    assert_validation_error(
        &missing,
        json!([
            {"field": "evidence_id", "message": "is required"},
            {"field": "valid_from", "message": "is required"},
            {"field": "valid_until", "message": "is required"},
        ]),
    );
    assert_validation_error(
        &malformed,
        json!([{
            "field": "valid_from",
            "message": "must be an RFC 3339 timestamp"
        }]),
    );
    assert_validation_error(
        &inverted,
        json!([{
            "field": "valid_until",
            "message": "valid_until must be greater than or equal to valid_from"
        }]),
    );
    assert_not_found(&unknown);
    assert_not_found(&denied);
    assert!(logs.is_empty());
    assert_eq!(listed, json!({ "submissions": [] }));
}

#[track_caller]
fn assert_evidence_projection(
    evidence: &Value,
    evidence_id: Uuid,
    workspace_id: Uuid,
    title: &str,
) {
    assert_eq!(
        object_keys(evidence),
        [
            "collection_instructions",
            "created_at",
            "description",
            "id",
            "status",
            "title",
            "updated_at",
            "workspace_id",
        ]
        .into_iter()
        .collect()
    );
    assert_eq!(evidence["id"], evidence_id.to_string());
    assert_eq!(evidence["workspace_id"], workspace_id.to_string());
    assert_eq!(evidence["title"], title);
    assert_eq!(evidence["description"], format!("Collect {title}."));
    assert_eq!(
        evidence["collection_instructions"],
        format!("Upload {title}.")
    );
    assert_eq!(evidence["status"], "active");
    assert_rfc3339(&evidence["created_at"]);
    assert_eq!(evidence["updated_at"], evidence["created_at"]);
}

#[track_caller]
fn assert_submission_projection(
    detail: &Value,
    evidence_id: Uuid,
    user_id: Uuid,
    connection_id: Uuid,
    filename: &str,
    bytes: &[u8],
) {
    assert_eq!(
        object_keys(detail),
        ["document", "submission"].into_iter().collect()
    );

    let submission = &detail["submission"];
    assert_eq!(
        object_keys(submission),
        [
            "evidence_id",
            "id",
            "received_at",
            "submitted_by",
            "valid_from",
            "valid_until",
        ]
        .into_iter()
        .collect()
    );
    Uuid::parse_str(
        submission["id"]
            .as_str()
            .expect("submission id is a string"),
    )
    .expect("submission id is a UUID");
    assert_eq!(submission["evidence_id"], evidence_id.to_string());
    assert_rfc3339(&submission["received_at"]);
    assert_eq!(submission["valid_from"], VALID_FROM);
    assert_eq!(submission["valid_until"], VALID_UNTIL);
    assert_eq!(
        object_keys(&submission["submitted_by"]),
        ["agent_connection_id", "user_id"].into_iter().collect()
    );
    assert_eq!(
        submission["submitted_by"]["agent_connection_id"],
        connection_id.to_string()
    );
    assert_eq!(submission["submitted_by"]["user_id"], user_id.to_string());

    let document = &detail["document"];
    assert_eq!(
        object_keys(document),
        [
            "checksum_crc32c",
            "checksum_sha256",
            "content_length",
            "content_type",
            "created_by_user_id",
            "evidence_submission_id",
            "filename",
            "id",
            "upload_status",
        ]
        .into_iter()
        .collect()
    );
    Uuid::parse_str(document["id"].as_str().expect("document id is a string"))
        .expect("document id is a UUID");
    assert_eq!(document["evidence_submission_id"], submission["id"]);
    assert_eq!(document["created_by_user_id"], user_id.to_string());
    assert_eq!(document["filename"], filename);
    assert_eq!(document["content_type"], "text/plain");
    assert_eq!(document["content_length"], bytes.len());
    assert_eq!(
        document["checksum_sha256"],
        hex::encode(Sha256::digest(bytes))
    );
    assert_eq!(
        document["checksum_crc32c"],
        BASE64_STANDARD.encode(crc32c::crc32c(bytes).to_be_bytes())
    );
    assert_eq!(document["upload_status"], "uploaded");
}

#[allow(clippy::too_many_arguments)]
#[track_caller]
fn assert_mcp_evidence_audit(
    record: &Value,
    event_name: &str,
    operation: &str,
    user_id: Uuid,
    connection_id: Uuid,
    workspace_id: Uuid,
    evidence_id: Uuid,
) {
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
    assert_eq!(fields["event_name"], event_name);
    assert_eq!(fields["outcome"], "success");
    assert_eq!(fields["actor_type"], "agent_connection");
    assert_eq!(fields["user_id"], user_id.to_string());
    assert_eq!(fields["agent_connection_id"], connection_id.to_string());
    assert_eq!(fields["client_type"], "mcp");
    assert_eq!(fields["operation"], operation);
    assert_eq!(fields["workspace_id"], workspace_id.to_string());
    Uuid::parse_str(
        fields["request_id"]
            .as_str()
            .expect("request id is a string"),
    )
    .expect("request id is a UUID");
    assert_eq!(fields["object_type"], "evidence");
    assert_eq!(fields["object_id"], evidence_id.to_string());
    assert_eq!(
        serde_json::from_str::<Value>(
            fields["metadata"]
                .as_str()
                .expect("audit metadata is serialized JSON")
        )
        .expect("audit metadata parses"),
        json!({ "evidence_id": evidence_id })
    );
}
