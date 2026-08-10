use axum_test::TestResponse;
use http::{header::SET_COOKIE, StatusCode};
use proofplane::worker::{DOCUMENT_FINALIZATION_REQUESTED, DOCUMENT_SCAN_REQUESTED};
use proofplane::{domain::WorkspacePermission, routes::request_context::REQUEST_ID_HEADER};
use serde_json::{json, Value};
use url::Url;
use uuid::Uuid;

use crate::support::{
    auditor_access::{authenticate_auditor, invite_token},
    clamd::{EICAR, ERROR_TRIGGER},
    documents::upload_form,
    evidence_documents::archive_path_for_ids as evidence_archive_path,
    harness,
    http::{local_path, request_cookie},
    json::{assert_rfc3339, object_keys},
    mcp::McpClient,
    oauth::authorize_agent_connection,
    scenario::{types::TestEvidenceSubmission, ScenarioBuilder},
};

const AUDITOR_PORTAL_PREFIX: &str = "/auditor-access/portal";
const EVIDENCE_UPLOAD_PATH: &str = "/evidence-document-uploads/files";
const POLICY_UPLOAD_PATH: &str = "/policy-document-uploads/files";

#[tokio::test]
async fn eligible_evidence_download_streams_safe_bytes_and_logs_one_secret_free_audit_event() {
    let app = harness::app().await;
    let subject = "auth0|auditor-evidence-download-success";
    let workspace_name = "Auditor Evidence Download Success";
    let evidence_title = "Downloadable auditor evidence";
    let auditor_email = "auditor-evidence-download-success@example.com";
    let filename = "Auditor evidence packet.txt";
    let bytes = b"auditor evidence download bytes";

    let scenario = ScenarioBuilder::new(&app)
        .with_user(subject)
        .with_workspace(subject, workspace_name)
        .with_evidence(workspace_name, evidence_title)
        .with_evidence_document(
            workspace_name,
            evidence_title,
            filename,
            bytes,
            "2026-01-01T00:00:00.000Z",
            "2026-03-31T23:59:59.000Z",
        )
        .build()
        .await;
    let workspace = scenario.workspace(workspace_name);
    let workspace_id = workspace.id;
    let submission = workspace.evidence(evidence_title).submission(filename);

    let token = authorize_agent_connection(
        &app,
        subject,
        "Auditor Evidence Download Access",
        &[WorkspacePermission::ManageAuditorAccess],
    )
    .await;
    let client = McpClient::connect(app.mcp_server(), &token).await;
    let access = client
        .call_tool(
            "create_auditor_access_link",
            json!({
                "email": auditor_email,
                "expires_at": "2099-01-01T00:00:00Z",
                "period_start": "2026-01-01T00:00:00Z",
                "period_end": "2026-03-31T23:59:59Z",
            }),
        )
        .await;
    let invite_url = Url::parse(access["url"].as_str().expect("auditor URL is text"))
        .expect("auditor URL parses");
    let invite_token = invite_token(&invite_url);
    app.app_server()
        .get(&local_path(invite_url.as_str()))
        .await
        .assert_status_ok();
    let auditor_cookie = authenticate_auditor(
        &app,
        workspace_id,
        &invite_token,
        "auth0|auditor-evidence-download-success-identity",
        auditor_email,
    )
    .await;

    let ((response, request_id), logs) = app
        .capture_audit_logs(async |request_id| {
            let response = app
                .app_server()
                .get(&evidence_download_path(
                    submission.id,
                    submission.document_id,
                ))
                .add_header("cookie", auditor_cookie)
                .add_header(REQUEST_ID_HEADER, request_id.to_string())
                .await;
            (response, request_id)
        })
        .await;

    assert_safe_download(&response, bytes, filename);
    assert_eq!(logs.len(), 1);
    assert_download_audit_event(
        &logs[0],
        ExpectedDownloadAudit {
            event_name: "auditor_document.downloaded",
            operation: "download_auditor_document",
            workspace_id,
            request_id,
            object_type: "evidence_document",
            object_id: submission.document_id,
            metadata: json!({
                "auditor_email": auditor_email,
                "evidence_document_id": submission.document_id,
                "evidence_submission_id": submission.id,
            }),
        },
    );
}

