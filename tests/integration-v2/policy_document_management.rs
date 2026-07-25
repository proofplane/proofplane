use std::collections::BTreeSet;

use axum_test::{
    multipart::{MultipartForm, Part},
    TestResponse,
};
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use chrono::Utc;
use http::StatusCode;
use proofplane::{
    domain::WorkspacePermission,
    routes::request_context::REQUEST_ID_HEADER,
    worker::{DOCUMENT_FINALIZATION_REQUESTED, DOCUMENT_SCAN_REQUESTED},
};
use rmcp::model::ErrorCode;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::support::{
    agent_connections::get_agent_connection_id_for,
    clamd::EICAR,
    documents::upload_form,
    harness,
    http::{local_path, request_cookie},
    json::{assert_rfc3339, object_keys},
    mcp::{assert_not_found, assert_validation_error, McpClient, McpError},
    oauth::authorize_agent_connection,
    scenario::{types::TestPolicy, ScenarioBuilder},
};

const MANAGEMENT_PATH: &str = "/policy-document-uploads";
const UPLOAD_PATH: &str = "/policy-document-uploads/files";

#[tokio::test]
async fn grant_issues_and_redeems_once_with_a_scoped_session_and_complete_audits() {
    let app = harness::app().await;
    let subject = "auth0|policy-doc-grant-once";
    let workspace_name = "Policy Document Grant Once";

    let scenario = ScenarioBuilder::new(&app)
        .with_user(subject)
        .with_workspace(subject, workspace_name)
        .with_policy(workspace_name, "Grant Policy")
        .build()
        .await;
    let user_id = scenario.user(subject).id;
    let workspace = scenario.workspace(workspace_name);
    let workspace_id = workspace.id;
    let policy = workspace.policy("Grant Policy");

    let token = authorize_agent_connection(
        &app,
        subject,
        "Policy Document Grant Manager",
        &WorkspacePermission::ALL,
    )
    .await;
    let connection_id =
        get_agent_connection_id_for(&app, subject, "Policy Document Grant Manager").await;

    let ((grant, issue_request_id), issue_logs) = app
        .capture_audit_logs(async |request_id| {
            let grant = McpClient::connect_with_request_id(app.mcp_server(), &token, request_id)
                .await
                .call_tool("manage_policy_document", json!({ "policy_id": policy.id }))
                .await;
            (grant, request_id)
        })
        .await;
    assert_eq!(
        object_keys(&grant),
        [
            "expires_at",
            "intended_use",
            "policy_id",
            "url",
            "url_secret_type",
        ]
        .into_iter()
        .collect()
    );
    assert_eq!(grant["policy_id"], policy.id.to_string());
    assert_eq!(grant["url_secret_type"], "bearer_secret");
    assert_eq!(grant["intended_use"], "human_browser_document_management");
    let expires_at = chrono::DateTime::parse_from_rfc3339(
        grant["expires_at"]
            .as_str()
            .expect("grant expiry is a string"),
    )
    .expect("grant expiry is RFC 3339")
    .with_timezone(&Utc);
    assert!((1..=300).contains(&(expires_at - Utc::now()).num_seconds()));
    let grant_path = local_path(grant["url"].as_str().expect("grant URL is a string"));
    assert!(grant_path.starts_with("/policy-document-uploads?token=v4.local."));
    assert_policy_audit_event(
        &issue_logs,
        ExpectedAudit {
            event_name: "policy_document_upload_grant.issued",
            operation: "manage_policy_document",
            client_type: "mcp",
            user_id,
            connection_id,
            workspace_id,
            request_id: issue_request_id,
            object_type: "policy",
            object_id: policy.id,
            metadata: json!({ "policy_id": policy.id }),
        },
    );

    let ((redeemed, redeem_request_id), redeem_logs) = app
        .capture_audit_logs(async |request_id| {
            let response = app
                .app_server()
                .get(&grant_path)
                .add_header(REQUEST_ID_HEADER, request_id.to_string())
                .await;
            (response, request_id)
        })
        .await;
    redeemed.assert_status(StatusCode::SEE_OTHER);
    assert_eq!(redeemed.header("location"), MANAGEMENT_PATH);
    let set_cookie_header = redeemed.header("set-cookie");
    let set_cookie = set_cookie_header
        .to_str()
        .expect("policy session cookie is text");
    assert_policy_cookie(set_cookie);
    let cookie = request_cookie(set_cookie);
    assert_policy_audit_event(
        &redeem_logs,
        ExpectedAudit {
            event_name: "policy_document_upload_grant.redeemed",
            operation: "redeem_policy_document_upload_grant",
            client_type: "rest",
            user_id,
            connection_id,
            workspace_id,
            request_id: redeem_request_id,
            object_type: "policy",
            object_id: policy.id,
            metadata: json!({ "policy_id": policy.id }),
        },
    );

    let page = app
        .app_server()
        .get(MANAGEMENT_PATH)
        .add_header("cookie", cookie.clone())
        .await;
    page.assert_status_ok();
    assert_eq!(
        current_document_section(&page.text()),
        empty_current_document_section()
    );
    app.app_server()
        .get("/evidence-document-uploads")
        .add_header("cookie", cookie)
        .await
        .assert_status_not_found();

    let (replayed, replay_logs) = app
        .capture_audit_logs(async |request_id| {
            app.app_server()
                .get(&grant_path)
                .add_header(REQUEST_ID_HEADER, request_id.to_string())
                .await
        })
        .await;
    assert_policy_unavailable(&replayed);
    assert!(replay_logs.is_empty());
}

