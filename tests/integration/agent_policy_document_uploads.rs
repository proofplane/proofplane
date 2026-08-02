use axum::http::StatusCode;
use futures_util::stream;
use proofplane::{
    domain::{AgentPolicyDocumentUploadDeclaration, CreatePolicyPayload, PolicyId},
    object_storage::FilesystemObjectStore,
    repository::CreatePolicyDocumentResult,
    routes::request_context::REQUEST_ID_HEADER,
    services::{
        agent_policy_document_uploads::{
            AgentPolicyDocumentUploadError, AgentPolicyDocumentUploadOutcome,
        },
        policies::PolicyService,
        policy_documents::PolicyDocumentService,
    },
};
use secrecy::ExposeSecret;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{ffi::OsStr, fs, path::PathBuf, sync::Arc, time::Duration};
use tokio::sync::Barrier;
use uuid::Uuid;

use super::support::{capture_audit_logs, capture_logs, TestApp};

#[tokio::test]
async fn valid_machine_stream_completes_through_the_service_boundary() {
    let app = upload_app().await;
    let workspace_id = app.workspace_id("workspace");
    let policy_id = create_policy(&app, workspace_id, "Service policy").await;
    let content = b"machine-provided policy";
    let issued = app
        .agent_policy_document_upload_grant_service()
        .issue(
            &app.agent_connection_context(workspace_id),
            policy_id,
            declaration(content),
        )
        .await
        .expect("policy upload grant issues");

    let recorder = metrics_exporter_prometheus::PrometheusBuilder::new().build_recorder();
    let metrics = recorder.handle();
    let _metrics_guard = metrics::set_default_local_recorder(&recorder);
    let service = app.agent_policy_document_upload_service();
    let credential = issued.credential.expose_secret().to_owned();
    let upload_id = issued.grant.id();
    let (result, audit_logs) = capture_audit_logs(|request_id| async move {
        service
            .upload(
                upload_id,
                &credential,
                "application/pdf",
                content.len() as u64,
                request_id,
                stream::once(async { Ok(bytes::Bytes::from_static(content)) }),
            )
            .await
            .expect("policy upload completes")
    })
    .await;

    assert!(matches!(
        result,
        AgentPolicyDocumentUploadOutcome::Created(_)
    ));
    assert_eq!(result.result().policy_id, policy_id);
    assert_eq!(result.result().document.upload_status.as_str(), "pending");
    assert_eq!(audit_logs.len(), 1);
    let fields = &audit_logs[0]["fields"];
    assert_eq!(
        fields["event_name"],
        "agent_policy_document_upload.completed"
    );
    assert_eq!(fields["operation"], "upload_agent_policy_document");
    assert_eq!(fields["client_type"], "rest");
    assert_eq!(fields["object_id"], upload_id.to_string());
    assert!(!audit_logs[0]
        .to_string()
        .contains(issued.credential.expose_secret()));
    let rendered = metrics.render();
    assert!(rendered
        .contains("proofplane_agent_policy_document_upload_attempts_total{result=\"created\"} 1"));
    assert!(rendered.contains("proofplane_agent_policy_document_upload_received_bytes_total 23"));
}

#[tokio::test]
async fn valid_machine_stream_creates_one_pending_policy_document_and_scan_work() {
    let app = upload_app().await;
    let workspace_id = app.workspace_id("workspace");
    let policy_id = create_policy(&app, workspace_id, "Machine policy").await;
    let content = b"machine-provided policy";
    let issued = app
        .agent_policy_document_upload_grant_service()
        .issue(
            &app.agent_connection_context(workspace_id),
            policy_id,
            declaration(content),
        )
        .await
        .expect("policy upload grant issues");

    let response = app
        .server()
        .put(&format!(
            "/agent-policy-document-uploads/{}",
            issued.grant.id()
        ))
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
    assert_eq!(body["policy_id"], policy_id.to_string());
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
    d.created_by_user_id,
    g.issued_via_agent_connection_id,
    d.filename,
    d.content_type,
    d.content_length,
    d.checksum_sha256,
    d.upload_status,
    g.completed_at,
    g.document_id,
    count(o.id) AS outbox_count
FROM documents d
JOIN agent_policy_document_upload_grants g ON g.document_id = d.id
LEFT JOIN outbox_messages o
  ON o.aggregate_id = d.id::text
 AND o.event_type = 'document.scan_requested'
WHERE d.id = $1
  AND d.owner_type = 'policy'
  AND d.owner_id = $2
GROUP BY d.id, g.id
"#,
            &[&document_id, &Uuid::from(policy_id)],
        )
        .await
        .expect("completed policy upload loads");
    assert_eq!(row.get::<_, Uuid>("created_by_user_id"), app.user_id());
    assert_eq!(
        row.get::<_, Uuid>("issued_via_agent_connection_id"),
        app.api_token_id()
    );
    assert_eq!(row.get::<_, String>("filename"), "policy.pdf");
    assert_eq!(row.get::<_, String>("content_type"), "application/pdf");
    assert_eq!(row.get::<_, i64>("content_length"), content.len() as i64);
    assert_eq!(row.get::<_, String>("checksum_sha256"), sha256(content));
    assert_eq!(row.get::<_, String>("upload_status"), "pending");
    assert!(row
        .get::<_, Option<chrono::DateTime<chrono::Utc>>>("completed_at")
        .is_some());
    assert_eq!(row.get::<_, Option<Uuid>>("document_id"), Some(document_id));
    assert_eq!(row.get::<_, i64>("outbox_count"), 1);
}