#[tokio::test]
async fn eligible_policy_download_streams_safe_bytes_and_logs_one_secret_free_audit_event() {
    let app = harness::app().await;
    let subject = "auth0|auditor-policy-download-success";
    let workspace_name = "Auditor Policy Download Success";
    let policy_name = "Downloadable Auditor Policy";
    let auditor_email = "auditor-policy-download-success@example.com";
    let filename = "Auditor policy packet.txt";
    let bytes = b"auditor policy download bytes";

    let scenario = ScenarioBuilder::new(&app)
        .with_user(subject)
        .with_workspace(subject, workspace_name)
        .with_policy(workspace_name, policy_name)
        .with_policy_document(workspace_name, policy_name, filename, bytes)
        .build()
        .await;
    let workspace = scenario.workspace(workspace_name);
    let workspace_id = workspace.id;
    let policy = workspace.policy(policy_name);
    let document = policy.document();

    let token = authorize_agent_connection(
        &app,
        subject,
        "Auditor Policy Download Access",
        &[WorkspacePermission::ManageAuditorAccess],
    )
    .await;
    let client = McpClient::connect(app.mcp_server(), &token).await;
    let access = client
        .call_tool(
            "create_auditor_access_link",
            json!({
                "email": auditor_email,
                "expires_at": "2099-01-01T00:00:00Z",
                "period_start": "2026-01-01T00:00:00Z",
                "period_end": "2026-03-31T23:59:59Z",
            }),
        )
        .await;
    let invite_url = Url::parse(access["url"].as_str().expect("auditor URL is text"))
        .expect("auditor URL parses");
    let invite_token = invite_token(&invite_url);
    app.app_server()
        .get(&local_path(invite_url.as_str()))
        .await
        .assert_status_ok();
    let auditor_cookie = authenticate_auditor(
        &app,
        workspace_id,
        &invite_token,
        "auth0|auditor-policy-download-success-identity",
        auditor_email,
    )
    .await;

    let ((response, request_id), logs) = app
        .capture_audit_logs(async |request_id| {
            let response = app
                .app_server()
                .get(&policy_download_path(policy.id, document.document_id))
                .add_header("cookie", auditor_cookie)
                .add_header(REQUEST_ID_HEADER, request_id.to_string())
                .await;
            (response, request_id)
        })
        .await;

    assert_safe_download(&response, bytes, filename);
    assert_eq!(logs.len(), 1);
    assert_download_audit_event(
        &logs[0],
        ExpectedDownloadAudit {
            event_name: "auditor_policy_document.downloaded",
            operation: "download_auditor_policy_document",
            workspace_id,
            request_id,
            object_type: "policy_document",
            object_id: document.document_id,
            metadata: json!({
                "auditor_email": auditor_email,
                "policy_document_id": document.document_id,
                "policy_id": policy.id,
            }),
        },
    );
}