#[tokio::test]
async fn grant_requests_conceal_invalid_unavailable_cross_workspace_and_denied_policies() {
    let app = harness::app().await;
    let owner = "auth0|policy-doc-grant-rejections-owner";
    let foreign = "auth0|policy-doc-grant-rejections-foreign";
    let owner_workspace_name = "Policy Document Grant Rejections Owner";
    let foreign_workspace_name = "Policy Document Grant Rejections Foreign";

    let scenario = ScenarioBuilder::new(&app)
        .with_user(owner)
        .with_workspace(owner, owner_workspace_name)
        .with_policy(owner_workspace_name, "Active Policy")
        .with_policy(owner_workspace_name, "Archived Policy")
        .with_user(foreign)
        .with_workspace(foreign, foreign_workspace_name)
        .with_policy(foreign_workspace_name, "Foreign Policy")
        .build()
        .await;
    let owner_workspace = scenario.workspace(owner_workspace_name);
    let active_policy_id = owner_workspace.policy("Active Policy").id;
    let archived_policy_id = owner_workspace.policy("Archived Policy").id;
    let foreign_policy_id = scenario
        .workspace(foreign_workspace_name)
        .policy("Foreign Policy")
        .id;

    let manager_token = authorize_agent_connection(
        &app,
        owner,
        "Policy Grant Rejection Manager",
        &WorkspacePermission::ALL,
    )
    .await;
    let reader_token = authorize_agent_connection(
        &app,
        owner,
        "Policy Grant Rejection Reader",
        &[WorkspacePermission::ReadControls],
    )
    .await;
    let manager = McpClient::connect(app.mcp_server(), &manager_token).await;

    let archived_grant = manager
        .call_tool(
            "manage_policy_document",
            json!({ "policy_id": archived_policy_id }),
        )
        .await;
    manager
        .call_tool("archive_policy", json!({ "policy_id": archived_policy_id }))
        .await;

    let ((errors, archived_redemption), rejection_logs) = app
        .capture_audit_logs(async |request_id| {
            let manager =
                McpClient::connect_with_request_id(app.mcp_server(), &manager_token, request_id)
                    .await;
            let reader =
                McpClient::connect_with_request_id(app.mcp_server(), &reader_token, request_id)
                    .await;
            let errors = [
                manager
                    .call_tool_error("manage_policy_document", json!({}))
                    .await,
                manager
                    .call_tool_error(
                        "manage_policy_document",
                        json!({ "policy_id": "not-a-uuid" }),
                    )
                    .await,
                manager
                    .call_tool_error(
                        "manage_policy_document",
                        json!({ "policy_id": Uuid::new_v4() }),
                    )
                    .await,
                manager
                    .call_tool_error(
                        "manage_policy_document",
                        json!({ "policy_id": archived_policy_id }),
                    )
                    .await,
                manager
                    .call_tool_error(
                        "manage_policy_document",
                        json!({ "policy_id": foreign_policy_id }),
                    )
                    .await,
                reader
                    .call_tool_error(
                        "manage_policy_document",
                        json!({ "policy_id": active_policy_id }),
                    )
                    .await,
            ];
            let archived_redemption = app
                .app_server()
                .get(&local_path(
                    archived_grant["url"]
                        .as_str()
                        .expect("archived grant URL is a string"),
                ))
                .add_header(REQUEST_ID_HEADER, request_id.to_string())
                .await;
            (errors, archived_redemption)
        })
        .await;

    assert_validation_error(
        &errors[0],
        json!([{ "field": "policy_id", "message": "is required" }]),
    );
    assert_validation_error(
        &errors[1],
        json!([{ "field": "policy_id", "message": "must be a UUID" }]),
    );
    for concealed in &errors[2..] {
        assert_not_found(concealed);
    }
    assert_policy_unavailable(&archived_redemption);
    assert!(rejection_logs.is_empty());
}

#[tokio::test]
async fn pending_upload_has_one_complete_projection_and_blocks_replacement_and_browser_archive() {
    let app = harness::app().await;
    let subject = "auth0|policy-doc-pending";
    let workspace_name = "Policy Document Pending";
    let filename = "pending-policy.txt";
    let bytes = b"pending policy bytes";

    let scenario = ScenarioBuilder::new(&app)
        .with_user(subject)
        .with_workspace(subject, workspace_name)
        .with_policy(workspace_name, "Pending Policy")
        .build()
        .await;
    let user_id = scenario.user(subject).id;
    let workspace = scenario.workspace(workspace_name);
    let workspace_id = workspace.id;
    let policy = workspace.policy("Pending Policy");

    let token = authorize_agent_connection(
        &app,
        subject,
        "Pending Policy Manager",
        &WorkspacePermission::ALL,
    )
    .await;
    let connection_id = get_agent_connection_id_for(&app, subject, "Pending Policy Manager").await;
    let client = McpClient::connect(app.mcp_server(), &token).await;
    let grant = client
        .call_tool("manage_policy_document", json!({ "policy_id": policy.id }))
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

    let empty_page = app
        .app_server()
        .get(MANAGEMENT_PATH)
        .add_header("cookie", cookie.clone())
        .await;
    empty_page.assert_status_ok();
    assert_eq!(
        current_document_section(&empty_page.text()),
        empty_current_document_section()
    );

    let mut events = app.pipeline_events().subscribe();
    let ((uploaded, interception, upload_request_id, scan_gate), upload_logs) = app
        .capture_audit_logs(async |request_id| {
            let mut scan_gate = app
                .pipeline_controls()
                .hold(DOCUMENT_SCAN_REQUESTED, request_id);
            let uploaded = app
                .app_server()
                .post(UPLOAD_PATH)
                .add_header("cookie", cookie.clone())
                .add_header(REQUEST_ID_HEADER, request_id.to_string())
                .multipart(upload_form(bytes, filename))
                .await;
            let interception = scan_gate.await_interception().await;
            (uploaded, interception, request_id, scan_gate)
        })
        .await;
    uploaded.assert_status(StatusCode::SEE_OTHER);
    assert_eq!(uploaded.header("location"), MANAGEMENT_PATH);

    let pending = client
        .call_tool("get_policy", json!({ "policy_id": policy.id }))
        .await;
    let document_id = assert_policy_projection(
        &pending,
        policy,
        Some(ExpectedDocument {
            user_id,
            filename,
            bytes,
            upload_status: "pending",
        }),
    )
    .expect("pending policy has a document");
    assert_eq!(interception.aggregate_id, document_id.to_string());
    assert_policy_audit_event(
        &upload_logs,
        ExpectedAudit {
            event_name: "policy_document.accepted",
            operation: "accept_policy_document",
            client_type: "rest",
            user_id,
            connection_id,
            workspace_id,
            request_id: upload_request_id,
            object_type: "policy_document",
            object_id: document_id,
            metadata: json!({
                "policy_id": policy.id,
                "policy_document_id": document_id,
                "lifecycle_status": "pending",
            }),
        },
    );

    let page = app
        .app_server()
        .get(MANAGEMENT_PATH)
        .add_header("cookie", cookie.clone())
        .await;
    page.assert_status_ok();
    assert_eq!(
        current_document_section(&page.text()),
        populated_current_document_section(document_id, filename, bytes.len(), "pending")
    );

    let replacement = app
        .app_server()
        .post(UPLOAD_PATH)
        .add_header("cookie", cookie.clone())
        .multipart(upload_form(b"replacement", "replacement.txt"))
        .await;
    replacement.assert_status(StatusCode::CONFLICT);
    assert_eq!(
        notice_section(&replacement.text()),
        notice("Upload failed: this policy already has a current document")
    );
    assert_eq!(
        current_document_section(&replacement.text()),
        populated_current_document_section(document_id, filename, bytes.len(), "pending")
    );

    let archive = app
        .app_server()
        .post(&archive_path(document_id))
        .add_header("cookie", cookie)
        .await;
    archive.assert_status(StatusCode::CONFLICT);
    assert_eq!(
        notice_section(&archive.text()),
        notice("Archive failed: this document is not ready to archive")
    );
    assert_eq!(
        client
            .call_tool("get_policy", json!({ "policy_id": policy.id }))
            .await,
        pending
    );

    scan_gate.release();
    assert_eq!(
        events
            .await_event(DOCUMENT_SCAN_REQUESTED, &document_id.to_string())
            .await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        events
            .await_event(DOCUMENT_FINALIZATION_REQUESTED, &document_id.to_string())
            .await,
        StatusCode::NO_CONTENT
    );
}