#[tokio::test]
async fn matching_retry_returns_the_original_policy_document_without_duplicate_work() {
    let app = upload_app().await;
    let workspace_id = app.workspace_id("workspace");
    let policy_id = create_policy(&app, workspace_id, "Replay policy").await;
    let content = b"machine-provided policy";
    let issued = app
        .agent_policy_document_upload_grant_service()
        .issue(
            &app.agent_connection_context(workspace_id),
            policy_id,
            declaration(content),
        )
        .await
        .expect("policy upload grant issues");
    let recorder = metrics_exporter_prometheus::PrometheusBuilder::new().build_recorder();
    let metrics = recorder.handle();
    let _metrics_guard = metrics::set_default_local_recorder(&recorder);

    let created = upload_request(&app, &issued, content).await;
    created.assert_status(StatusCode::CREATED);
    let created_body: Value = created.json();
    let document_id = created_body["document_id"]
        .as_str()
        .and_then(|value| Uuid::parse_str(value).ok())
        .expect("created document ID is valid");
    app.postgres()
        .get()
        .await
        .expect("database opens")
        .execute(
            "UPDATE documents SET upload_status = 'finalizing' WHERE id = $1",
            &[&document_id],
        )
        .await
        .expect("document status advances");

    let app_ref = &app;
    let issued_ref = &issued;
    let (replayed, replay_logs) = capture_audit_logs(|request_id| async move {
        upload_request_with_request_id(app_ref, issued_ref, content, Some(request_id)).await
    })
    .await;
    replayed.assert_status(StatusCode::OK);
    assert!(replay_logs.is_empty());
    let replayed_body: Value = replayed.json();
    assert_eq!(replayed_body["policy_id"], created_body["policy_id"]);
    assert_eq!(replayed_body["document_id"], created_body["document_id"]);
    assert_eq!(replayed_body["upload_status"], "finalizing");

    let row = app
        .postgres()
        .get()
        .await
        .expect("database opens")
        .query_one(
            r#"
SELECT
    (SELECT count(*) FROM documents WHERE owner_type = 'policy' AND owner_id = $1) AS document_count,
    (SELECT count(*) FROM outbox_messages WHERE aggregate_id = $2::uuid::text) AS outbox_count
"#,
            &[&Uuid::from(policy_id), &document_id],
        )
        .await
        .expect("upload work counts load");
    assert_eq!(row.get::<_, i64>("document_count"), 1);
    assert_eq!(row.get::<_, i64>("outbox_count"), 1);
    let rendered = metrics.render();
    assert!(rendered
        .contains("proofplane_agent_policy_document_upload_attempts_total{result=\"created\"} 1"));
    assert!(rendered
        .contains("proofplane_agent_policy_document_upload_attempts_total{result=\"replayed\"} 1"));
}

