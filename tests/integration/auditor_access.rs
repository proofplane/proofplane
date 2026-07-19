use axum::http::{header::SET_COOKIE, StatusCode};
use bytes::Bytes;
use chrono::{DateTime, Utc};
use futures_util::stream;
use proofplane::{
    domain::{CreatePolicyPayload, PolicyId, WorkspacePermission, WorkspacePermissions},
    mailer::DisabledMailAdapter,
    object_storage::{FilesystemObjectStore, ObjectKey, ObjectStore},
    repository::CreatePolicyDocumentResult,
    routes::request_context::REQUEST_ID_HEADER,
    services::{
        agent_connections::AgentConnectionContext,
        auditor_access_grants::{
            AuditorAccessGrantService, CreateAuditorAccessGrantRequest, IssuedAuditorAccessGrant,
        },
        auditor_access_sessions::{AuditorAccessSessionError, AuditorAccessSessionService},
        policies::PolicyService,
        policy_documents::PolicyDocumentService,
    },
};
use secrecy::ExposeSecret;
use serde_json::{json, Value};
use std::sync::Arc;
use uuid::Uuid;

use super::support::{capture_audit_logs, cc61_id, upload_document, TestApp};

#[tokio::test]
async fn valid_invite_otp_creates_digest_only_session_cookie_and_audits_without_secrets() {
    let app = auditor_app().await;
    let workspace_id = app.workspace_id("workspace");
    let grant = create_grant(&app, workspace_id).await;
    let invite_token = grant.raw_secret.expose_secret().to_owned();

    let request = app
        .server()
        .post(&format!("/auditor-access/{workspace_id}/otp/request"))
        .json(&serde_json::json!({ "token": invite_token }));
    let (response, request_logs) = capture_audit_logs(|request_id| async move {
        request
            .add_header(REQUEST_ID_HEADER, request_id.to_string())
            .await
    })
    .await;
    response.assert_status_ok();
    assert_eq!(app.sent_mail().len(), 1);
    assert_eq!(app.sent_mail()[0].auditor_email, "auditor@example.com");
    let otp = app.sent_mail()[0].code.clone();

    let request = app
        .server()
        .post(&format!("/auditor-access/{workspace_id}/otp/verify"))
        .json(&serde_json::json!({ "token": invite_token, "code": otp }));
    let (response, verify_logs) = capture_audit_logs(|request_id| async move {
        request
            .add_header(REQUEST_ID_HEADER, request_id.to_string())
            .await
    })
    .await;
    response.assert_status_ok();
    let set_cookie = response
        .headers()
        .get(SET_COOKIE)
        .expect("session cookie set")
        .to_str()
        .expect("cookie is ASCII");
    assert!(set_cookie.contains("proofplane_auditor_session="));
    assert!(set_cookie.contains("HttpOnly"));
    assert!(set_cookie.contains("Secure"));
    assert!(set_cookie.contains("SameSite=Lax"));
    assert!(set_cookie.contains("Path=/auditor-access"));
    assert!(set_cookie.contains("Max-Age=604800"));
    let raw_session = cookie_value(set_cookie);

    let session =
        AuditorAccessSessionService::new(app.postgres_arc(), Arc::new(DisabledMailAdapter))
            .load_session(raw_session)
            .await
            .expect("session loads");
    assert_eq!(session.auditor_email, "auditor@example.com");

    let row = app
        .postgres()
        .get()
        .await
        .expect("connection opens")
        .query_one(
            "SELECT octet_length(session_digest) AS digest_length, encode(session_digest, 'hex') AS digest_hex FROM auditor_sessions",
            &[],
        )
        .await
        .expect("session row reads");
    assert_eq!(row.get::<_, i32>("digest_length"), 32);
    assert!(!row.get::<_, String>("digest_hex").contains(raw_session));

    let logs = serde_json::to_string(&(request_logs, verify_logs)).expect("logs serialize");
    assert!(logs.contains("auditor_access_otp.requested"));
    assert!(logs.contains("auditor_access_otp.verified"));
    assert!(logs.contains("auditor_access_session.created"));
    assert!(!logs.contains(&invite_token));
    assert!(!logs.contains(&otp));
    assert!(!logs.contains(raw_session));
}

#[tokio::test]
async fn wrong_reused_rate_limited_and_invalid_invites_do_not_create_sessions() {
    let app = auditor_app().await;
    let workspace_id = app.workspace_id("workspace");
    let other_workspace_id = app.workspace_id("other");
    let grant = create_grant(&app, workspace_id).await;
    let invite_token = grant.raw_secret.expose_secret();

    for _ in 0..3 {
        app.server()
            .post(&format!("/auditor-access/{workspace_id}/otp/request"))
            .json(&serde_json::json!({ "token": invite_token }))
            .await
            .assert_status_ok();
    }
    let rate_limited = app
        .server()
        .post(&format!("/auditor-access/{workspace_id}/otp/request"))
        .json(&serde_json::json!({ "token": invite_token }))
        .await;
    assert_eq!(rate_limited.status_code(), StatusCode::CONFLICT);

    let wrong = app
        .server()
        .post(&format!("/auditor-access/{workspace_id}/otp/verify"))
        .json(&serde_json::json!({ "token": invite_token, "code": "000000" }))
        .await;
    assert_eq!(wrong.status_code(), StatusCode::NOT_FOUND);

    let code = app.sent_mail().last().expect("OTP sent").code.clone();
    app.server()
        .post(&format!("/auditor-access/{workspace_id}/otp/verify"))
        .json(&serde_json::json!({ "token": invite_token, "code": code }))
        .await
        .assert_status_ok();
    let reused = app
        .server()
        .post(&format!("/auditor-access/{workspace_id}/otp/verify"))
        .json(&serde_json::json!({ "token": invite_token, "code": code }))
        .await;
    assert_eq!(reused.status_code(), StatusCode::NOT_FOUND);

    let invalid = app
        .server()
        .post(&format!("/auditor-access/{workspace_id}/otp/request"))
        .json(&serde_json::json!({ "token": "not-a-token" }))
        .await;
    assert_eq!(invalid.status_code(), StatusCode::NOT_FOUND);
    let cross_workspace = app
        .server()
        .post(&format!("/auditor-access/{other_workspace_id}/otp/request"))
        .json(&serde_json::json!({ "token": invite_token }))
        .await;
    assert_eq!(cross_workspace.status_code(), StatusCode::NOT_FOUND);
    assert_eq!(app.sent_mail().len(), 3);
}