#[tokio::test]
async fn policy_archive_is_blocked_at_pending_and_finalizing_then_allowed_at_terminal() {
    let app = harness::app().await;
    let subject = "auth0|policy-doc-policy-archive";
    let workspace_name = "Policy Document Policy Archive";
    let bytes = b"archive gate policy bytes";

    let scenario = ScenarioBuilder::new(&app)
        .with_user(subject)
        .with_workspace(subject, workspace_name)
        .with_policy(workspace_name, "Archive Gate Policy")
        .build()
        .await;
    let user_id = scenario.user(subject).id;
    let workspace = scenario.workspace(workspace_name);
    let workspace_id = workspace.id;
    let policy = workspace.policy("Archive Gate Policy");

    let token = authorize_agent_connection(
        &app,
        subject,
        "Policy Archive Gate Manager",
        &WorkspacePermission::ALL,
    )
    .await;
    let connection_id =
        get_agent_connection_id_for(&app, subject, "Policy Archive Gate Manager").await;
    let client = McpClient::connect(app.mcp_server(), &token).await;
    let grant = client
        .call_tool("manage_policy_document", json!({ "policy_id": policy.id }))
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

    let upload_request_id = Uuid::new_v4();
    let mut scan_gate = app
        .pipeline_controls()
        .hold(DOCUMENT_SCAN_REQUESTED, upload_request_id);
    let mut finalization_gate = app
        .pipeline_controls()
        .hold(DOCUMENT_FINALIZATION_REQUESTED, upload_request_id);
    let mut events = app.pipeline_events().subscribe();
    app.app_server()
        .post(UPLOAD_PATH)
        .add_header("cookie", cookie)
        .add_header(REQUEST_ID_HEADER, upload_request_id.to_string())
        .multipart(upload_form(bytes, "archive-gate.txt"))
        .await
        .assert_status(StatusCode::SEE_OTHER);
    let pending_interception = scan_gate.await_interception().await;
    let pending_id = pending_interception.aggregate_id.clone();

    let (pending_error, pending_logs) = app
        .capture_audit_logs(async |request_id| {
            McpClient::connect_with_request_id(app.mcp_server(), &token, request_id)
                .await
                .call_tool_error("archive_policy", json!({ "policy_id": policy.id }))
                .await
        })
        .await;
    assert_policy_document_in_progress(&pending_error);
    assert!(pending_logs.is_empty());
    let pending = client
        .call_tool("get_policy", json!({ "policy_id": policy.id }))
        .await;
    assert_eq!(pending["document"]["upload_status"], "pending");

    scan_gate.release();
    assert_eq!(
        events
            .await_event(DOCUMENT_SCAN_REQUESTED, &pending_id)
            .await,
        StatusCode::NO_CONTENT
    );
    let finalizing_interception = finalization_gate.await_interception().await;
    assert_eq!(finalizing_interception.aggregate_id, pending_id);
    let finalizing = client
        .call_tool("get_policy", json!({ "policy_id": policy.id }))
        .await;
    assert_eq!(finalizing["document"]["upload_status"], "finalizing");

    let (finalizing_error, finalizing_logs) = app
        .capture_audit_logs(async |request_id| {
            McpClient::connect_with_request_id(app.mcp_server(), &token, request_id)
                .await
                .call_tool_error("archive_policy", json!({ "policy_id": policy.id }))
                .await
        })
        .await;
    assert_policy_document_in_progress(&finalizing_error);
    assert!(finalizing_logs.is_empty());

    finalization_gate.release();
    assert_eq!(
        events
            .await_event(DOCUMENT_FINALIZATION_REQUESTED, &pending_id)
            .await,
        StatusCode::NO_CONTENT
    );
    let terminal = client
        .call_tool("get_policy", json!({ "policy_id": policy.id }))
        .await;
    assert_eq!(terminal["document"]["upload_status"], "uploaded");

    let ((archived, archive_request_id), archive_logs) = app
        .capture_audit_logs(async |request_id| {
            let archived = McpClient::connect_with_request_id(app.mcp_server(), &token, request_id)
                .await
                .call_tool("archive_policy", json!({ "policy_id": policy.id }))
                .await;
            (archived, request_id)
        })
        .await;
    assert_eq!(
        object_keys(&archived),
        ["archived_at", "policy_id"].into_iter().collect()
    );
    assert_eq!(archived["policy_id"], policy.id.to_string());
    assert_rfc3339(&archived["archived_at"]);
    assert_policy_audit_event(
        &archive_logs,
        ExpectedAudit {
            event_name: "policy.archived",
            operation: "archive_policy",
            client_type: "mcp",
            user_id,
            connection_id,
            workspace_id,
            request_id: archive_request_id,
            object_type: "policy",
            object_id: policy.id,
            metadata: json!({ "policy_id": policy.id }),
        },
    );
    assert_not_found(
        &client
            .call_tool_error("get_policy", json!({ "policy_id": policy.id }))
            .await,
    );
}