#[tokio::test]
async fn evidence_download_is_concealed_outside_the_grant_period_and_served_on_overlap() {
    let app = harness::app().await;
    let subject = "auth0|auditor-evidence-download-period";
    let workspace_name = "Auditor Evidence Download Period";
    let evidence_title = "Period-scoped auditor evidence";
    let filename = "period-evidence.txt";
    let bytes = b"period scoped evidence bytes";

    let scenario = ScenarioBuilder::new(&app)
        .with_user(subject)
        .with_workspace(subject, workspace_name)
        .with_evidence(workspace_name, evidence_title)
        .with_evidence_document(
            workspace_name,
            evidence_title,
            filename,
            bytes,
            "2026-01-01T00:00:00.000Z",
            "2026-03-31T23:59:59.000Z",
        )
        .build()
        .await;
    let workspace = scenario.workspace(workspace_name);
    let workspace_id = workspace.id;
    let submission = workspace.evidence(evidence_title).submission(filename);
    let path = evidence_download_path(submission.id, submission.document_id);

    let token = authorize_agent_connection(
        &app,
        subject,
        "Auditor Evidence Period Access",
        &[WorkspacePermission::ManageAuditorAccess],
    )
    .await;
    let client = McpClient::connect(app.mcp_server(), &token).await;

    let outside_email = "auditor-evidence-period-outside@example.com";
    let outside_access = client
        .call_tool(
            "create_auditor_access_link",
            json!({
                "email": outside_email,
                "expires_at": "2099-01-01T00:00:00Z",
                "period_start": "2027-01-01T00:00:00Z",
                "period_end": "2027-12-31T23:59:59Z",
            }),
        )
        .await;
    let outside_url = Url::parse(
        outside_access["url"]
            .as_str()
            .expect("outside-period auditor URL is text"),
    )
    .expect("outside-period auditor URL parses");
    let outside_token = invite_token(&outside_url);
    app.app_server()
        .get(&local_path(outside_url.as_str()))
        .await
        .assert_status_ok();
    let outside_cookie = authenticate_auditor(
        &app,
        workspace_id,
        &outside_token,
        "auth0|auditor-evidence-period-outside-identity",
        outside_email,
    )
    .await;
    assert_download_not_found(
        &app.app_server()
            .get(&path)
            .add_header("cookie", outside_cookie)
            .await,
    );

    let overlap_email = "auditor-evidence-period-overlap@example.com";
    let overlap_access = client
        .call_tool(
            "create_auditor_access_link",
            json!({
                "email": overlap_email,
                "expires_at": "2099-01-01T00:00:00Z",
                "period_start": "2026-03-01T00:00:00Z",
                "period_end": "2026-04-30T23:59:59Z",
            }),
        )
        .await;
    let overlap_url = Url::parse(
        overlap_access["url"]
            .as_str()
            .expect("overlapping auditor URL is text"),
    )
    .expect("overlapping auditor URL parses");
    let overlap_token = invite_token(&overlap_url);
    app.app_server()
        .get(&local_path(overlap_url.as_str()))
        .await
        .assert_status_ok();
    let overlap_cookie = authenticate_auditor(
        &app,
        workspace_id,
        &overlap_token,
        "auth0|auditor-evidence-period-overlap-identity",
        overlap_email,
    )
    .await;
    assert_safe_download(
        &app.app_server()
            .get(&path)
            .add_header("cookie", overlap_cookie)
            .await,
        bytes,
        filename,
    );
}