#[tokio::test]
async fn logout_and_grant_revocation_invalidate_existing_session() {
    let app = auditor_app().await;
    let workspace_id = app.workspace_id("workspace");
    let grant = create_grant(&app, workspace_id).await;
    let invite_token = grant.raw_secret.expose_secret();
    let session_service =
        AuditorAccessSessionService::new(app.postgres_arc(), Arc::new(DisabledMailAdapter));

    let raw_session = verified_session_cookie(&app, workspace_id, invite_token).await;
    app.server()
        .post("/auditor-access/logout")
        .add_header(
            "Cookie",
            format!("proofplane_auditor_session={raw_session}"),
        )
        .await
        .assert_status_ok();
    assert_unavailable(session_service.load_session(&raw_session).await);

    let raw_session = verified_session_cookie(&app, workspace_id, invite_token).await;
    AuditorAccessGrantService::new(app.postgres_arc())
        .revoke(
            &agent_connection_context(&app, workspace_id),
            grant.grant.id,
        )
        .await
        .expect("grant revokes");
    assert_unavailable(session_service.load_session(&raw_session).await);
}

#[tokio::test]
async fn portal_data_returns_workspace_graph_and_filters_archived_documents() {
    let app = auditor_app().await;
    let workspace_id = app.workspace_id("workspace");
    let control_id = app.control_id("workspace", "PP-AC-01");
    let grant = create_grant(&app, workspace_id).await;
    let invite_token = grant.raw_secret.expose_secret();

    let alpha_policy_id = create_policy(
        &app,
        workspace_id,
        "alpha policy",
        Some("Mapped policy"),
        vec![control_id],
    )
    .await;
    let pending_policy_document = upload_policy_document(
        &app,
        workspace_id,
        alpha_policy_id,
        "pending-policy.txt",
        b"pending policy",
    )
    .await;
    let pending_policy_document_id = pending_policy_document.id();
    let zulu_policy_id = create_policy(&app, workspace_id, "Zulu policy", None, Vec::new()).await;
    let archived_document_policy_id = create_policy(
        &app,
        workspace_id,
        "Archived document policy",
        None,
        Vec::new(),
    )
    .await;
    let archived_policy_document = upload_policy_document(
        &app,
        workspace_id,
        archived_document_policy_id,
        "hidden-policy.txt",
        b"hidden policy document",
    )
    .await;
    archive_document(&app, archived_policy_document.id().into()).await;
    let archived_policy_id = create_policy(
        &app,
        workspace_id,
        "Hidden archived policy",
        Some("must not appear"),
        vec![control_id],
    )
    .await;
    archive_policy(&app, archived_policy_id).await;
    let other_policy_id = create_policy(
        &app,
        app.workspace_id("other"),
        "Other workspace policy",
        None,
        vec![app.control_id("other", "PP-OTHER")],
    )
    .await;

    let request = app
        .create_evidence_request(
            workspace_id,
            &evidence_request("Access review evidence", "2026-03-01T00:00:00Z"),
        )
        .await;
    let request_id = uuid_field(&request["id"]);
    insert_control_mapping(
        &app,
        request_id,
        control_id,
        "Shows access reviews were performed.",
    )
    .await;

    let unmapped = app
        .create_evidence_request(
            workspace_id,
            &evidence_request("Unmapped evidence", "2026-02-01T00:00:00Z"),
        )
        .await;
    app.create_evidence_submission(
        workspace_id,
        uuid_field(&unmapped["id"]),
        &submission("unmapped should not appear"),
    )
    .await;

    let older = app
        .create_evidence_submission(workspace_id, request_id, &submission("older submission"))
        .await;
    let newer = app
        .create_evidence_submission(workspace_id, request_id, &submission("newer submission"))
        .await;
    let older_submission_id = uuid_field(&older["id"]);
    let newer_submission_id = uuid_field(&newer["id"]);
    set_received_at(&app, older_submission_id, "2026-04-01T00:00:00Z").await;
    set_received_at(&app, newer_submission_id, "2026-05-01T00:00:00Z").await;

    let uploaded = upload_document(
        &app,
        workspace_id,
        newer_submission_id,
        "eligible.txt",
        b"eligible",
    )
    .await;
    let uploaded_id = uuid_field(&uploaded["id"]);
    finalize_document(&app, workspace_id, newer_submission_id, uploaded_id).await;

    let pending = upload_document(
        &app,
        workspace_id,
        newer_submission_id,
        "pending.txt",
        b"pending",
    )
    .await;
    let pending_id = uuid_field(&pending["id"]);

    let archived = upload_document(
        &app,
        workspace_id,
        older_submission_id,
        "archived.txt",
        b"archived",
    )
    .await;
    let archived_id = uuid_field(&archived["id"]);
    finalize_document(&app, workspace_id, older_submission_id, archived_id).await;
    archive_document(&app, archived_id).await;

    let raw_session = verified_session_cookie(&app, workspace_id, invite_token).await;
    let request = app.server().get("/auditor-access/portal/data").add_header(
        "Cookie",
        format!("proofplane_auditor_session={raw_session}"),
    );
    let (response, logs) = capture_audit_logs(|request_id| async move {
        request
            .add_header(REQUEST_ID_HEADER, request_id.to_string())
            .await
    })
    .await;
    response.assert_status_ok();
    let body = response.json::<Value>();
    let serialized = serde_json::to_string(&body).expect("portal response serializes");

    assert_eq!(body["workspace_name"], "Auditor access workspace");
    assert!(body.get("workspace_id").is_none());
    assert_eq!(body["auditor_email"], "auditor@example.com");
    assert!(serialized.contains("Access review evidence"));
    assert!(!serialized.contains(&workspace_id.to_string()));
    assert!(!serialized.contains("Unmapped evidence"));
    assert!(!serialized.contains("Other auditor access workspace"));
    assert!(!serialized.contains("archived.txt"));
    assert!(!serialized.contains("object_key"));
    assert!(!serialized.contains("quarantine/"));
    assert!(!serialized.contains("workspaces/"));
    assert!(!serialized.contains("hidden-policy.txt"));
    assert!(!serialized.contains("Hidden archived policy"));
    assert!(!serialized.contains("Other workspace policy"));
    assert!(!serialized.contains(&archived_policy_id.to_string()));
    assert!(!serialized.contains(&other_policy_id.to_string()));
    assert!(!serialized.contains(invite_token));
    assert!(!serialized.contains(&raw_session));

    let controls = body["controls"].as_array().expect("controls is array");
    let control = controls
        .iter()
        .find(|control| control["code"] == "PP-AC-01")
        .expect("mapped control appears");
    let control_policies = control["policies"]
        .as_array()
        .expect("control policies is array");
    assert_eq!(control_policies.len(), 1);
    assert_eq!(control_policies[0]["id"], alpha_policy_id.to_string());
    assert_eq!(control_policies[0]["name"], "alpha policy");
    assert_eq!(control_policies[0]["document"]["upload_status"], "pending");
    let requirement = &control["framework_requirements"][0];
    assert_eq!(requirement["framework_name"], "SOC 2");
    assert_eq!(requirement["code"], "CC6.1");
    assert_eq!(requirement["title"], "Logical access security");
    let portal_request = &control["evidence_requests"][0];
    assert_eq!(
        portal_request["mapping_rationale"],
        "Shows access reviews were performed."
    );
    let submissions = portal_request["submissions"]
        .as_array()
        .expect("submissions is array");
    assert_eq!(submissions.len(), 2);
    assert_eq!(
        submissions[0]["submission"]["id"],
        newer_submission_id.to_string()
    );
    assert_eq!(
        submissions[1]["submission"]["id"],
        older_submission_id.to_string()
    );

    let documents = submissions[0]["documents"]
        .as_array()
        .expect("documents is array");
    assert_eq!(documents[0]["filename"], "eligible.txt");
    assert_eq!(
        documents[0]["created_by_user_id"],
        app.user_id().to_string()
    );
    assert_eq!(documents[0]["upload_status"], "uploaded");
    assert_eq!(documents[0]["download_eligible"], true);
    assert_eq!(documents[1]["filename"], "pending.txt");
    assert_eq!(documents[1]["upload_status"], "pending");
    assert_eq!(documents[1]["download_eligible"], false);
    assert_eq!(uuid_field(&documents[1]["id"]), pending_id);

    let policies = body["policies"].as_array().expect("policies is array");
    assert_eq!(
        policies
            .iter()
            .map(|policy| policy["name"].as_str().expect("policy name"))
            .collect::<Vec<_>>(),
        ["alpha policy", "Archived document policy", "Zulu policy"]
    );
    let alpha_policy = &policies[0];
    assert_eq!(alpha_policy["id"], alpha_policy_id.to_string());
    assert_eq!(alpha_policy["description"], "Mapped policy");
    assert_eq!(alpha_policy["controls"][0]["id"], control_id.to_string());
    assert_eq!(
        alpha_policy["document"]["id"],
        pending_policy_document_id.to_string()
    );
    assert_eq!(
        alpha_policy["document"]["policy_id"],
        alpha_policy_id.to_string()
    );
    assert_eq!(
        alpha_policy["document"]["created_by_user_id"],
        app.user_id().to_string()
    );
    assert_eq!(alpha_policy["document"]["upload_status"], "pending");
    assert_eq!(alpha_policy["document"]["download_eligible"], false);
    assert_eq!(policies[1]["document"], Value::Null);
    assert_eq!(policies[2]["id"], zulu_policy_id.to_string());
    assert_eq!(policies[2]["document"], Value::Null);

    let logs = serde_json::to_string(&logs).expect("logs serialize");
    assert!(logs.contains("auditor_portal.read"));
    assert!(logs.contains("auditor_policy_catalog.read"));
    assert!(!logs.contains(invite_token));
    assert!(!logs.contains(&raw_session));
}