#[tokio::test]
async fn clean_document_downloads_archives_is_concealed_and_allows_one_replacement() {
    let app = harness::app().await;
    let subject = "auth0|policy-doc-clean-lifecycle";
    let workspace_name = "Policy Document Clean Lifecycle";
    let filename = "security-policy.txt";
    let bytes = b"downloadable security policy bytes";

    let scenario = ScenarioBuilder::new(&app)
        .with_user(subject)
        .with_workspace(subject, workspace_name)
        .with_policy(workspace_name, "Security Policy")
        .build()
        .await;
    let user_id = scenario.user(subject).id;
    let workspace = scenario.workspace(workspace_name);
    let workspace_id = workspace.id;
    let policy = workspace.policy("Security Policy");

    let token = authorize_agent_connection(
        &app,
        subject,
        "Clean Policy Document Manager",
        &WorkspacePermission::ALL,
    )
    .await;
    let connection_id =
        get_agent_connection_id_for(&app, subject, "Clean Policy Document Manager").await;
    let client = McpClient::connect(app.mcp_server(), &token).await;
    let grant = client
        .call_tool("manage_policy_document", json!({ "policy_id": policy.id }))
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
        .post(UPLOAD_PATH)
        .add_header("cookie", cookie.clone())
        .multipart(upload_form(bytes, filename))
        .await
        .assert_status(StatusCode::SEE_OTHER);
    let submitted = client
        .call_tool("get_policy", json!({ "policy_id": policy.id }))
        .await;
    let document_id = Uuid::parse_str(
        submitted["document"]["id"]
            .as_str()
            .expect("document id is a string"),
    )
    .expect("document id is a UUID");
    assert_eq!(
        events
            .await_event(DOCUMENT_SCAN_REQUESTED, &document_id.to_string())
            .await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        events
            .await_event(DOCUMENT_FINALIZATION_REQUESTED, &document_id.to_string())
            .await,
        StatusCode::NO_CONTENT
    );

    let settled = client
        .call_tool("get_policy", json!({ "policy_id": policy.id }))
        .await;
    assert_eq!(
        assert_policy_projection(
            &settled,
            policy,
            Some(ExpectedDocument {
                user_id,
                filename,
                bytes,
                upload_status: "uploaded",
            }),
        ),
        Some(document_id)
    );
    let page = app
        .app_server()
        .get(MANAGEMENT_PATH)
        .add_header("cookie", cookie.clone())
        .await;
    page.assert_status_ok();
    assert_eq!(
        current_document_section(&page.text()),
        populated_current_document_section(document_id, filename, bytes.len(), "uploaded")
    );

    let ((download, download_request_id), download_logs) = app
        .capture_audit_logs(async |request_id| {
            let response = app
                .app_server()
                .get(&download_path(document_id))
                .add_header("cookie", cookie.clone())
                .add_header(REQUEST_ID_HEADER, request_id.to_string())
                .await;
            (response, request_id)
        })
        .await;
    download.assert_status_ok();
    assert_eq!(download.as_bytes().as_ref(), bytes);
    assert_eq!(download.header("content-type"), "text/plain");
    assert_eq!(download.header("content-length"), bytes.len().to_string());
    assert_eq!(
        download.header("content-disposition"),
        format!("document; filename=\"{filename}\"")
    );
    assert_eq!(download.header("cache-control"), "private, no-store");
    assert_eq!(download.header("referrer-policy"), "no-referrer");
    assert_policy_audit_event(
        &download_logs,
        ExpectedAudit {
            event_name: "policy_document.downloaded",
            operation: "download_policy_document_via_upload_session",
            client_type: "rest",
            user_id,
            connection_id,
            workspace_id,
            request_id: download_request_id,
            object_type: "policy_document",
            object_id: document_id,
            metadata: json!({
                "policy_document_id": document_id,
                "policy_id": policy.id,
            }),
        },
    );

    let ((archived, archive_request_id), archive_logs) = app
        .capture_audit_logs(async |request_id| {
            let response = app
                .app_server()
                .post(&archive_path(document_id))
                .add_header("cookie", cookie.clone())
                .add_header(REQUEST_ID_HEADER, request_id.to_string())
                .await;
            (response, request_id)
        })
        .await;
    archived.assert_status(StatusCode::SEE_OTHER);
    assert_eq!(archived.header("location"), MANAGEMENT_PATH);
    assert_policy_audit_event(
        &archive_logs,
        ExpectedAudit {
            event_name: "policy_document.archived",
            operation: "archive_policy_document",
            client_type: "rest",
            user_id,
            connection_id,
            workspace_id,
            request_id: archive_request_id,
            object_type: "policy_document",
            object_id: document_id,
            metadata: json!({
                "policy_document_id": document_id,
                "policy_id": policy.id,
            }),
        },
    );
    assert_policy_unavailable(
        &app.app_server()
            .get(&download_path(document_id))
            .add_header("cookie", cookie.clone())
            .await,
    );
    let empty = client
        .call_tool("get_policy", json!({ "policy_id": policy.id }))
        .await;
    assert_eq!(assert_policy_projection(&empty, policy, None), None);

    let replacement_bytes = b"replacement policy bytes";
    let mut replacement_events = app.pipeline_events().subscribe();
    app.app_server()
        .post(UPLOAD_PATH)
        .add_header("cookie", cookie)
        .multipart(upload_form(replacement_bytes, "replacement-policy.txt"))
        .await
        .assert_status(StatusCode::SEE_OTHER);
    let replacement_pending = client
        .call_tool("get_policy", json!({ "policy_id": policy.id }))
        .await;
    let replacement_id = Uuid::parse_str(
        replacement_pending["document"]["id"]
            .as_str()
            .expect("replacement document id is a string"),
    )
    .expect("replacement document id is a UUID");
    assert_ne!(replacement_id, document_id);
    assert_eq!(
        replacement_events
            .await_event(DOCUMENT_SCAN_REQUESTED, &replacement_id.to_string())
            .await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        replacement_events
            .await_event(DOCUMENT_FINALIZATION_REQUESTED, &replacement_id.to_string(),)
            .await,
        StatusCode::NO_CONTENT
    );
    let replacement = client
        .call_tool("get_policy", json!({ "policy_id": policy.id }))
        .await;
    assert_eq!(
        assert_policy_projection(
            &replacement,
            policy,
            Some(ExpectedDocument {
                user_id,
                filename: "replacement-policy.txt",
                bytes: replacement_bytes,
                upload_status: "uploaded",
            }),
        ),
        Some(replacement_id)
    );
}