#[tokio::test]
async fn concurrent_attempts_under_one_grant_converge_on_one_document() {
    let app = upload_app().await;
    let workspace_id = app.workspace_id("workspace");
    let policy_id = create_policy(&app, workspace_id, "Concurrent policy").await;
    let content = b"machine-provided policy";
    let issued = app
        .agent_policy_document_upload_grant_service()
        .issue(
            &app.agent_connection_context(workspace_id),
            policy_id,
            declaration(content),
        )
        .await
        .expect("policy upload grant issues");
    let service = app.agent_policy_document_upload_service();
    let barrier = Arc::new(Barrier::new(2));
    let upload_id = issued.grant.id();
    let credential = issued.credential.expose_secret().to_owned();
    let recorder = metrics_exporter_prometheus::PrometheusBuilder::new().build_recorder();
    let metrics = recorder.handle();
    let _metrics_guard = metrics::set_default_local_recorder(&recorder);

    let upload = |barrier: Arc<Barrier>, request_id: Uuid| {
        let service = service.clone();
        let credential = credential.clone();
        async move {
            service
                .upload(
                    upload_id,
                    &credential,
                    "application/pdf",
                    content.len() as u64,
                    request_id,
                    stream::once(async move {
                        barrier.wait().await;
                        Ok(bytes::Bytes::from_static(content))
                    }),
                )
                .await
        }
    };

    let ((left, right), audit_logs) = capture_audit_logs(|request_id| async move {
        tokio::join!(
            upload(barrier.clone(), request_id),
            upload(barrier, request_id)
        )
    })
    .await;
    let left = left.expect("left upload resolves");
    let right = right.expect("right upload resolves");
    assert!(matches!(
        (&left, &right),
        (
            AgentPolicyDocumentUploadOutcome::Created(_),
            AgentPolicyDocumentUploadOutcome::Replayed(_)
        ) | (
            AgentPolicyDocumentUploadOutcome::Replayed(_),
            AgentPolicyDocumentUploadOutcome::Created(_)
        )
    ));
    assert_eq!(left.result().document.id(), right.result().document.id());
    assert_eq!(
        audit_logs
            .iter()
            .filter(|record| {
                record["fields"]["event_name"] == "agent_policy_document_upload.completed"
            })
            .count(),
        1
    );
    assert_single_upload_work(&app, policy_id).await;
    assert_eq!(files_under(app.object_storage_root()).len(), 2);
    let rendered = metrics.render();
    assert!(rendered
        .contains("proofplane_agent_policy_document_upload_attempts_total{result=\"created\"} 1"));
    assert!(rendered.contains(
        "proofplane_agent_policy_document_upload_attempts_total{result=\"concurrency_lost\"} 1"
    ));
}

#[tokio::test]
async fn concurrent_grants_for_one_policy_commit_one_document_and_leave_the_loser_retryable() {
    let app = upload_app().await;
    let workspace_id = app.workspace_id("workspace");
    let policy_id = create_policy(&app, workspace_id, "Competing grants policy").await;
    let content = b"machine-provided policy";
    let grants = app.agent_policy_document_upload_grant_service();
    let first = grants
        .issue(
            &app.agent_connection_context(workspace_id),
            policy_id,
            declaration(content),
        )
        .await
        .expect("first policy upload grant issues");
    let second = grants
        .issue(
            &app.agent_connection_context(workspace_id),
            policy_id,
            declaration(content),
        )
        .await
        .expect("second policy upload grant issues");
    let service = app.agent_policy_document_upload_service();
    let barrier = Arc::new(Barrier::new(2));
    let recorder = metrics_exporter_prometheus::PrometheusBuilder::new().build_recorder();
    let metrics = recorder.handle();
    let _metrics_guard = metrics::set_default_local_recorder(&recorder);

    let upload = |issued: &proofplane::services::agent_policy_document_upload_grants::IssuedAgentPolicyDocumentUploadGrant,
                  barrier: Arc<Barrier>,
                  request_id: Uuid| {
        let service = service.clone();
        let credential = issued.credential.expose_secret().to_owned();
        let upload_id = issued.grant.id();
        async move {
            service
                .upload(
                    upload_id,
                    &credential,
                    "application/pdf",
                    content.len() as u64,
                    request_id,
                    stream::once(async move {
                        barrier.wait().await;
                        Ok(bytes::Bytes::from_static(content))
                    }),
                )
                .await
        }
    };

    let first_ref = &first;
    let second_ref = &second;
    let ((left, right), audit_logs) = capture_audit_logs(|request_id| async move {
        tokio::join!(
            upload(first_ref, barrier.clone(), request_id),
            upload(second_ref, barrier, request_id)
        )
    })
    .await;
    assert!(matches!(
        (&left, &right),
        (
            Ok(AgentPolicyDocumentUploadOutcome::Created(_)),
            Err(AgentPolicyDocumentUploadError::CurrentDocument)
        ) | (
            Err(AgentPolicyDocumentUploadError::CurrentDocument),
            Ok(AgentPolicyDocumentUploadOutcome::Created(_))
        )
    ));
    assert_single_upload_work(&app, policy_id).await;
    assert_eq!(
        audit_logs
            .iter()
            .filter(|record| {
                record["fields"]["event_name"] == "agent_policy_document_upload.completed"
            })
            .count(),
        1
    );
    assert_eq!(files_under(app.object_storage_root()).len(), 2);
    let rendered = metrics.render();
    assert!(rendered
        .contains("proofplane_agent_policy_document_upload_attempts_total{result=\"created\"} 1"));
    assert!(rendered.contains(
        "proofplane_agent_policy_document_upload_attempts_total{result=\"current_document\"} 1"
    ));

    let losing = if left.is_err() { &first } else { &second };
    let persisted_loser = app
        .postgres()
        .agent_policy_document_upload_grants()
        .get(losing.grant.id(), workspace_id.into())
        .await
        .expect("losing grant loads")
        .expect("losing grant exists");
    assert!(persisted_loser.completed_at().is_none());
    assert!(persisted_loser.document_id().is_none());

    let winning_document_id = match (&left, &right) {
        (Ok(outcome), _) | (_, Ok(outcome)) => outcome.result().document.id(),
        _ => unreachable!("one upload created the document"),
    };
    app.postgres()
        .get()
        .await
        .expect("database opens")
        .execute(
            "UPDATE documents SET upload_status = 'failed' WHERE id = $1",
            &[&Uuid::from(winning_document_id)],
        )
        .await
        .expect("winning document becomes terminal");
    let object_store = Arc::new(
        FilesystemObjectStore::new(app.object_storage_root())
            .await
            .expect("object store opens"),
    );
    PolicyDocumentService::new(app.postgres_arc(), object_store)
        .archive(
            &app.agent_connection_context(workspace_id),
            Uuid::new_v4(),
            policy_id,
            winning_document_id,
        )
        .await
        .expect("winning document archives");
    upload_request(&app, losing, content)
        .await
        .assert_status(StatusCode::CREATED);
}