#[tokio::test]
async fn portal_data_rejects_missing_tampered_and_revoked_sessions() {
    let app = auditor_app().await;
    let workspace_id = app.workspace_id("workspace");
    let grant = create_grant(&app, workspace_id).await;
    let invite_token = grant.raw_secret.expose_secret();

    app.server()
        .get("/auditor-access/portal/data")
        .await
        .assert_status_not_found();
    app.server()
        .get("/auditor-access/portal/data")
        .add_header("Cookie", "proofplane_auditor_session=tampered")
        .await
        .assert_status_not_found();

    let raw_session = verified_session_cookie(&app, workspace_id, invite_token).await;
    AuditorAccessGrantService::new(app.postgres_arc())
        .revoke(
            &agent_connection_context(&app, workspace_id),
            grant.grant.id,
        )
        .await
        .expect("grant revokes");
    app.server()
        .get("/auditor-access/portal/data")
        .add_header(
            "Cookie",
            format!("proofplane_auditor_session={raw_session}"),
        )
        .await
        .assert_status_not_found();
}

#[tokio::test]
async fn auditor_session_downloads_uploaded_document_with_safe_headers_and_audit() {
    let app = auditor_app().await;
    let workspace_id = app.workspace_id("workspace");
    let grant = create_grant(&app, workspace_id).await;
    let invite_token = grant.raw_secret.expose_secret();
    let submission_id = create_submission(&app, workspace_id).await;
    let content = b"auditor downloadable bytes";
    let document = upload_document(
        &app,
        workspace_id,
        submission_id,
        "Auditor packet.txt",
        content,
    )
    .await;
    let document_id = uuid_field(&document["id"]);
    let final_key = finalize_document(&app, workspace_id, submission_id, document_id).await;
    let raw_session = verified_session_cookie(&app, workspace_id, invite_token).await;

    let path = auditor_download_path(submission_id, document_id);
    let request = app.server().get(&path).add_header(
        "Cookie",
        format!("proofplane_auditor_session={raw_session}"),
    );
    let (response, logs) = capture_audit_logs(|request_id| async move {
        request
            .add_header(REQUEST_ID_HEADER, request_id.to_string())
            .await
    })
    .await;

    response.assert_status_ok();
    assert_eq!(response.as_bytes().as_ref(), content);
    assert_eq!(response.header("content-type"), "text/plain");
    assert_eq!(response.header("content-length"), content.len().to_string());
    assert_eq!(
        response.header("content-disposition"),
        "document; filename=\"Auditor packet.txt\""
    );
    assert_eq!(response.header("cache-control"), "private, no-store");
    assert_eq!(response.header("referrer-policy"), "no-referrer");

    let logs = serde_json::to_string(&logs).expect("logs serialize");
    assert!(logs.contains("auditor_document.downloaded"));
    assert!(logs.contains("auditor@example.com"));
    assert!(logs.contains(&workspace_id.to_string()));
    assert!(logs.contains(&submission_id.to_string()));
    assert!(logs.contains(&document_id.to_string()));
    assert!(!logs.contains(invite_token));
    assert!(!logs.contains(&raw_session));
    assert!(!logs.contains(final_key.as_str()));
    assert!(!logs.contains("auditor downloadable bytes"));
}