#[tokio::test]
async fn eicar_document_settles_as_contains_virus_renders_upload_failed_and_is_concealed() {
    let app = harness::app().await;
    let subject = "auth0|policy-doc-eicar";
    let workspace_name = "Policy Document Eicar";
    let filename = "eicar-policy.txt";

    let scenario = ScenarioBuilder::new(&app)
        .with_user(subject)
        .with_workspace(subject, workspace_name)
        .with_policy(workspace_name, "Eicar Policy")
        .build()
        .await;
    let user_id = scenario.user(subject).id;
    let policy = scenario.workspace(workspace_name).policy("Eicar Policy");

    let token = authorize_agent_connection(
        &app,
        subject,
        "Eicar Policy Document Manager",
        &WorkspacePermission::ALL,
    )
    .await;
    let client = McpClient::connect(app.mcp_server(), &token).await;
    let grant = client
        .call_tool("manage_policy_document", json!({ "policy_id": policy.id }))
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
        .post(UPLOAD_PATH)
        .add_header("cookie", cookie.clone())
        .multipart(upload_form(EICAR, filename))
        .await
        .assert_status(StatusCode::SEE_OTHER);
    let pending = client
        .call_tool("get_policy", json!({ "policy_id": policy.id }))
        .await;
    let document_id = Uuid::parse_str(
        pending["document"]["id"]
            .as_str()
            .expect("document id is a string"),
    )
    .expect("document id is a UUID");
    assert_eq!(
        events
            .await_event(DOCUMENT_SCAN_REQUESTED, &document_id.to_string())
            .await,
        StatusCode::NO_CONTENT
    );

    let settled = client
        .call_tool("get_policy", json!({ "policy_id": policy.id }))
        .await;
    assert_eq!(
        assert_policy_projection(
            &settled,
            policy,
            Some(ExpectedDocument {
                user_id,
                filename,
                bytes: EICAR,
                upload_status: "contains_virus",
            }),
        ),
        Some(document_id)
    );
    let page = app
        .app_server()
        .get(MANAGEMENT_PATH)
        .add_header("cookie", cookie.clone())
        .await;
    page.assert_status_ok();
    assert_eq!(
        current_document_section(&page.text()),
        populated_current_document_section(document_id, filename, EICAR.len(), "contains_virus")
    );
    assert_policy_unavailable(
        &app.app_server()
            .get(&download_path(document_id))
            .add_header("cookie", cookie)
            .await,
    );
}

#[tokio::test]
async fn concurrent_policy_uploads_choose_one_redirected_winner_and_one_conflict() {
    let app = harness::app().await;
    let subject = "auth0|policy-doc-concurrent";
    let workspace_name = "Policy Document Concurrent";

    let scenario = ScenarioBuilder::new(&app)
        .with_user(subject)
        .with_workspace(subject, workspace_name)
        .with_policy(workspace_name, "Concurrent Policy")
        .build()
        .await;
    let user_id = scenario.user(subject).id;
    let policy = scenario
        .workspace(workspace_name)
        .policy("Concurrent Policy");

    let token = authorize_agent_connection(
        &app,
        subject,
        "Concurrent Policy Document Manager",
        &WorkspacePermission::ALL,
    )
    .await;
    let client = McpClient::connect(app.mcp_server(), &token).await;
    let grant = client
        .call_tool("manage_policy_document", json!({ "policy_id": policy.id }))
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
    let first = app
        .app_server()
        .post(UPLOAD_PATH)
        .add_header("cookie", cookie.clone())
        .add_header(REQUEST_ID_HEADER, Uuid::new_v4().to_string())
        .multipart(upload_form(b"first concurrent policy", "first-policy.txt"));
    let second = app
        .app_server()
        .post(UPLOAD_PATH)
        .add_header("cookie", cookie.clone())
        .add_header(REQUEST_ID_HEADER, Uuid::new_v4().to_string())
        .multipart(upload_form(
            b"second concurrent policy",
            "second-policy.txt",
        ));
    let (first, second) = tokio::join!(first, second);
    let statuses = [first.status_code(), second.status_code()];
    assert_eq!(
        statuses
            .iter()
            .filter(|status| **status == StatusCode::SEE_OTHER)
            .count(),
        1
    );
    assert_eq!(
        statuses
            .iter()
            .filter(|status| **status == StatusCode::CONFLICT)
            .count(),
        1
    );
    let conflict = if first.status_code() == StatusCode::CONFLICT {
        &first
    } else {
        &second
    };
    assert_eq!(
        notice_section(&conflict.text()),
        notice("Upload failed: this policy already has a current document")
    );

    let current = client
        .call_tool("get_policy", json!({ "policy_id": policy.id }))
        .await;
    let filename = current["document"]["filename"]
        .as_str()
        .expect("winner filename is a string");
    let (winner_bytes, winner_filename): (&[u8], &str) = match filename {
        "first-policy.txt" => (b"first concurrent policy", "first-policy.txt"),
        "second-policy.txt" => (b"second concurrent policy", "second-policy.txt"),
        other => panic!("unexpected concurrent upload winner: {other}"),
    };
    let document_id = Uuid::parse_str(
        current["document"]["id"]
            .as_str()
            .expect("winner document id is a string"),
    )
    .expect("winner document id is a UUID");
    assert_eq!(
        events
            .await_event(DOCUMENT_SCAN_REQUESTED, &document_id.to_string())
            .await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        events
            .await_event(DOCUMENT_FINALIZATION_REQUESTED, &document_id.to_string())
            .await,
        StatusCode::NO_CONTENT
    );

    let settled = client
        .call_tool("get_policy", json!({ "policy_id": policy.id }))
        .await;
    assert_eq!(
        assert_policy_projection(
            &settled,
            policy,
            Some(ExpectedDocument {
                user_id,
                filename: winner_filename,
                bytes: winner_bytes,
                upload_status: "uploaded",
            }),
        ),
        Some(document_id)
    );
    let page = app
        .app_server()
        .get(MANAGEMENT_PATH)
        .add_header("cookie", cookie)
        .await;
    page.assert_status_ok();
    assert_eq!(
        current_document_section(&page.text()),
        populated_current_document_section(
            document_id,
            winner_filename,
            winner_bytes.len(),
            "uploaded",
        )
    );
}