#[tokio::test]
async fn authority_and_metadata_rejections_leave_the_grant_retryable_and_storage_clean() {
    let app = upload_app().await;
    let workspace_id = app.workspace_id("workspace");
    let policy_id = create_policy(&app, workspace_id, "Validation policy").await;
    let content = b"machine-provided policy";
    let issued = app
        .agent_policy_document_upload_grant_service()
        .issue(
            &app.agent_connection_context(workspace_id),
            policy_id,
            declaration(content),
        )
        .await
        .expect("policy upload grant issues");
    let path = format!("/agent-policy-document-uploads/{}", issued.grant.id());
    let authorization = format!("Proofplane-Upload {}", issued.credential.expose_secret());
    let recorder = metrics_exporter_prometheus::PrometheusBuilder::new().build_recorder();
    let metrics = recorder.handle();
    let _metrics_guard = metrics::set_default_local_recorder(&recorder);

    app.server()
        .put(&path)
        .add_header("content-type", "application/pdf")
        .add_header("content-length", content.len().to_string())
        .bytes(content.as_slice().into())
        .await
        .assert_status_not_found();
    app.server()
        .put(&path)
        .add_header(
            "authorization",
            format!(
                "Proofplane-Upload {}",
                tamper(issued.credential.expose_secret())
            ),
        )
        .add_header("content-type", "application/pdf")
        .add_header("content-length", content.len().to_string())
        .bytes(content.as_slice().into())
        .await
        .assert_status_not_found();
    app.server()
        .put(&format!(
            "/agent-policy-document-uploads/{}",
            Uuid::new_v4()
        ))
        .add_header("authorization", &authorization)
        .add_header("content-type", "application/pdf")
        .add_header("content-length", content.len().to_string())
        .bytes(content.as_slice().into())
        .await
        .assert_status_not_found();
    app.server()
        .put(&path)
        .add_header("authorization", &authorization)
        .add_header("content-type", "text/plain")
        .add_header("content-length", content.len().to_string())
        .bytes(content.as_slice().into())
        .await
        .assert_status_bad_request();
    app.server()
        .put(&path)
        .add_header("authorization", &authorization)
        .add_header("content-type", "application/pdf")
        .add_header("content-length", content.len().to_string())
        .bytes(content[..content.len() - 1].to_vec().into())
        .await
        .assert_status_bad_request();
    app.server()
        .put(&path)
        .add_header("authorization", &authorization)
        .add_header("content-type", "application/pdf")
        .add_header("content-length", content.len().to_string())
        .bytes(vec![b'x'; content.len()].into())
        .await
        .assert_status_bad_request();

    assert_upload_incomplete(&app, issued.grant.id()).await;
    assert!(files_under(app.object_storage_root()).is_empty());
    upload_request(&app, &issued, content)
        .await
        .assert_status(StatusCode::CREATED);
    let rendered = metrics.render();
    assert!(rendered.contains(
        "proofplane_agent_policy_document_upload_attempts_total{result=\"unavailable\"} 3"
    ));
    assert!(rendered.contains(
        "proofplane_agent_policy_document_upload_attempts_total{result=\"validation_rejected\"} 3"
    ));
}