#[tokio::test]
async fn auditor_session_downloads_uploaded_policy_document_with_safe_headers_and_audit() {
    let app = auditor_app().await;
    let workspace_id = app.workspace_id("workspace");
    let grant = create_grant(&app, workspace_id).await;
    let invite_token = grant.raw_secret.expose_secret();
    let policy_id = create_policy(
        &app,
        workspace_id,
        "Downloadable policy",
        Some("Policy description must not enter download audits"),
        Vec::new(),
    )
    .await;
    let content = b"auditor policy bytes";
    let document =
        upload_policy_document(&app, workspace_id, policy_id, "Auditor policy.txt", content).await;
    let document_id = document.id();
    let final_key =
        finalize_policy_document(&app, workspace_id, policy_id, document_id.into()).await;
    let raw_session = verified_session_cookie(&app, workspace_id, invite_token).await;

    let request = app
        .server()
        .get(&policy_auditor_download_path(policy_id, document_id.into()))
        .add_header(
            "Cookie",
            format!("proofplane_auditor_session={raw_session}"),
        );
    let (response, logs) = capture_audit_logs(|request_id| async move {
        request
            .add_header(REQUEST_ID_HEADER, request_id.to_string())
            .await
    })
    .await;

    response.assert_status_ok();
    assert_eq!(response.as_bytes().as_ref(), content);
    assert_eq!(response.header("content-type"), "text/plain");
    assert_eq!(response.header("content-length"), content.len().to_string());
    assert_eq!(
        response.header("content-disposition"),
        "document; filename=\"Auditor policy.txt\""
    );
    assert_eq!(response.header("cache-control"), "private, no-store");
    assert_eq!(response.header("referrer-policy"), "no-referrer");

    let logs = serde_json::to_string(&logs).expect("logs serialize");
    assert!(logs.contains("auditor_policy_document.downloaded"));
    assert!(logs.contains(&workspace_id.to_string()));
    assert!(logs.contains(&policy_id.to_string()));
    assert!(logs.contains(&document_id.to_string()));
    assert!(!logs.contains("Downloadable policy"));
    assert!(!logs.contains("Policy description"));
    assert!(!logs.contains("Auditor policy.txt"));
    assert!(!logs.contains(invite_token));
    assert!(!logs.contains(&raw_session));
    assert!(!logs.contains(&final_key));
    assert!(!logs.contains("auditor policy bytes"));
}

#[tokio::test]
async fn auditor_policy_download_conceals_ineligible_tenant_and_session_states() {
    let app = auditor_app().await;
    let workspace_id = app.workspace_id("workspace");
    let other_workspace_id = app.workspace_id("other");
    let grant = create_grant(&app, workspace_id).await;
    let invite_token = grant.raw_secret.expose_secret();
    let raw_session = verified_session_cookie(&app, workspace_id, invite_token).await;
    let policy_id = create_policy(
        &app,
        workspace_id,
        "Unavailable policy document",
        None,
        Vec::new(),
    )
    .await;
    let document = upload_policy_document(
        &app,
        workspace_id,
        policy_id,
        "unavailable-policy.txt",
        b"unavailable policy",
    )
    .await;
    let document_id = document.id();
    let path = policy_auditor_download_path(policy_id, document_id.into());

    app.server().get(&path).await.assert_status_not_found();
    app.server()
        .get(&path)
        .add_header("Cookie", "proofplane_auditor_session=tampered")
        .await
        .assert_status_not_found();
    assert_policy_auditor_download_not_found(&app, &raw_session, policy_id, document_id.into())
        .await;
    for status in ["finalizing", "failed", "contains_virus"] {
        set_document_status(&app, document_id.into(), status).await;
        assert_policy_auditor_download_not_found(&app, &raw_session, policy_id, document_id.into())
            .await;
    }
    finalize_policy_document(&app, workspace_id, policy_id, document_id.into()).await;
    assert_policy_auditor_download_not_found(
        &app,
        &raw_session,
        Uuid::new_v4().into(),
        document_id.into(),
    )
    .await;
    archive_document(&app, document_id.into()).await;
    assert_policy_auditor_download_not_found(&app, &raw_session, policy_id, document_id.into())
        .await;

    let archived_policy_id = create_policy(
        &app,
        workspace_id,
        "Archived policy download",
        None,
        Vec::new(),
    )
    .await;
    let archived_policy_document = upload_policy_document(
        &app,
        workspace_id,
        archived_policy_id,
        "archived-policy.txt",
        b"archived policy",
    )
    .await;
    finalize_policy_document(
        &app,
        workspace_id,
        archived_policy_id,
        archived_policy_document.id().into(),
    )
    .await;
    archive_policy(&app, archived_policy_id).await;
    assert_policy_auditor_download_not_found(
        &app,
        &raw_session,
        archived_policy_id,
        archived_policy_document.id().into(),
    )
    .await;

    let other_policy_id = create_policy(
        &app,
        other_workspace_id,
        "Other tenant policy",
        None,
        Vec::new(),
    )
    .await;
    let other_document = upload_policy_document(
        &app,
        other_workspace_id,
        other_policy_id,
        "other-policy.txt",
        b"other tenant policy",
    )
    .await;
    finalize_policy_document(
        &app,
        other_workspace_id,
        other_policy_id,
        other_document.id().into(),
    )
    .await;
    assert_policy_auditor_download_not_found(
        &app,
        &raw_session,
        other_policy_id,
        other_document.id().into(),
    )
    .await;

    AuditorAccessGrantService::new(app.postgres_arc())
        .revoke(
            &agent_connection_context(&app, workspace_id),
            grant.grant.id,
        )
        .await
        .expect("grant revokes");
    app.server()
        .get(&policy_auditor_download_path(
            other_policy_id,
            other_document.id().into(),
        ))
        .add_header(
            "Cookie",
            format!("proofplane_auditor_session={raw_session}"),
        )
        .await
        .assert_status_not_found();
}

#[tokio::test]
async fn auditor_policy_download_metadata_mismatch_is_internal_without_storage_details() {
    let app = auditor_app().await;
    let workspace_id = app.workspace_id("workspace");
    let grant = create_grant(&app, workspace_id).await;
    let policy_id = create_policy(
        &app,
        workspace_id,
        "Metadata mismatch policy",
        None,
        Vec::new(),
    )
    .await;
    let document = upload_policy_document(
        &app,
        workspace_id,
        policy_id,
        "policy-mismatch.txt",
        b"policy mismatch bytes",
    )
    .await;
    let document_id = document.id();
    let final_key =
        finalize_policy_document(&app, workspace_id, policy_id, document_id.into()).await;
    let metadata_path = app
        .object_storage_root()
        .join("metadata")
        .join(format!("{final_key}.json"));
    let mut metadata: Value =
        serde_json::from_slice(&std::fs::read(&metadata_path).expect("metadata reads"))
            .expect("metadata parses");
    metadata["sha256"] = Value::String("0".repeat(64));
    std::fs::write(
        metadata_path,
        serde_json::to_vec_pretty(&metadata).expect("metadata serializes"),
    )
    .expect("metadata writes");
    let raw_session =
        verified_session_cookie(&app, workspace_id, grant.raw_secret.expose_secret()).await;

    let response = app
        .server()
        .get(&policy_auditor_download_path(policy_id, document_id.into()))
        .add_header(
            "Cookie",
            format!("proofplane_auditor_session={raw_session}"),
        )
        .await;
    response.assert_status_internal_server_error();
    let body = String::from_utf8_lossy(response.as_bytes().as_ref());
    assert!(!body.contains(&final_key));
    assert!(!body.contains("policy mismatch bytes"));
}