#[tokio::test]
async fn evidence_downloads_conceal_pipeline_archive_identifier_tenant_and_session_boundaries() {
    let app = harness::app().await;
    let subject = "auth0|auditor-evidence-download-concealment";
    let foreign_subject = "auth0|auditor-evidence-download-concealment-foreign";
    let workspace_name = "Auditor Evidence Download Concealment";
    let foreign_workspace_name = "Auditor Evidence Download Concealment Foreign";
    let evidence_title = "Concealed auditor evidence";
    let foreign_evidence_title = "Foreign concealed auditor evidence";
    let auditor_email = "auditor-evidence-download-concealment@example.com";

    let scenario = ScenarioBuilder::new(&app)
        .with_user(subject)
        .with_workspace(subject, workspace_name)
        .with_evidence(workspace_name, evidence_title)
        .with_evidence_document(
            workspace_name,
            evidence_title,
            "mismatch-a.txt",
            b"mismatch evidence a",
            "2026-01-01T00:00:00.000Z",
            "2026-03-31T23:59:59.000Z",
        )
        .with_evidence_document(
            workspace_name,
            evidence_title,
            "mismatch-b.txt",
            b"mismatch evidence b",
            "2026-01-01T00:00:00.000Z",
            "2026-03-31T23:59:59.000Z",
        )
        .with_evidence_document(
            workspace_name,
            evidence_title,
            "archived-evidence.txt",
            b"archived evidence bytes",
            "2026-01-01T00:00:00.000Z",
            "2026-03-31T23:59:59.000Z",
        )
        .with_user(foreign_subject)
        .with_workspace(foreign_subject, foreign_workspace_name)
        .with_evidence(foreign_workspace_name, foreign_evidence_title)
        .with_evidence_document(
            foreign_workspace_name,
            foreign_evidence_title,
            "foreign-evidence.txt",
            b"foreign evidence bytes",
            "2026-01-01T00:00:00.000Z",
            "2026-03-31T23:59:59.000Z",
        )
        .build()
        .await;
    let workspace = scenario.workspace(workspace_name);
    let workspace_id = workspace.id;
    let evidence = workspace.evidence(evidence_title);
    let mismatch_a = evidence.submission("mismatch-a.txt");
    let mismatch_b = evidence.submission("mismatch-b.txt");
    let archived = evidence.submission("archived-evidence.txt");
    let foreign = scenario
        .workspace(foreign_workspace_name)
        .evidence(foreign_evidence_title)
        .submission("foreign-evidence.txt");

    let owner_token = authorize_agent_connection(
        &app,
        subject,
        "Auditor Evidence Concealment Owner",
        &WorkspacePermission::ALL,
    )
    .await;
    let owner = McpClient::connect(app.mcp_server(), &owner_token).await;
    let upload_grant = owner
        .call_tool(
            "manage_evidence_submissions",
            json!({
                "evidence_id": evidence.id,
                "valid_from": "2026-01-01T00:00:00.000Z",
                "valid_until": "2026-03-31T23:59:59.000Z",
            }),
        )
        .await;
    let redeemed = app
        .app_server()
        .get(&local_path(
            upload_grant["url"]
                .as_str()
                .expect("evidence upload URL is text"),
        ))
        .await;
    redeemed.assert_status(StatusCode::SEE_OTHER);
    assert_eq!(redeemed.header("location"), "/evidence-document-uploads");
    let owner_cookie = request_cookie(
        redeemed
            .headers()
            .get(SET_COOKIE)
            .expect("evidence redemption sets a cookie")
            .to_str()
            .expect("evidence cookie is text"),
    );

    let archived_response = app
        .app_server()
        .post(&evidence_archive_path(archived.id, archived.document_id))
        .add_header("cookie", owner_cookie.clone())
        .add_header(REQUEST_ID_HEADER, Uuid::new_v4().to_string())
        .await;
    archived_response.assert_status(StatusCode::SEE_OTHER);
    assert_eq!(
        archived_response.header("location"),
        "/evidence-document-uploads"
    );

    let pending_request_id = Uuid::new_v4();
    let mut pending_gate = app
        .pipeline_controls()
        .hold(DOCUMENT_SCAN_REQUESTED, pending_request_id);
    let mut pending_events = app.pipeline_events().subscribe();
    app.app_server()
        .post(EVIDENCE_UPLOAD_PATH)
        .add_header("cookie", owner_cookie.clone())
        .add_header(REQUEST_ID_HEADER, pending_request_id.to_string())
        .multipart(upload_form(
            b"pending evidence bytes",
            "pending-evidence.txt",
        ))
        .await
        .assert_status(StatusCode::SEE_OTHER);
    let pending_interception = pending_gate.await_interception().await;
    let pending_read_model = owner
        .call_tool(
            "list_evidence_submissions",
            json!({ "evidence_id": evidence.id }),
        )
        .await["submissions"]
        .as_array()
        .expect("evidence submissions is an array")
        .iter()
        .find(|submission| submission["document"]["filename"] == "pending-evidence.txt")
        .expect("pending evidence is listed")
        .clone();
    let pending = TestEvidenceSubmission::from_mcp(&pending_read_model);
    assert_eq!(pending.upload_status, "pending");
    assert_eq!(
        pending_interception.aggregate_id,
        pending.document_id.to_string()
    );

    let failed_request_id = Uuid::new_v4();
    let mut failed_events = app.pipeline_events().subscribe();
    app.app_server()
        .post(EVIDENCE_UPLOAD_PATH)
        .add_header("cookie", owner_cookie)
        .add_header(REQUEST_ID_HEADER, failed_request_id.to_string())
        .multipart(upload_form(ERROR_TRIGGER, "failed-evidence.txt"))
        .await
        .assert_status(StatusCode::SEE_OTHER);
    let failed_read_model = owner
        .call_tool(
            "list_evidence_submissions",
            json!({ "evidence_id": evidence.id }),
        )
        .await["submissions"]
        .as_array()
        .expect("evidence submissions is an array")
        .iter()
        .find(|submission| submission["document"]["filename"] == "failed-evidence.txt")
        .expect("failed evidence is listed")
        .clone();
    let failed = TestEvidenceSubmission::from_mcp(&failed_read_model);
    assert_eq!(
        failed_events
            .await_event(DOCUMENT_SCAN_REQUESTED, &failed.document_id.to_string())
            .await,
        StatusCode::NO_CONTENT
    );
    let failed_read = owner
        .call_tool(
            "list_evidence_submissions",
            json!({ "evidence_id": evidence.id }),
        )
        .await;
    assert_eq!(
        failed_read["submissions"]
            .as_array()
            .expect("evidence submissions is an array")
            .iter()
            .find(|submission| submission["document"]["id"] == failed.document_id.to_string())
            .expect("failed evidence remains listed")["document"]["upload_status"],
        "failed"
    );

    let access_token = authorize_agent_connection(
        &app,
        subject,
        "Auditor Evidence Concealment Access",
        &[WorkspacePermission::ManageAuditorAccess],
    )
    .await;
    let access_client = McpClient::connect(app.mcp_server(), &access_token).await;
    let access = access_client
        .call_tool(
            "create_auditor_access_link",
            json!({
                "email": auditor_email,
                "expires_at": "2099-01-01T00:00:00Z",
                "period_start": "2026-01-01T00:00:00Z",
                "period_end": "2026-03-31T23:59:59Z",
            }),
        )
        .await;
    let invite_url = Url::parse(access["url"].as_str().expect("auditor URL is text"))
        .expect("auditor URL parses");
    let invite_token = invite_token(&invite_url);
    app.app_server()
        .get(&local_path(invite_url.as_str()))
        .await
        .assert_status_ok();
    let auditor_cookie = authenticate_auditor(
        &app,
        workspace_id,
        &invite_token,
        "auth0|auditor-evidence-download-concealment-identity",
        auditor_email,
    )
    .await;

    let active_path = evidence_download_path(mismatch_a.id, mismatch_a.document_id);
    assert_download_not_found(&app.app_server().get(&active_path).await);
    assert_download_not_found(
        &app.app_server()
            .get(&active_path)
            .add_header("cookie", "proofplane_auditor_session=tampered")
            .await,
    );
    for path in [
        evidence_download_path(pending.id, pending.document_id),
        evidence_download_path(failed.id, failed.document_id),
        evidence_download_path(archived.id, archived.document_id),
        evidence_download_path(mismatch_a.id, mismatch_b.document_id),
        evidence_download_path(foreign.id, foreign.document_id),
    ] {
        assert_download_not_found(
            &app.app_server()
                .get(&path)
                .add_header("cookie", auditor_cookie.clone())
                .await,
        );
    }

    pending_gate.release();
    assert_eq!(
        pending_events
            .await_event(DOCUMENT_SCAN_REQUESTED, &pending.document_id.to_string())
            .await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        pending_events
            .await_event(
                DOCUMENT_FINALIZATION_REQUESTED,
                &pending.document_id.to_string(),
            )
            .await,
        StatusCode::NO_CONTENT
    );
    let settled = owner
        .call_tool(
            "list_evidence_submissions",
            json!({ "evidence_id": evidence.id }),
        )
        .await;
    assert_eq!(
        settled["submissions"]
            .as_array()
            .expect("evidence submissions is an array")
            .iter()
            .find(|submission| submission["document"]["id"] == pending.document_id.to_string())
            .expect("released pending evidence remains listed")["document"]["upload_status"],
        "uploaded"
    );
}