#[tokio::test]
async fn expired_persisted_authority_is_concealed_without_staging() {
    let app = upload_app().await;
    let workspace_id = app.workspace_id("workspace");
    let policy_id = create_policy(&app, workspace_id, "Expired policy").await;
    let content = b"machine-provided policy";
    let issued = app
        .agent_policy_document_upload_grant_service()
        .issue(
            &app.agent_connection_context(workspace_id),
            policy_id,
            declaration(content),
        )
        .await
        .expect("policy upload grant issues");
    app.postgres()
        .get()
        .await
        .expect("database opens")
        .execute(
            "UPDATE agent_policy_document_upload_grants SET issued_at = now() - interval '10 minutes', expires_at = now() - interval '5 minutes' WHERE id = $1",
            &[&Uuid::from(issued.grant.id())],
        )
        .await
        .expect("grant expires");

    upload_request(&app, &issued, content)
        .await
        .assert_status_not_found();
    assert!(files_under(app.object_storage_root()).is_empty());
}

#[tokio::test]
async fn storage_failure_returns_a_stable_error_without_completing_the_grant() {
    let app = upload_app().await;
    let workspace_id = app.workspace_id("workspace");
    let policy_id = create_policy(&app, workspace_id, "Storage failure policy").await;
    let content = b"machine-provided policy";
    let issued = app
        .agent_policy_document_upload_grant_service()
        .issue(
            &app.agent_connection_context(workspace_id),
            policy_id,
            declaration(content),
        )
        .await
        .expect("policy upload grant issues");
    fs::write(app.object_storage_root().join("objects"), b"blocked")
        .expect("object directory is blocked by a file");
    let recorder = metrics_exporter_prometheus::PrometheusBuilder::new().build_recorder();
    let metrics = recorder.handle();
    let _metrics_guard = metrics::set_default_local_recorder(&recorder);

    upload_request(&app, &issued, content)
        .await
        .assert_status_internal_server_error();
    assert_upload_incomplete(&app, issued.grant.id()).await;
    assert!(metrics.render().contains(
        "proofplane_agent_policy_document_upload_attempts_total{result=\"storage_failed\"} 1"
    ));
}

#[tokio::test]
async fn configured_route_limit_checks_authority_before_rejecting_the_body() {
    let app = TestApp::builder()
        .without_default_auth()
        .with_max_document_bytes(4)
        .workspace("workspace", "Limited policy upload workspace")
        .with_default_membership()
        .build()
        .await;
    let workspace_id = app.workspace_id("workspace");
    let policy_id = create_policy(&app, workspace_id, "Limited policy").await;
    let content = b"large";
    let issued = app
        .agent_policy_document_upload_grant_service()
        .issue(
            &app.agent_connection_context(workspace_id),
            policy_id,
            declaration(content),
        )
        .await
        .expect("policy upload grant issues");
    let recorder = metrics_exporter_prometheus::PrometheusBuilder::new().build_recorder();
    let metrics = recorder.handle();
    let _metrics_guard = metrics::set_default_local_recorder(&recorder);

    let path = format!("/agent-policy-document-uploads/{}", issued.grant.id());
    for authorization in [
        None,
        Some(format!(
            "Proofplane-Upload {}",
            tamper(issued.credential.expose_secret())
        )),
    ] {
        let mut request = app
            .server()
            .put(&path)
            .add_header("content-type", "application/pdf")
            .add_header("content-length", content.len().to_string());
        if let Some(authorization) = authorization {
            request = request.add_header("authorization", authorization);
        }
        request
            .bytes(content.as_slice().into())
            .await
            .assert_status_not_found();
    }

    upload_request(&app, &issued, content)
        .await
        .assert_status(StatusCode::PAYLOAD_TOO_LARGE);
    assert_upload_incomplete(&app, issued.grant.id()).await;
    assert!(files_under(app.object_storage_root()).is_empty());
    assert!(metrics.render().contains(
        "proofplane_agent_policy_document_upload_attempts_total{result=\"validation_rejected\"} 1"
    ));
    assert!(metrics.render().contains(
        "proofplane_agent_policy_document_upload_attempts_total{result=\"unavailable\"} 2"
    ));
}