#[tokio::test]
async fn auditor_download_rejects_ineligible_missing_and_cross_workspace_documents() {
    let app = auditor_app().await;
    let workspace_id = app.workspace_id("workspace");
    let other_workspace_id = app.workspace_id("other");
    let grant = create_grant(&app, workspace_id).await;
    let invite_token = grant.raw_secret.expose_secret();
    let raw_session = verified_session_cookie(&app, workspace_id, invite_token).await;
    let submission_id = create_submission(&app, workspace_id).await;

    let pending =
        upload_document(&app, workspace_id, submission_id, "pending.txt", b"pending").await;
    let pending_id = uuid_field(&pending["id"]);
    assert_auditor_download_not_found(&app, &raw_session, submission_id, pending_id).await;

    for status in ["finalizing", "failed", "contains_virus"] {
        set_document_status(&app, pending_id, status).await;
        assert_auditor_download_not_found(&app, &raw_session, submission_id, pending_id).await;
    }

    let archived = upload_document(
        &app,
        workspace_id,
        submission_id,
        "archived.txt",
        b"archived",
    )
    .await;
    let archived_id = uuid_field(&archived["id"]);
    finalize_document(&app, workspace_id, submission_id, archived_id).await;
    archive_document(&app, archived_id).await;
    assert_auditor_download_not_found(&app, &raw_session, submission_id, archived_id).await;

    assert_auditor_download_not_found(&app, &raw_session, submission_id, Uuid::new_v4()).await;
    assert_auditor_download_not_found(&app, &raw_session, Uuid::new_v4(), archived_id).await;

    let other_submission_id = create_submission(&app, other_workspace_id).await;
    let other = upload_document(
        &app,
        other_workspace_id,
        other_submission_id,
        "other.txt",
        b"other",
    )
    .await;
    let other_document_id = uuid_field(&other["id"]);
    finalize_document(
        &app,
        other_workspace_id,
        other_submission_id,
        other_document_id,
    )
    .await;
    assert_auditor_download_not_found(&app, &raw_session, other_submission_id, other_document_id)
        .await;
}

#[tokio::test]
async fn auditor_download_rejects_logged_out_tampered_expired_and_grant_revoked_sessions() {
    let app = auditor_app().await;
    let workspace_id = app.workspace_id("workspace");
    let grant = create_grant(&app, workspace_id).await;
    let invite_token = grant.raw_secret.expose_secret();
    let submission_id = create_submission(&app, workspace_id).await;
    let document =
        upload_document(&app, workspace_id, submission_id, "session.txt", b"session").await;
    let document_id = uuid_field(&document["id"]);
    finalize_document(&app, workspace_id, submission_id, document_id).await;
    let path = auditor_download_path(submission_id, document_id);

    app.server().get(&path).await.assert_status_not_found();
    app.server()
        .get(&path)
        .add_header("Cookie", "proofplane_auditor_session=tampered")
        .await
        .assert_status_not_found();

    let logged_out_session = verified_session_cookie(&app, workspace_id, invite_token).await;
    app.server()
        .post("/auditor-access/logout")
        .add_header(
            "Cookie",
            format!("proofplane_auditor_session={logged_out_session}"),
        )
        .await
        .assert_status_ok();
    let logged_out = app
        .server()
        .get(&path)
        .add_header(
            "Cookie",
            format!("proofplane_auditor_session={logged_out_session}"),
        )
        .await;
    logged_out.assert_status_not_found();
    assert!(!String::from_utf8_lossy(logged_out.as_bytes().as_ref()).contains("session"));

    let expired_session = verified_session_cookie(&app, workspace_id, invite_token).await;
    let loaded_session =
        AuditorAccessSessionService::new(app.postgres_arc(), Arc::new(DisabledMailAdapter))
            .load_session(&expired_session)
            .await
            .expect("session loads");
    app.postgres()
        .get()
        .await
        .expect("connection opens")
        .execute(
            "UPDATE auditor_sessions SET created_at = now() - interval '2 seconds', expires_at = now() - interval '1 second' WHERE id = $1",
            &[&Uuid::from(loaded_session.id)],
        )
        .await
        .expect("session expires");
    app.server()
        .get(&path)
        .add_header(
            "Cookie",
            format!("proofplane_auditor_session={expired_session}"),
        )
        .await
        .assert_status_not_found();

    let revoked_session = verified_session_cookie(&app, workspace_id, invite_token).await;
    AuditorAccessGrantService::new(app.postgres_arc())
        .revoke(
            &agent_connection_context(&app, workspace_id),
            grant.grant.id,
        )
        .await
        .expect("grant revokes");
    app.server()
        .get(&path)
        .add_header(
            "Cookie",
            format!("proofplane_auditor_session={revoked_session}"),
        )
        .await
        .assert_status_not_found();
}

#[tokio::test]
async fn auditor_download_metadata_mismatch_is_internal_without_public_storage_details() {
    let app = auditor_app().await;
    let workspace_id = app.workspace_id("workspace");
    let grant = create_grant(&app, workspace_id).await;
    let invite_token = grant.raw_secret.expose_secret();
    let submission_id = create_submission(&app, workspace_id).await;
    let content = b"mismatch bytes";
    let document =
        upload_document(&app, workspace_id, submission_id, "mismatch.txt", content).await;
    let document_id = uuid_field(&document["id"]);
    let final_key = finalize_document(&app, workspace_id, submission_id, document_id).await;
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

    let raw_session = verified_session_cookie(&app, workspace_id, invite_token).await;
    let response = app
        .server()
        .get(&auditor_download_path(submission_id, document_id))
        .add_header(
            "Cookie",
            format!("proofplane_auditor_session={raw_session}"),
        )
        .await;
    response.assert_status_internal_server_error();
    let body = String::from_utf8_lossy(response.as_bytes().as_ref());
    assert!(!body.contains(final_key.as_str()));
    assert!(!body.contains("mismatch bytes"));
}