#[tokio::test]
async fn policy_downloads_conceal_pipeline_archives_identifier_tenant_and_revoked_session() {
    let app = harness::app().await;
    let subject = "auth0|auditor-policy-download-concealment";
    let foreign_subject = "auth0|auditor-policy-download-concealment-foreign";
    let workspace_name = "Auditor Policy Download Concealment";
    let foreign_workspace_name = "Auditor Policy Download Concealment Foreign";
    let auditor_email = "auditor-policy-download-concealment@example.com";

    let scenario = ScenarioBuilder::new(&app)
        .with_user(subject)
        .with_workspace(subject, workspace_name)
        .with_policy(workspace_name, "Finalizing Policy")
        .with_policy(workspace_name, "Virus Policy")
        .with_policy(workspace_name, "Archived Document Policy")
        .with_policy_document(
            workspace_name,
            "Archived Document Policy",
            "archived-policy-document.txt",
            b"archived policy document bytes",
        )
        .with_policy(workspace_name, "Archived Policy")
        .with_policy_document(
            workspace_name,
            "Archived Policy",
            "archived-policy.txt",
            b"archived policy bytes",
        )
        .with_policy(workspace_name, "Mismatch Policy A")
        .with_policy_document(
            workspace_name,
            "Mismatch Policy A",
            "mismatch-policy-a.txt",
            b"mismatch policy a bytes",
        )
        .with_policy(workspace_name, "Mismatch Policy B")
        .with_policy_document(
            workspace_name,
            "Mismatch Policy B",
            "mismatch-policy-b.txt",
            b"mismatch policy b bytes",
        )
        .with_user(foreign_subject)
        .with_workspace(foreign_subject, foreign_workspace_name)
        .with_policy(foreign_workspace_name, "Foreign Policy")
        .with_policy_document(
            foreign_workspace_name,
            "Foreign Policy",
            "foreign-policy.txt",
            b"foreign policy bytes",
        )
        .build()
        .await;
    let workspace = scenario.workspace(workspace_name);
    let workspace_id = workspace.id;
    let finalizing_policy = workspace.policy("Finalizing Policy");
    let virus_policy = workspace.policy("Virus Policy");
    let archived_document_policy = workspace.policy("Archived Document Policy");
    let archived_document = archived_document_policy.document();
    let archived_policy = workspace.policy("Archived Policy");
    let archived_policy_document = archived_policy.document();
    let mismatch_a = workspace.policy("Mismatch Policy A");
    let mismatch_a_document = mismatch_a.document();
    let mismatch_b = workspace.policy("Mismatch Policy B");
    let mismatch_b_document = mismatch_b.document();
    let foreign_policy = scenario
        .workspace(foreign_workspace_name)
        .policy("Foreign Policy");
    let foreign_document = foreign_policy.document();

    let owner_token = authorize_agent_connection(
        &app,
        subject,
        "Auditor Policy Concealment Owner",
        &WorkspacePermission::ALL,
    )
    .await;
    let owner = McpClient::connect(app.mcp_server(), &owner_token).await;

    let archived_document_grant = owner
        .call_tool(
            "manage_policy_document",
            json!({ "policy_id": archived_document_policy.id }),
        )
        .await;
    let archived_document_redeemed = app
        .app_server()
        .get(&local_path(
            archived_document_grant["url"]
                .as_str()
                .expect("archived document grant URL is text"),
        ))
        .await;
    archived_document_redeemed.assert_status(StatusCode::SEE_OTHER);
    assert_eq!(
        archived_document_redeemed.header("location"),
        "/policy-document-uploads"
    );
    let archived_document_cookie = request_cookie(
        archived_document_redeemed
            .headers()
            .get(SET_COOKIE)
            .expect("archived document redemption sets a cookie")
            .to_str()
            .expect("archived document cookie is text"),
    );
    let archived_document_response = app
        .app_server()
        .post(&policy_archive_path(archived_document.document_id))
        .add_header("cookie", archived_document_cookie)
        .add_header(REQUEST_ID_HEADER, Uuid::new_v4().to_string())
        .await;
    archived_document_response.assert_status(StatusCode::SEE_OTHER);
    assert_eq!(
        archived_document_response.header("location"),
        "/policy-document-uploads"
    );

    let archived_policy_response = owner
        .call_tool("archive_policy", json!({ "policy_id": archived_policy.id }))
        .await;
    assert_eq!(
        archived_policy_response["policy_id"],
        archived_policy.id.to_string()
    );

    let finalizing_grant = owner
        .call_tool(
            "manage_policy_document",
            json!({ "policy_id": finalizing_policy.id }),
        )
        .await;
    let finalizing_redeemed = app
        .app_server()
        .get(&local_path(
            finalizing_grant["url"]
                .as_str()
                .expect("finalizing grant URL is text"),
        ))
        .await;
    finalizing_redeemed.assert_status(StatusCode::SEE_OTHER);
    assert_eq!(
        finalizing_redeemed.header("location"),
        "/policy-document-uploads"
    );
    let finalizing_cookie = request_cookie(
        finalizing_redeemed
            .headers()
            .get(SET_COOKIE)
            .expect("finalizing redemption sets a cookie")
            .to_str()
            .expect("finalizing cookie is text"),
    );
    let finalizing_request_id = Uuid::new_v4();
    let mut finalizing_gate = app
        .pipeline_controls()
        .hold(DOCUMENT_FINALIZATION_REQUESTED, finalizing_request_id);
    let mut finalizing_events = app.pipeline_events().subscribe();
    app.app_server()
        .post(POLICY_UPLOAD_PATH)
        .add_header("cookie", finalizing_cookie)
        .add_header(REQUEST_ID_HEADER, finalizing_request_id.to_string())
        .multipart(upload_form(
            b"finalizing policy bytes",
            "finalizing-policy.txt",
        ))
        .await
        .assert_status(StatusCode::SEE_OTHER);
    let finalizing_read = owner
        .call_tool("get_policy", json!({ "policy_id": finalizing_policy.id }))
        .await;
    let finalizing_document_id = Uuid::parse_str(
        finalizing_read["document"]["id"]
            .as_str()
            .expect("finalizing policy document id is text"),
    )
    .expect("finalizing policy document id is a UUID");
    assert_eq!(
        finalizing_events
            .await_event(DOCUMENT_SCAN_REQUESTED, &finalizing_document_id.to_string(),)
            .await,
        StatusCode::NO_CONTENT
    );
    let finalizing_interception = finalizing_gate.await_interception().await;
    assert_eq!(
        finalizing_interception.aggregate_id,
        finalizing_document_id.to_string()
    );
    let finalizing_read = owner
        .call_tool("get_policy", json!({ "policy_id": finalizing_policy.id }))
        .await;
    assert_eq!(finalizing_read["document"]["upload_status"], "finalizing");

    let virus_grant = owner
        .call_tool(
            "manage_policy_document",
            json!({ "policy_id": virus_policy.id }),
        )
        .await;
    let virus_redeemed = app
        .app_server()
        .get(&local_path(
            virus_grant["url"]
                .as_str()
                .expect("virus grant URL is text"),
        ))
        .await;
    virus_redeemed.assert_status(StatusCode::SEE_OTHER);
    assert_eq!(
        virus_redeemed.header("location"),
        "/policy-document-uploads"
    );
    let virus_cookie = request_cookie(
        virus_redeemed
            .headers()
            .get(SET_COOKIE)
            .expect("virus redemption sets a cookie")
            .to_str()
            .expect("virus cookie is text"),
    );
    let virus_request_id = Uuid::new_v4();
    let mut virus_events = app.pipeline_events().subscribe();
    app.app_server()
        .post(POLICY_UPLOAD_PATH)
        .add_header("cookie", virus_cookie)
        .add_header(REQUEST_ID_HEADER, virus_request_id.to_string())
        .multipart(upload_form(EICAR, "virus-policy.txt"))
        .await
        .assert_status(StatusCode::SEE_OTHER);
    let virus_read = owner
        .call_tool("get_policy", json!({ "policy_id": virus_policy.id }))
        .await;
    let virus_document_id = Uuid::parse_str(
        virus_read["document"]["id"]
            .as_str()
            .expect("virus policy document id is text"),
    )
    .expect("virus policy document id is a UUID");
    assert_eq!(
        virus_events
            .await_event(DOCUMENT_SCAN_REQUESTED, &virus_document_id.to_string())
            .await,
        StatusCode::NO_CONTENT
    );
    let virus_read = owner
        .call_tool("get_policy", json!({ "policy_id": virus_policy.id }))
        .await;
    assert_eq!(virus_read["document"]["upload_status"], "contains_virus");

    let access_token = authorize_agent_connection(
        &app,
        subject,
        "Auditor Policy Concealment Access",
        &[WorkspacePermission::ManageAuditorAccess],
    )
    .await;
    let access_client = McpClient::connect(app.mcp_server(), &access_token).await;
    let access = access_client
        .call_tool(
            "create_auditor_access_link",
            json!({
                "email": auditor_email,
                "expires_at": "2099-01-01T00:00:00Z",
                "period_start": "2026-01-01T00:00:00Z",
                "period_end": "2026-03-31T23:59:59Z",
            }),
        )
        .await;
    let grant_id = access["grant"]["id"]
        .as_str()
        .expect("auditor grant id is text")
        .to_owned();
    let invite_url = Url::parse(access["url"].as_str().expect("auditor URL is text"))
        .expect("auditor URL parses");
    let invite_token = invite_token(&invite_url);
    app.app_server()
        .get(&local_path(invite_url.as_str()))
        .await
        .assert_status_ok();
    let auditor_cookie = authenticate_auditor(
        &app,
        workspace_id,
        &invite_token,
        "auth0|auditor-policy-download-concealment-identity",
        auditor_email,
    )
    .await;

    for path in [
        policy_download_path(finalizing_policy.id, finalizing_document_id),
        policy_download_path(virus_policy.id, virus_document_id),
        policy_download_path(archived_document_policy.id, archived_document.document_id),
        policy_download_path(archived_policy.id, archived_policy_document.document_id),
        policy_download_path(mismatch_a.id, mismatch_b_document.document_id),
        policy_download_path(foreign_policy.id, foreign_document.document_id),
    ] {
        assert_download_not_found(
            &app.app_server()
                .get(&path)
                .add_header("cookie", auditor_cookie.clone())
                .await,
        );
    }

    access_client
        .call_tool(
            "revoke_auditor_access_link",
            json!({ "grant_id": grant_id }),
        )
        .await;
    assert_download_not_found(
        &app.app_server()
            .get(&policy_download_path(
                mismatch_a.id,
                mismatch_a_document.document_id,
            ))
            .add_header("cookie", auditor_cookie)
            .await,
    );

    finalizing_gate.release();
    assert_eq!(
        finalizing_events
            .await_event(
                DOCUMENT_FINALIZATION_REQUESTED,
                &finalizing_document_id.to_string(),
            )
            .await,
        StatusCode::NO_CONTENT
    );
    let settled = owner
        .call_tool("get_policy", json!({ "policy_id": finalizing_policy.id }))
        .await;
    assert_eq!(settled["document"]["upload_status"], "uploaded");
}