#[tokio::test]
async fn database_failure_rolls_back_completion_cleans_storage_and_allows_retry() {
    let app = upload_app().await;
    let workspace_id = app.workspace_id("workspace");
    let policy_id = create_policy(&app, workspace_id, "Rollback policy").await;
    let content = b"machine-provided policy";
    let issued = app
        .agent_policy_document_upload_grant_service()
        .issue(
            &app.agent_connection_context(workspace_id),
            policy_id,
            declaration(content),
        )
        .await
        .expect("policy upload grant issues");
    let client = app.postgres().get().await.expect("database opens");
    client
        .batch_execute(
            r#"
CREATE FUNCTION fail_policy_machine_scan_outbox() RETURNS trigger AS $$
BEGIN
    IF NEW.aggregate_type = 'policy_document' THEN
        RAISE EXCEPTION 'injected policy scan outbox failure';
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;
CREATE TRIGGER fail_policy_machine_scan_outbox
BEFORE INSERT ON outbox_messages
FOR EACH ROW EXECUTE FUNCTION fail_policy_machine_scan_outbox();
"#,
        )
        .await
        .expect("failure trigger installs");

    let failed = upload_request(&app, &issued, content).await;
    failed.assert_status_internal_server_error();
    assert_upload_incomplete(&app, issued.grant.id()).await;
    let document_count: i64 = client
        .query_one(
            "SELECT count(*) FROM documents WHERE owner_type = 'policy' AND owner_id = $1",
            &[&Uuid::from(policy_id)],
        )
        .await
        .expect("document count loads")
        .get(0);
    assert_eq!(document_count, 0);
    assert!(files_under(app.object_storage_root()).is_empty());

    client
        .batch_execute(
            r#"
DROP TRIGGER fail_policy_machine_scan_outbox ON outbox_messages;
DROP FUNCTION fail_policy_machine_scan_outbox();
"#,
        )
        .await
        .expect("failure trigger drops");
    upload_request(&app, &issued, content)
        .await
        .assert_status(StatusCode::CREATED);
}

#[tokio::test]
async fn interrupted_stream_removes_partial_storage_and_allows_retry() {
    let app = upload_app().await;
    let workspace_id = app.workspace_id("workspace");
    let policy_id = create_policy(&app, workspace_id, "Interrupted policy").await;
    let content = b"machine-provided policy";
    let issued = app
        .agent_policy_document_upload_grant_service()
        .issue(
            &app.agent_connection_context(workspace_id),
            policy_id,
            declaration(content),
        )
        .await
        .expect("policy upload grant issues");
    let interrupted = stream::iter([
        Ok(bytes::Bytes::from_static(b"partial")),
        Err(proofplane::object_storage::StorageError::StreamRead {
            message: "connection interrupted".to_owned(),
            payload_too_large: false,
        }),
    ]);
    let recorder = metrics_exporter_prometheus::PrometheusBuilder::new().build_recorder();
    let metrics = recorder.handle();
    let _metrics_guard = metrics::set_default_local_recorder(&recorder);

    let result = app
        .agent_policy_document_upload_service()
        .upload(
            issued.grant.id(),
            issued.credential.expose_secret(),
            "application/pdf",
            content.len() as u64,
            Uuid::new_v4(),
            interrupted,
        )
        .await;
    assert!(matches!(
        result,
        Err(AgentPolicyDocumentUploadError::Service(
            proofplane::services::Error::Storage(
                proofplane::object_storage::StorageError::StreamRead { .. }
            )
        ))
    ));
    assert_upload_incomplete(&app, issued.grant.id()).await;
    assert!(files_under(app.object_storage_root()).is_empty());
    assert!(metrics.render().contains(
        "proofplane_agent_policy_document_upload_attempts_total{result=\"stream_failed\"} 1"
    ));

    upload_request(&app, &issued, content)
        .await
        .assert_status(StatusCode::CREATED);
}