#[tokio::test]
async fn browser_invite_otp_and_portal_flow_renders_read_only_graph() {
    let app = auditor_app().await;
    let workspace_id = app.workspace_id("workspace");
    let control_id = app.control_id("workspace", "PP-AC-01");
    let grant = create_grant(&app, workspace_id).await;
    let invite_token = grant.raw_secret.expose_secret();

    let invite = app
        .server()
        .get(&format!(
            "/auditor-access/{workspace_id}?token={invite_token}"
        ))
        .await;
    invite.assert_status_ok();
    let invite_body = html_body(&invite);
    assert!(invite_body.contains("Verify access for auditor@example.com"));
    assert!(invite_body.contains("Send verification code"));
    assert!(!invite_body.contains("Access reviews"));

    let request = app
        .server()
        .post(&format!(
            "/auditor-access/{workspace_id}/otp/request/browser"
        ))
        .form(&[("token", invite_token)])
        .await;
    request.assert_status_ok();
    assert_eq!(app.sent_mail().len(), 1);
    let request_body = html_body(&request);
    assert!(request_body.contains("Code sent"));
    assert!(request_body.contains("Verification code"));

    let code = app.sent_mail()[0].code.clone();
    let verify = app
        .server()
        .post(&format!(
            "/auditor-access/{workspace_id}/otp/verify/browser"
        ))
        .form(&[("token", invite_token), ("code", code.as_str())])
        .await;
    verify.assert_status(StatusCode::SEE_OTHER);
    assert_eq!(verify.header("location"), "/auditor-access/portal");
    let raw_session = cookie_value(
        verify
            .headers()
            .get(SET_COOKIE)
            .expect("session cookie set")
            .to_str()
            .expect("cookie is ASCII"),
    )
    .to_owned();

    let request = app
        .create_evidence_request(
            workspace_id,
            &evidence_request("Access review evidence", "2026-03-01T00:00:00Z"),
        )
        .await;
    let request_id = uuid_field(&request["id"]);
    insert_control_mapping(
        &app,
        request_id,
        control_id,
        "Shows access reviews were performed.",
    )
    .await;
    let submission = app
        .create_evidence_submission(
            workspace_id,
            request_id,
            &submission("browser portal submission"),
        )
        .await;
    let submission_id = uuid_field(&submission["id"]);
    let uploaded = upload_document(
        &app,
        workspace_id,
        submission_id,
        "auditor-evidence.txt",
        b"eligible",
    )
    .await;
    let uploaded_id = uuid_field(&uploaded["id"]);
    finalize_document(&app, workspace_id, submission_id, uploaded_id).await;
    let pending = upload_document(
        &app,
        workspace_id,
        submission_id,
        "pending-evidence.txt",
        b"pending",
    )
    .await;
    let pending_id = uuid_field(&pending["id"]);

    let portal = app
        .server()
        .get("/auditor-access/portal")
        .add_header(
            "Cookie",
            format!("proofplane_auditor_session={raw_session}"),
        )
        .await;
    portal.assert_status_ok();
    let body = html_body(&portal);

    assert!(body.contains("Auditor access workspace"));
    assert!(!body.contains(&workspace_id.to_string()));
    assert!(body.contains("Framework requirements"));
    assert!(body.contains("SOC 2"));
    assert!(body.contains("CC6.1"));
    assert!(body.contains("Logical access security"));
    assert!(body.contains("Mapped controls"));
    assert!(body.contains("Evidence requests"));
    assert!(body.contains("Evidence submissions"));
    assert!(body.contains("auditor@example.com"));
    assert!(body.contains("Read-only"));
    assert!(!body.contains("browser portal submission"));
    assert!(!body.contains("auditor-evidence.txt"));

    let requirement_id = cc61_id();
    let requirement = app
        .server()
        .get(&format!(
            "/auditor-access/portal/framework-requirements/{requirement_id}"
        ))
        .add_header(
            "Cookie",
            format!("proofplane_auditor_session={raw_session}"),
        )
        .await;
    requirement.assert_status_ok();
    let body = html_body(&requirement);
    assert!(body.contains("Requirement context"));
    assert!(body.contains("PP-AC-01"));
    assert!(body.contains("Access reviews"));
    assert!(!body.contains("browser portal submission"));

    let control = app
        .server()
        .get(&format!(
            "/auditor-access/portal/framework-requirements/{requirement_id}/controls/{control_id}"
        ))
        .add_header(
            "Cookie",
            format!("proofplane_auditor_session={raw_session}"),
        )
        .await;
    control.assert_status_ok();
    let body = html_body(&control);
    assert!(body.contains("Submission history"));
    assert!(body.contains("Access review evidence"));
    assert!(body.contains("browser portal submission"));
    assert!(body.contains("Evidence documents"));
    assert!(body.contains("auditor-evidence.txt"));
    assert!(body.contains(&auditor_download_path(submission_id, uploaded_id)));
    assert!(body.contains("pending-evidence.txt"));
    assert!(!body.contains(&auditor_download_path(submission_id, pending_id)));
    assert!(!body.contains(invite_token));
    assert!(!body.contains(&raw_session));
}

#[tokio::test]
async fn browser_unavailable_states_do_not_leak_workspace_data() {
    let app = auditor_app().await;
    let workspace_id = app.workspace_id("workspace");
    let grant = create_grant(&app, workspace_id).await;
    let invite_token = grant.raw_secret.expose_secret();
    let raw_session = verified_browser_session_cookie(&app, workspace_id, invite_token).await;

    let invalid_invite = app
        .server()
        .get(&format!("/auditor-access/{workspace_id}?token=not-a-token"))
        .await;
    invalid_invite.assert_status_not_found();
    let body = html_body(&invalid_invite);
    assert!(body.contains("This auditor portal is not available"));
    assert!(!body.contains("Auditor access workspace"));
    assert!(!body.contains("Access reviews"));

    AuditorAccessGrantService::new(app.postgres_arc())
        .revoke(
            &agent_connection_context(&app, workspace_id),
            grant.grant.id,
        )
        .await
        .expect("grant revokes");
    let revoked_session = app
        .server()
        .get("/auditor-access/portal")
        .add_header(
            "Cookie",
            format!("proofplane_auditor_session={raw_session}"),
        )
        .await;
    revoked_session.assert_status_not_found();
    let body = html_body(&revoked_session);
    assert!(body.contains("This auditor portal is not available"));
    assert!(!body.contains("Auditor access workspace"));
    assert!(!body.contains("Access reviews"));
}

#[tokio::test]
async fn browser_portal_escapes_untrusted_content() {
    let app = auditor_app().await;
    let workspace_id = app.workspace_id("workspace");
    let control_id = app.control_id("workspace", "PP-AC-01");
    let grant = create_grant(&app, workspace_id).await;
    let invite_token = grant.raw_secret.expose_secret();
    let raw_session = verified_browser_session_cookie(&app, workspace_id, invite_token).await;

    let request = app
        .create_evidence_request(
            workspace_id,
            &evidence_request("<script>alert('request')</script>", "2026-03-01T00:00:00Z"),
        )
        .await;
    let request_id = uuid_field(&request["id"]);
    insert_control_mapping(&app, request_id, control_id, "<b>mapped</b>").await;
    let submission = app
        .create_evidence_submission(
            workspace_id,
            request_id,
            &json!({
            "coverage_start_at": "2026-01-01T00:00:00Z",
            "coverage_end_at": "2026-03-31T23:59:59Z",
            "source_system": "<source>",
            "collection_method": "manual",
            "summary": "<img src=x onerror=alert(1)>",
            "description": "Description with <strong>markup</strong>."
            }),
        )
        .await;
    let submission_id = uuid_field(&submission["id"]);
    let uploaded = upload_document(
        &app,
        workspace_id,
        submission_id,
        "portable evidence.txt",
        b"eligible",
    )
    .await;
    let uploaded_id = uuid_field(&uploaded["id"]);
    finalize_document(&app, workspace_id, submission_id, uploaded_id).await;

    let portal = app
        .server()
        .get(&format!(
            "/auditor-access/portal/framework-requirements/{}/controls/{control_id}",
            cc61_id()
        ))
        .add_header(
            "Cookie",
            format!("proofplane_auditor_session={raw_session}"),
        )
        .await;
    portal.assert_status_ok();
    let body = html_body(&portal);

    assert!(body.contains("&lt;script&gt;alert(&#39;request&#39;)&lt;/script&gt;"));
    assert!(body.contains("&lt;b&gt;mapped&lt;/b&gt;"));
    assert!(body.contains("&lt;img src=x onerror=alert(1)&gt;"));
    assert!(body.contains("&lt;source&gt;"));
    assert!(body.contains("Description with &lt;strong&gt;markup&lt;/strong&gt;."));
    assert!(!body.contains("<script>alert"));
    assert!(!body.contains("<b>mapped</b>"));
    assert!(!body.contains("<img src=x"));
    assert!(!body.contains("<source>"));
    assert!(!body.contains("<strong>markup</strong>"));
}