#[tokio::test]
async fn browser_routes_reject_invalid_sessions_forms_tokens_and_cross_policy_document_ids() {
    let app = harness::app().await;
    let subject = "auth0|policy-doc-route-rejections";
    let workspace_name = "Policy Document Route Rejections";

    let scenario = ScenarioBuilder::new(&app)
        .with_user(subject)
        .with_workspace(subject, workspace_name)
        .with_policy(workspace_name, "Validation Policy")
        .with_policy(workspace_name, "Other Policy")
        .build()
        .await;
    let user_id = scenario.user(subject).id;
    let workspace = scenario.workspace(workspace_name);
    let policy = workspace.policy("Validation Policy");
    let other_policy = workspace.policy("Other Policy");

    let token = authorize_agent_connection(
        &app,
        subject,
        "Policy Route Rejection Manager",
        &WorkspacePermission::ALL,
    )
    .await;
    let client = McpClient::connect(app.mcp_server(), &token).await;
    let grant = client
        .call_tool("manage_policy_document", json!({ "policy_id": policy.id }))
        .await;
    let grant_path = local_path(grant["url"].as_str().expect("grant URL is a string"));
    let tampered = app.app_server().get(&format!("{grant_path}x")).await;
    assert_policy_unavailable(&tampered);
    let redeemed = app.app_server().get(&grant_path).await;
    redeemed.assert_status(StatusCode::SEE_OTHER);
    let cookie = request_cookie(
        redeemed
            .header("set-cookie")
            .to_str()
            .expect("cookie header is text"),
    );

    let baseline = client
        .call_tool("get_policy", json!({ "policy_id": policy.id }))
        .await;
    assert_eq!(assert_policy_projection(&baseline, policy, None), None);
    let missing_page = app.app_server().get(MANAGEMENT_PATH).await;
    assert_policy_unavailable(&missing_page);
    let malformed_page = app
        .app_server()
        .get(MANAGEMENT_PATH)
        .add_header(
            "cookie",
            "proofplane_policy_document_upload_session=not-a-token",
        )
        .await;
    assert_policy_unavailable(&malformed_page);
    let missing_upload = app
        .app_server()
        .post(UPLOAD_PATH)
        .multipart(upload_form(b"unavailable", "unavailable.txt"))
        .await;
    assert_policy_unavailable(&missing_upload);

    let invalid_forms = [
        (
            MultipartForm::new().add_part("note", Part::text("not a file")),
            "Upload failed: multipart upload field for file must have correct name",
        ),
        (
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
            "Upload failed: browser upload requires exactly one file field",
        ),
        (
            upload_form(b"invalid filename", "path/file.txt"),
            "Upload failed: document filename contains unsupported characters",
        ),
    ];
    for (form, message) in invalid_forms {
        let response = app
            .app_server()
            .post(UPLOAD_PATH)
            .add_header("cookie", cookie.clone())
            .multipart(form)
            .await;
        response.assert_status(StatusCode::BAD_REQUEST);
        assert_eq!(notice_section(&response.text()), notice(message));
        assert_eq!(
            current_document_section(&response.text()),
            empty_current_document_section()
        );
    }
    assert_eq!(
        client
            .call_tool("get_policy", json!({ "policy_id": policy.id }))
            .await,
        baseline
    );

    let other_grant = client
        .call_tool(
            "manage_policy_document",
            json!({ "policy_id": other_policy.id }),
        )
        .await;
    let other_redeemed = app
        .app_server()
        .get(&local_path(
            other_grant["url"]
                .as_str()
                .expect("other grant URL is a string"),
        ))
        .await;
    other_redeemed.assert_status(StatusCode::SEE_OTHER);
    let other_cookie = request_cookie(
        other_redeemed
            .header("set-cookie")
            .to_str()
            .expect("other cookie header is text"),
    );
    let other_bytes = b"other policy bytes";
    let mut events = app.pipeline_events().subscribe();
    app.app_server()
        .post(UPLOAD_PATH)
        .add_header("cookie", other_cookie)
        .multipart(upload_form(other_bytes, "other-policy.txt"))
        .await
        .assert_status(StatusCode::SEE_OTHER);
    let other_pending = client
        .call_tool("get_policy", json!({ "policy_id": other_policy.id }))
        .await;
    let other_document_id = Uuid::parse_str(
        other_pending["document"]["id"]
            .as_str()
            .expect("other document id is a string"),
    )
    .expect("other document id is a UUID");
    assert_eq!(
        events
            .await_event(DOCUMENT_SCAN_REQUESTED, &other_document_id.to_string())
            .await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        events
            .await_event(
                DOCUMENT_FINALIZATION_REQUESTED,
                &other_document_id.to_string(),
            )
            .await,
        StatusCode::NO_CONTENT
    );
    let other_settled = client
        .call_tool("get_policy", json!({ "policy_id": other_policy.id }))
        .await;
    assert_eq!(
        assert_policy_projection(
            &other_settled,
            other_policy,
            Some(ExpectedDocument {
                user_id,
                filename: "other-policy.txt",
                bytes: other_bytes,
                upload_status: "uploaded",
            }),
        ),
        Some(other_document_id)
    );

    for response in [
        app.app_server()
            .get(&download_path(other_document_id))
            .add_header("cookie", cookie.clone())
            .await,
        app.app_server()
            .post(&archive_path(other_document_id))
            .add_header("cookie", cookie)
            .await,
    ] {
        assert_policy_unavailable(&response);
    }
    assert_eq!(
        client
            .call_tool("get_policy", json!({ "policy_id": policy.id }))
            .await,
        baseline
    );
    assert_eq!(
        client
            .call_tool("get_policy", json!({ "policy_id": other_policy.id }))
            .await,
        other_settled
    );
}