#[tokio::test]
async fn cleanup_failure_preserves_the_primary_error_and_emits_safe_diagnostics() {
    let app = upload_app().await;
    let workspace_id = app.workspace_id("workspace");
    let policy_id = create_policy(&app, workspace_id, "Cleanup failure policy").await;
    let content = b"machine-provided policy";
    let issued = app
        .agent_policy_document_upload_grant_service()
        .issue(
            &app.agent_connection_context(workspace_id),
            policy_id,
            declaration(content),
        )
        .await
        .expect("policy upload grant issues");
    app.postgres()
        .get()
        .await
        .expect("database opens")
        .batch_execute(
            r#"
CREATE FUNCTION delay_and_fail_policy_machine_document() RETURNS trigger AS $$
BEGIN
    IF NEW.owner_type = 'policy' THEN
        PERFORM pg_sleep(2);
        RAISE EXCEPTION 'injected policy document failure';
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;
CREATE TRIGGER delay_and_fail_policy_machine_document
BEFORE INSERT ON documents
FOR EACH ROW EXECUTE FUNCTION delay_and_fail_policy_machine_document();
"#,
        )
        .await
        .expect("delayed failure trigger installs");
    let service = app.agent_policy_document_upload_service();
    let root = app.object_storage_root().to_path_buf();
    let credential = issued.credential.expose_secret().to_owned();
    let upload_id = issued.grant.id();
    let recorder = metrics_exporter_prometheus::PrometheusBuilder::new().build_recorder();
    let metrics = recorder.handle();
    let _metrics_guard = metrics::set_default_local_recorder(&recorder);

    let ((result, sabotaged_path), logs) = capture_logs(|request_id| async move {
        tokio::join!(
            service.upload(
                upload_id,
                &credential,
                "application/pdf",
                content.len() as u64,
                request_id,
                stream::once(async { Ok(bytes::Bytes::from_static(content)) }),
            ),
            make_staged_object_undeletable(root),
        )
    })
    .await;

    assert!(matches!(
        result,
        Err(AgentPolicyDocumentUploadError::Repository(_))
    ));
    assert_upload_incomplete(&app, upload_id).await;
    assert!(sabotaged_path.is_dir());
    let cleanup_log = logs
        .iter()
        .find(|record| {
            record["fields"]["operation"] == "agent_policy_document_upload_cleanup"
                && record["fields"]["result"] == "failed"
        })
        .expect("cleanup failure is logged");
    assert!(!logs.iter().any(|record| {
        record["fields"]["event_name"] == "agent_policy_document_upload.completed"
    }));
    let serialized = cleanup_log.to_string();
    assert!(!serialized.contains(issued.credential.expose_secret()));
    assert!(!serialized.contains("policy.pdf"));
    assert!(!serialized.contains(&sha256(content)));
    assert!(!serialized.contains(&sabotaged_path.to_string_lossy().to_string()));
    let rendered = metrics.render();
    assert!(rendered.contains(
        "proofplane_agent_policy_document_upload_attempts_total{result=\"database_failed\"} 1"
    ));
    assert!(rendered.contains("proofplane_cleanup_total"));
    assert!(rendered.contains("operation=\"agent_policy_document_upload_cleanup\""));
    assert!(rendered.contains("result=\"failed\""));
}

#[tokio::test]
async fn machine_and_browser_transfers_race_to_one_current_document() {
    let app = upload_app().await;
    let workspace_id = app.workspace_id("workspace");
    let connection = app.agent_connection_context(workspace_id);
    let policy_id = create_policy(&app, workspace_id, "Browser race policy").await;
    let content = b"machine-provided policy";
    let issued = app
        .agent_policy_document_upload_grant_service()
        .issue(&connection, policy_id, declaration(content))
        .await
        .expect("policy upload grant issues");
    let object_store = Arc::new(
        FilesystemObjectStore::new(app.object_storage_root())
            .await
            .expect("object store opens"),
    );
    let browser_documents = PolicyDocumentService::new(app.postgres_arc(), object_store);
    let browser_payload = browser_documents
        .upload(
            &connection,
            policy_id,
            "browser.pdf".to_owned(),
            "application/pdf".to_owned(),
            stream::once(async { Ok(bytes::Bytes::from_static(b"browser policy")) }),
        )
        .await
        .expect("browser policy stages");
    let machine_service = app.agent_policy_document_upload_service();
    let credential = issued.credential.expose_secret().to_owned();
    let upload_id = issued.grant.id();
    let barrier = Arc::new(Barrier::new(2));

    let machine_barrier = barrier.clone();
    let machine = async move {
        machine_service
            .upload(
                upload_id,
                &credential,
                "application/pdf",
                content.len() as u64,
                Uuid::new_v4(),
                stream::once(async move {
                    machine_barrier.wait().await;
                    Ok(bytes::Bytes::from_static(content))
                }),
            )
            .await
    };
    let browser = async {
        barrier.wait().await;
        browser_documents
            .create(&connection, Uuid::new_v4(), policy_id, browser_payload)
            .await
    };
    let (machine, browser) = tokio::join!(machine, browser);

    assert!(matches!(
        (&machine, &browser),
        (
            Ok(AgentPolicyDocumentUploadOutcome::Created(_)),
            Ok(CreatePolicyDocumentResult::DocumentExists)
        ) | (
            Err(AgentPolicyDocumentUploadError::CurrentDocument),
            Ok(CreatePolicyDocumentResult::Created(_))
        )
    ));
    assert_single_upload_work(&app, policy_id).await;
    assert_eq!(files_under(app.object_storage_root()).len(), 2);
}