async fn verified_session_cookie(app: &TestApp, workspace_id: Uuid, invite_token: &str) -> String {
    app.server()
        .post(&format!("/auditor-access/{workspace_id}/otp/request"))
        .json(&serde_json::json!({ "token": invite_token }))
        .await
        .assert_status_ok();
    let code = app.sent_mail().last().expect("OTP sent").code.clone();
    let response = app
        .server()
        .post(&format!("/auditor-access/{workspace_id}/otp/verify"))
        .json(&serde_json::json!({ "token": invite_token, "code": code }))
        .await;
    response.assert_status_ok();
    cookie_value(
        response
            .headers()
            .get(SET_COOKIE)
            .expect("cookie set")
            .to_str()
            .expect("cookie is ASCII"),
    )
    .to_owned()
}

async fn verified_browser_session_cookie(
    app: &TestApp,
    workspace_id: Uuid,
    invite_token: &str,
) -> String {
    app.server()
        .post(&format!(
            "/auditor-access/{workspace_id}/otp/request/browser"
        ))
        .form(&[("token", invite_token)])
        .await
        .assert_status_ok();
    let code = app.sent_mail().last().expect("OTP sent").code.clone();
    let response = app
        .server()
        .post(&format!(
            "/auditor-access/{workspace_id}/otp/verify/browser"
        ))
        .form(&[("token", invite_token), ("code", code.as_str())])
        .await;
    response.assert_status(StatusCode::SEE_OTHER);
    cookie_value(
        response
            .headers()
            .get(SET_COOKIE)
            .expect("cookie set")
            .to_str()
            .expect("cookie is ASCII"),
    )
    .to_owned()
}

async fn auditor_app() -> TestApp {
    TestApp::builder()
        .without_default_auth()
        .with_soc2_reference_data()
        .workspace("workspace", "Auditor access workspace")
        .with_control("PP-AC-01", "Access reviews", vec![cc61_id()])
        .with_default_membership()
        .workspace("other", "Other auditor access workspace")
        .with_control("PP-OTHER", "Other control", vec![])
        .with_default_membership()
        .build()
        .await
}

async fn create_grant(app: &TestApp, workspace_id: Uuid) -> IssuedAuditorAccessGrant {
    AuditorAccessGrantService::new(app.postgres_arc())
        .create(
            &agent_connection_context(app, workspace_id),
            CreateAuditorAccessGrantRequest {
                auditor_email: "auditor@example.com".to_owned(),
                expires_at: None,
            },
        )
        .await
        .expect("auditor grant creates")
}

async fn create_policy(
    app: &TestApp,
    workspace_id: Uuid,
    name: &str,
    description: Option<&str>,
    control_ids: Vec<Uuid>,
) -> PolicyId {
    PolicyService::new(app.postgres_arc())
        .create(
            app.agent_connection_context(workspace_id),
            CreatePolicyPayload {
                name: name.to_owned(),
                description: description.map(str::to_owned),
                control_ids: control_ids.into_iter().map(Into::into).collect(),
            },
        )
        .await
        .expect("policy creates")
        .policy
        .id
}

async fn upload_policy_document(
    app: &TestApp,
    workspace_id: Uuid,
    policy_id: PolicyId,
    filename: &str,
    content: &[u8],
) -> proofplane::domain::Document {
    let object_store = Arc::new(
        FilesystemObjectStore::new(app.object_storage_root())
            .await
            .expect("policy document object store initializes"),
    );
    let service = PolicyDocumentService::new(app.postgres_arc(), object_store);
    let connection = app.agent_connection_context(workspace_id);
    let bytes = Bytes::copy_from_slice(content);
    let payload = service
        .upload(
            &connection,
            policy_id,
            filename.to_owned(),
            "text/plain".to_owned(),
            stream::once(async move { Ok(bytes) }),
        )
        .await
        .expect("policy document stages");
    match service
        .create(&connection, Uuid::new_v4(), policy_id, payload)
        .await
        .expect("policy document creates")
    {
        CreatePolicyDocumentResult::Created(document) => document,
        other => panic!("expected created policy document, got {other:?}"),
    }
}

async fn finalize_policy_document(
    app: &TestApp,
    workspace_id: Uuid,
    policy_id: PolicyId,
    document_id: Uuid,
) -> String {
    let client = app.postgres().get().await.expect("connection opens");
    let row = client
        .query_one(
            "SELECT filename, object_key FROM documents WHERE id = $1",
            &[&document_id],
        )
        .await
        .expect("policy document reads");
    let filename: String = row.get("filename");
    let source =
        ObjectKey::parse(row.get::<_, String>("object_key")).expect("policy quarantine key parses");
    let target = ObjectKey::new(
        workspace_id.into(),
        format!("policies/{policy_id}/documents/{document_id}"),
        &filename,
    )
    .expect("policy final key builds");
    let store = FilesystemObjectStore::new(app.object_storage_root())
        .await
        .expect("policy object store initializes");
    store
        .copy_object(&source, &target)
        .await
        .expect("policy document finalizes");
    let final_key = target.to_string();
    client
        .execute(
            "UPDATE documents SET object_key = $2, upload_status = 'uploaded' WHERE id = $1",
            &[&document_id, &final_key],
        )
        .await
        .expect("policy document status updates");

    final_key
}

async fn archive_policy(app: &TestApp, policy_id: PolicyId) {
    app.postgres()
        .get()
        .await
        .expect("connection opens")
        .execute(
            "UPDATE policies SET archived_at = now() WHERE id = $1",
            &[&Uuid::from(policy_id)],
        )
        .await
        .expect("policy archives");
}