struct ExpectedDocument<'a> {
    user_id: Uuid,
    filename: &'a str,
    bytes: &'a [u8],
    upload_status: &'a str,
}

#[track_caller]
fn assert_policy_projection(
    projection: &Value,
    policy: &TestPolicy,
    expected_document: Option<ExpectedDocument<'_>>,
) -> Option<Uuid> {
    assert_eq!(
        object_keys(projection),
        [
            "controls",
            "created_at",
            "description",
            "document",
            "id",
            "name",
            "updated_at",
        ]
        .into_iter()
        .collect()
    );
    assert_eq!(projection["id"], policy.id.to_string());
    assert_eq!(projection["name"], policy.name);
    assert_eq!(projection["description"], json!(policy.description));
    assert_eq!(projection["controls"], json!([]));
    assert_rfc3339(&projection["created_at"]);
    assert_rfc3339(&projection["updated_at"]);

    let Some(expected) = expected_document else {
        assert_eq!(projection["document"], Value::Null);
        return None;
    };
    let document = &projection["document"];
    assert_eq!(
        object_keys(document),
        [
            "checksum_crc32c",
            "checksum_sha256",
            "content_length",
            "content_type",
            "created_at",
            "created_by_user_id",
            "filename",
            "id",
            "upload_status",
        ]
        .into_iter()
        .collect()
    );
    let document_id = Uuid::parse_str(
        document["id"]
            .as_str()
            .expect("policy document id is a string"),
    )
    .expect("policy document id is a UUID");
    assert_eq!(document["created_by_user_id"], expected.user_id.to_string());
    assert_eq!(document["filename"], expected.filename);
    assert_eq!(document["content_type"], "text/plain");
    assert_eq!(document["content_length"], expected.bytes.len());
    assert_eq!(
        document["checksum_sha256"],
        hex::encode(Sha256::digest(expected.bytes))
    );
    assert_eq!(
        document["checksum_crc32c"],
        BASE64_STANDARD.encode(crc32c::crc32c(expected.bytes).to_be_bytes())
    );
    assert_eq!(document["upload_status"], expected.upload_status);
    assert_rfc3339(&document["created_at"]);
    Some(document_id)
}

fn archive_path(document_id: Uuid) -> String {
    format!("{UPLOAD_PATH}/{document_id}/archive")
}

fn download_path(document_id: Uuid) -> String {
    format!("{UPLOAD_PATH}/{document_id}/download")
}

fn current_document_section(html: &str) -> String {
    html_section(
        html,
        r#"<section class="panel" aria-labelledby="current-document">"#,
    )
}