async fn upload_app() -> TestApp {
    TestApp::builder()
        .without_default_auth()
        .workspace("workspace", "Policy upload workspace")
        .with_default_membership()
        .build()
        .await
}

async fn create_policy(app: &TestApp, workspace_id: Uuid, name: &str) -> PolicyId {
    PolicyService::new(app.postgres_arc())
        .create(
            app.agent_connection_context(workspace_id),
            CreatePolicyPayload {
                name: name.to_owned(),
                description: None,
                control_ids: vec![],
            },
        )
        .await
        .expect("policy creates")
        .policy
        .id
}

fn declaration(content: &[u8]) -> AgentPolicyDocumentUploadDeclaration {
    AgentPolicyDocumentUploadDeclaration::new(
        "policy.pdf".to_owned(),
        "application/pdf".to_owned(),
        content.len() as u64,
        Some(sha256(content)),
        i64::MAX as u64,
    )
    .into_result()
    .expect("policy upload declaration is valid")
}

fn sha256(content: &[u8]) -> String {
    hex::encode(Sha256::digest(content))
}

fn tamper(token: &str) -> String {
    let mut bytes = token.as_bytes().to_vec();
    if let Some(last) = bytes.last_mut() {
        *last = if *last == b'a' { b'b' } else { b'a' };
    }
    String::from_utf8(bytes).expect("tampered token remains UTF-8")
}

async fn upload_request(
    app: &TestApp,
    issued: &proofplane::services::agent_policy_document_upload_grants::IssuedAgentPolicyDocumentUploadGrant,
    content: &[u8],
) -> axum_test::TestResponse {
    upload_request_with_request_id(app, issued, content, None).await
}

async fn upload_request_with_request_id(
    app: &TestApp,
    issued: &proofplane::services::agent_policy_document_upload_grants::IssuedAgentPolicyDocumentUploadGrant,
    content: &[u8],
    request_id: Option<Uuid>,
) -> axum_test::TestResponse {
    let request = app
        .server()
        .put(&format!(
            "/agent-policy-document-uploads/{}",
            issued.grant.id()
        ))
        .add_header(
            "authorization",
            format!("Proofplane-Upload {}", issued.credential.expose_secret()),
        )
        .add_header("content-type", "application/pdf")
        .add_header("content-length", content.len().to_string());
    let request = if let Some(request_id) = request_id {
        request.add_header(REQUEST_ID_HEADER, request_id.to_string())
    } else {
        request
    };
    request.bytes(content.to_vec().into()).await
}

async fn assert_single_upload_work(app: &TestApp, policy_id: PolicyId) {
    let row = app
        .postgres()
        .get()
        .await
        .expect("database opens")
        .query_one(
            r#"
SELECT
    count(DISTINCT d.id) AS document_count,
    count(DISTINCT o.id) AS outbox_count
FROM documents d
LEFT JOIN outbox_messages o
  ON o.aggregate_id = d.id::text
 AND o.event_type = 'document.scan_requested'
WHERE d.owner_type = 'policy'
  AND d.owner_id = $1
  AND d.archived = false
"#,
            &[&Uuid::from(policy_id)],
        )
        .await
        .expect("upload work counts load");
    assert_eq!(row.get::<_, i64>("document_count"), 1);
    assert_eq!(row.get::<_, i64>("outbox_count"), 1);
}

async fn assert_upload_incomplete(
    app: &TestApp,
    upload_id: proofplane::domain::AgentPolicyDocumentUploadGrantId,
) {
    let grant = app
        .postgres()
        .agent_policy_document_upload_grants()
        .get(upload_id, app.workspace_id("workspace").into())
        .await
        .expect("grant loads")
        .expect("grant exists");
    assert!(grant.completed_at().is_none());
    assert!(grant.document_id().is_none());
}

fn files_under(root: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(path) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(path) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else {
                files.push(path);
            }
        }
    }
    files
}

async fn make_staged_object_undeletable(root: PathBuf) -> PathBuf {
    for _ in 0..200 {
        let files = files_under(&root);
        let metadata_exists = files.iter().any(|path| {
            path.components()
                .any(|part| part.as_os_str() == OsStr::new("metadata"))
        });
        let object = files.into_iter().find(|path| {
            path.components()
                .any(|part| part.as_os_str() == OsStr::new("objects"))
        });
        if metadata_exists {
            if let Some(object) = object {
                fs::remove_file(&object).expect("staged object can be replaced");
                fs::create_dir(&object).expect("staged object path becomes a directory");
                fs::write(object.join("blocker"), b"retain directory")
                    .expect("staged object directory is non-empty");
                return object;
            }
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("staged object appeared before timeout");
}