fn evidence_download_path(submission_id: Uuid, document_id: Uuid) -> String {
    format!(
        "{AUDITOR_PORTAL_PREFIX}/evidence-submissions/{submission_id}/documents/{document_id}/download"
    )
}

fn policy_download_path(policy_id: Uuid, document_id: Uuid) -> String {
    format!("{AUDITOR_PORTAL_PREFIX}/policies/{policy_id}/documents/{document_id}/download")
}

fn policy_archive_path(document_id: Uuid) -> String {
    format!("/policy-document-uploads/files/{document_id}/archive")
}

#[track_caller]
fn assert_download_not_found(response: &TestResponse) {
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

#[track_caller]
fn assert_safe_download(response: &TestResponse, bytes: &[u8], filename: &str) {
    response.assert_status_ok();
    assert_eq!(response.as_bytes().as_ref(), bytes);
    assert_eq!(response.header("content-type"), "text/plain");
    assert_eq!(response.header("content-length"), bytes.len().to_string());
    assert_eq!(
        response.header("content-disposition"),
        format!("document; filename=\"{filename}\"")
    );
    assert_eq!(response.header("cache-control"), "private, no-store");
    assert_eq!(response.header("referrer-policy"), "no-referrer");
}

struct ExpectedDownloadAudit<'a> {
    event_name: &'a str,
    operation: &'a str,
    workspace_id: Uuid,
    request_id: Uuid,
    object_type: &'a str,
    object_id: Uuid,
    metadata: Value,
}