fn agent_connection_context(app: &TestApp, workspace_id: Uuid) -> AgentConnectionContext {
    AgentConnectionContext {
        user_id: app.user_id().into(),
        connection_id: app.api_token_id().into(),
        workspace_id: workspace_id.into(),
        permissions: WorkspacePermissions::from_iter(WorkspacePermission::ALL),
    }
}

fn cookie_value(set_cookie: &str) -> &str {
    set_cookie
        .split(';')
        .next()
        .expect("cookie pair")
        .split_once('=')
        .expect("cookie has value")
        .1
}

fn html_body(response: &axum_test::TestResponse) -> String {
    String::from_utf8_lossy(response.as_bytes().as_ref()).into_owned()
}

fn assert_unavailable(result: Result<impl std::fmt::Debug, AuditorAccessSessionError>) {
    assert!(matches!(
        result,
        Err(AuditorAccessSessionError::Unavailable)
    ));
}

fn evidence_request(title: &str, due_at: &str) -> Value {
    json!({
        "title": title,
        "description": format!("Description for {title}."),
        "collection_instructions": format!("Collect {title}."),
        "cadence": "quarterly",
        "due_at": due_at,
        "schedule_anchor_at": "2026-01-01T00:00:00Z",
        "freshness_window_days": 90,
        "status": "active"
    })
}

fn submission(summary: &str) -> Value {
    json!({
        "coverage_start_at": "2026-01-01T00:00:00Z",
        "coverage_end_at": "2026-03-31T23:59:59Z",
        "source_system": "test",
        "collection_method": "manual",
        "summary": summary,
        "description": format!("Description for {summary}.")
    })
}

async fn create_submission(app: &TestApp, workspace_id: Uuid) -> Uuid {
    let request = app
        .create_evidence_request(
            workspace_id,
            &evidence_request("Auditor download evidence", "2099-01-01T00:00:00Z"),
        )
        .await;
    let submission = app
        .create_evidence_submission(
            workspace_id,
            uuid_field(&request["id"]),
            &submission("auditor download submission"),
        )
        .await;

    uuid_field(&submission["id"])
}

fn auditor_download_path(submission_id: Uuid, document_id: Uuid) -> String {
    format!(
        "/auditor-access/portal/evidence-submissions/{submission_id}/documents/{document_id}/download"
    )
}

fn policy_auditor_download_path(policy_id: PolicyId, document_id: Uuid) -> String {
    format!("/auditor-access/portal/policies/{policy_id}/documents/{document_id}/download")
}

async fn assert_auditor_download_not_found(
    app: &TestApp,
    raw_session: &str,
    submission_id: Uuid,
    document_id: Uuid,
) {
    app.server()
        .get(&auditor_download_path(submission_id, document_id))
        .add_header(
            "Cookie",
            format!("proofplane_auditor_session={raw_session}"),
        )
        .await
        .assert_status_not_found();
}

async fn assert_policy_auditor_download_not_found(
    app: &TestApp,
    raw_session: &str,
    policy_id: PolicyId,
    document_id: Uuid,
) {
    app.server()
        .get(&policy_auditor_download_path(policy_id, document_id))
        .add_header(
            "Cookie",
            format!("proofplane_auditor_session={raw_session}"),
        )
        .await
        .assert_status_not_found();
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
fn uuid_field(value: &Value) -> Uuid {
    Uuid::parse_str(value.as_str().expect("UUID field is a string")).expect("field is a UUID")
}

async fn insert_control_mapping(
    app: &TestApp,
    evidence_request_id: Uuid,
    control_id: Uuid,
    rationale: &str,
) {
    app.postgres()
        .get()
        .await
        .expect("connection opens")
        .execute(
            r#"
INSERT INTO evidence_request_control_mappings (evidence_request_id, control_id, rationale)
VALUES ($1, $2, $3)
"#,
            &[&evidence_request_id, &control_id, &rationale],
        )
        .await
        .expect("control mapping inserts");
}

async fn set_received_at(app: &TestApp, submission_id: Uuid, received_at: &str) {
    let received_at = DateTime::parse_from_rfc3339(received_at).expect("received_at parses");
    let received_at = received_at.with_timezone(&Utc);
    app.postgres()
        .get()
        .await
        .expect("connection opens")
        .execute(
            "UPDATE evidence_submissions SET received_at = $2 WHERE id = $1",
            &[&submission_id, &received_at],
        )
        .await
        .expect("submission received_at updates");
}

async fn archive_document(app: &TestApp, document_id: Uuid) {
    app.postgres()
        .get()
        .await
        .expect("connection opens")
        .execute(
            "UPDATE documents SET archived = true WHERE id = $1",
            &[&document_id],
        )
        .await
        .expect("document archives");
}

async fn finalize_document(
    app: &TestApp,
    workspace_id: Uuid,
    submission_id: Uuid,
    document_id: Uuid,
) -> String {
    let client = app.postgres().get().await.expect("connection opens");
    let row = client
        .query_one(
            "SELECT filename, object_key FROM documents WHERE id = $1",
            &[&document_id],
        )
        .await
        .expect("document reads");
    let filename: String = row.get("filename");
    let quarantine_key: String = row.get("object_key");
    let final_key = format!(
        "workspaces/{workspace_id}/evidence-submissions/{submission_id}/documents/{document_id}/{filename}"
    );
    copy_filesystem_object(app, &quarantine_key, &final_key);

    client
        .execute(
            "UPDATE documents SET object_key = $2, upload_status = 'uploaded' WHERE id = $1",
            &[&document_id, &final_key],
        )
        .await
        .expect("document finalizes");

    final_key
}

fn copy_filesystem_object(app: &TestApp, source_key: &str, destination_key: &str) {
    let source_object = app.object_storage_root().join("objects").join(source_key);
    let destination_object = app
        .object_storage_root()
        .join("objects")
        .join(destination_key);
    std::fs::create_dir_all(
        destination_object
            .parent()
            .expect("destination object has parent"),
    )
    .expect("destination object parent creates");
    std::fs::copy(&source_object, &destination_object).expect("object copies");

    let source_metadata = app
        .object_storage_root()
        .join("metadata")
        .join(format!("{source_key}.json"));
    let destination_metadata = app
        .object_storage_root()
        .join("metadata")
        .join(format!("{destination_key}.json"));
    std::fs::create_dir_all(
        destination_metadata
            .parent()
            .expect("destination metadata has parent"),
    )
    .expect("destination metadata parent creates");
    let mut metadata: Value =
        serde_json::from_slice(&std::fs::read(&source_metadata).expect("metadata reads"))
            .expect("metadata parses");
    metadata["key"] = Value::String(destination_key.to_owned());
    std::fs::write(
        destination_metadata,
        serde_json::to_vec_pretty(&metadata).expect("metadata serializes"),
    )
    .expect("metadata writes");
}