fn notice_section(html: &str) -> String {
    html_section(html, r#"<section class="notice" role="alert">"#)
}

fn html_section(html: &str, opening: &str) -> String {
    let start = html.find(opening).expect("HTML section opens");
    let end = html[start..]
        .find("</section>")
        .map(|offset| start + offset + "</section>".len())
        .expect("HTML section closes");
    html[start..end].to_owned()
}

fn empty_current_document_section() -> String {
    r#"<section class="panel" aria-labelledby="current-document"><div class="panel-heading"><h2 id="current-document">Current document</h2><span class="count">None</span></div><div class="empty"><svg viewBox="0 0 24 24" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="1.7"><path d="M7 7.5V6a5 5 0 0 1 10 0v9a7 7 0 0 1-14 0V7a3 3 0 0 1 6 0v8a1 1 0 0 1-2 0V8.5"/></svg><div><strong>No policy document yet</strong><p>Add the document for this policy.</p></div></div></section>"#.to_owned()
}

fn populated_current_document_section(
    document_id: Uuid,
    filename: &str,
    content_length: usize,
    status: &str,
) -> String {
    let status_label = match status {
        "pending" => "Uploading",
        "finalizing" => "Scanning",
        "uploaded" => "Uploaded",
        "contains_virus" | "failed" => "Upload failed",
        other => panic!("unsupported policy document status: {other}"),
    };
    let actions = match status {
        "uploaded" => format!(
            r#"<div class="actions"><a class="button icon-button" href="/policy-document-uploads/files/{document_id}/download" aria-label="Download policy document"><svg viewBox="0 0 24 24" aria-hidden="true" fill="none" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 3v12"/><path d="m7 10 5 5 5-5"/><path d="M5 21h14"/></svg><span class="sr-only">Download</span></a><form method="post" action="/policy-document-uploads/files/{document_id}/archive" onsubmit="return confirm('Archive this policy document?');"><button class="icon-button danger-button" type="submit" aria-label="Archive policy document"><svg viewBox="0 0 24 24" aria-hidden="true" fill="none" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M3 6h18"/><path d="M8 6V4h8v2"/><path d="M19 6l-1 14H6L5 6"/><path d="M10 11v5"/><path d="M14 11v5"/></svg><span class="sr-only">Archive</span></button></form></div>"#
        ),
        "contains_virus" | "failed" => format!(
            r#"<div class="actions"><form method="post" action="/policy-document-uploads/files/{document_id}/archive" onsubmit="return confirm('Archive this policy document?');"><button class="icon-button danger-button" type="submit" aria-label="Archive policy document"><svg viewBox="0 0 24 24" aria-hidden="true" fill="none" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M3 6h18"/><path d="M8 6V4h8v2"/><path d="M19 6l-1 14H6L5 6"/><path d="M10 11v5"/><path d="M14 11v5"/></svg><span class="sr-only">Archive</span></button></form></div>"#
        ),
        "pending" | "finalizing" => String::new(),
        other => panic!("unsupported policy document status: {other}"),
    };
    format!(
        r#"<section class="panel" aria-labelledby="current-document"><div class="panel-heading"><h2 id="current-document">Current document</h2><span class="count">1 current</span></div><table><thead><tr><th>Filename</th><th>Size</th><th>Status</th><th>Actions</th></tr></thead><tbody><tr><td class="filename" data-label="File">{filename}</td><td data-label="Size">{content_length} B</td><td data-label="Status"><span class="status">{status_label}</span></td><td data-label="Actions">{actions}</td></tr></tbody></table></section>"#
    )
}

fn notice(message: &str) -> String {
    format!(
        r#"<section class="notice" role="alert"><strong>{message}</strong><p>Review the message above, then try again.</p></section>"#
    )
}

fn assert_policy_unavailable(response: &TestResponse) {
    response.assert_status_not_found();
    assert_eq!(response.text(), unavailable_page());
}

fn unavailable_page() -> &'static str {
    r#"<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><title>Policy document link unavailable</title><style>:root{color-scheme:dark;--canvas:oklch(17% .012 170);--line:oklch(39% .018 170);--ink:oklch(94% .01 150);--muted:oklch(76% .015 155);--accent:oklch(78% .09 174)}*{box-sizing:border-box}body{margin:0;min-height:100vh;background:var(--canvas);color:var(--ink);font-family:Inter,ui-sans-serif,system-ui,-apple-system,BlinkMacSystemFont,"Segoe UI",sans-serif}header{border-bottom:1px solid var(--line);padding:14px max(16px,calc((100vw - 1080px)/2));font-size:.78rem;font-weight:750;letter-spacing:.08em}header span{color:var(--muted);font-weight:550}main{width:min(690px,calc(100% - 32px));min-height:calc(100vh - 49px);margin:0 auto;display:grid;align-content:center;padding:40px 0}.eyebrow{margin:0 0 10px;color:var(--accent);font-size:.78rem;font-weight:700}h1{max-width:20ch;margin:0 0 12px;font-size:clamp(1.8rem,4vw,2.5rem);line-height:1.05}p{max-width:58ch;margin:0;color:var(--muted);line-height:1.55}</style></head><body><header>PROOFPLANE <span>/ POLICY DOCUMENTS</span></header><main><p class="eyebrow">LINK UNAVAILABLE</p><h1>This policy document link is no longer available</h1><p>Return to your MCP client and request a new policy document link.</p></main></body></html>"#
}

fn assert_policy_cookie(set_cookie: &str) {
    let parts = set_cookie.split("; ").collect::<Vec<_>>();
    assert_eq!(parts.len(), 6);
    assert!(parts[0].starts_with("proofplane_policy_document_upload_session=v4.local."));
    assert_eq!(parts[1], "HttpOnly");
    assert_eq!(parts[2], "SameSite=Lax");
    assert_eq!(parts[3], "Path=/policy-document-uploads");
    let max_age = parts[4]
        .strip_prefix("Max-Age=")
        .expect("cookie has Max-Age")
        .parse::<i64>()
        .expect("cookie Max-Age is an integer");
    assert!((1..=300).contains(&max_age));
    assert_eq!(parts[5], "Secure");
}

#[track_caller]
fn assert_policy_document_in_progress(error: &McpError) {
    assert_eq!(error.code, ErrorCode(-32000));
    assert_eq!(
        error.data,
        json!({
            "problem": {
                "code": "policy_document_in_progress",
                "message": "policy cannot be archived while its document is being processed",
            }
        })
    );
}

struct ExpectedAudit<'a> {
    event_name: &'a str,
    operation: &'a str,
    client_type: &'a str,
    user_id: Uuid,
    connection_id: Uuid,
    workspace_id: Uuid,
    request_id: Uuid,
    object_type: &'a str,
    object_id: Uuid,
    metadata: Value,
}

#[track_caller]
fn assert_policy_audit_event(records: &[Value], expected: ExpectedAudit<'_>) {
    assert_eq!(records.len(), 1);
    let record = &records[0];
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
    let expected_keys: BTreeSet<_> = [
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
    .collect();
    assert_eq!(object_keys(fields), expected_keys);
    assert_eq!(fields["type"], "audit_log");
    Uuid::parse_str(fields["event_id"].as_str().expect("event id is a string"))
        .expect("event id is a UUID");
    assert_eq!(fields["event_name"], expected.event_name);
    assert_eq!(fields["outcome"], "success");
    assert_eq!(fields["actor_type"], "agent_connection");
    assert_eq!(fields["user_id"], expected.user_id.to_string());
    assert_eq!(
        fields["agent_connection_id"],
        expected.connection_id.to_string()
    );
    assert_eq!(fields["client_type"], expected.client_type);
    assert_eq!(fields["operation"], expected.operation);
    assert_eq!(fields["workspace_id"], expected.workspace_id.to_string());
    assert_eq!(fields["request_id"], expected.request_id.to_string());
    assert_eq!(fields["object_type"], expected.object_type);
    assert_eq!(fields["object_id"], expected.object_id.to_string());
    assert_eq!(
        serde_json::from_str::<Value>(
            fields["metadata"]
                .as_str()
                .expect("audit metadata is serialized JSON"),
        )
        .expect("audit metadata parses"),
        expected.metadata
    );
}