#[track_caller]
fn assert_download_audit_event(record: &Value, expected: ExpectedDownloadAudit<'_>) {
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
            "client_type",
            "event_id",
            "event_name",
            "metadata",
            "object_id",
            "object_type",
            "operation",
            "outcome",
            "request_id",
            "system_name",
            "type",
            "workspace_id",
        ]
        .into_iter()
        .collect()
    );
    assert_eq!(fields["type"], "audit_log");
    Uuid::parse_str(fields["event_id"].as_str().expect("event id is text"))
        .expect("event id is a UUID");
    assert_eq!(fields["event_name"], expected.event_name);
    assert_eq!(fields["outcome"], "success");
    assert_eq!(fields["actor_type"], "system");
    assert_eq!(fields["system_name"], "auditor_browser");
    assert_eq!(fields["client_type"], "rest");
    assert_eq!(fields["operation"], expected.operation);
    assert_eq!(fields["workspace_id"], expected.workspace_id.to_string());
    assert_eq!(fields["request_id"], expected.request_id.to_string());
    assert_eq!(
        serde_json::from_str::<Value>(
            fields["metadata"]
                .as_str()
                .expect("audit metadata is JSON text")
        )
        .expect("audit metadata parses"),
        expected.metadata
    );
    assert_eq!(fields["object_type"], expected.object_type);
    assert_eq!(fields["object_id"], expected.object_id.to_string());
}
