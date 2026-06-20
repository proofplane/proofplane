use chrono::{Duration, Utc};
use proofplane::domain::{
    ApiTokenId, AttachmentUploadStatus, CreateEvidenceAttachmentPayload,
    CreateEvidenceRequestPayload, CreateEvidenceSubmissionPayload, CreateWorkspacePayload,
    EvidenceRequestCadence, EvidenceRequestStatus, EvidenceSubmissionId, UpdateWorkspacePayload,
    UserId,
};
use proofplane::pubsub::{TopicName, MESSAGE_BUS_TOPIC};
use proofplane::repository::NewOutboxMessage;
use serde_json::json;
use uuid::Uuid;

use super::support::TestApp;

#[derive(Clone, Copy)]
struct RepositoryContext {
    workspace_id: proofplane::domain::WorkspaceId,
    user_id: UserId,
    api_token_id: ApiTokenId,
}

#[tokio::test]
async fn workspace_repository_crud_uses_typed_rows() {
    let app = TestApp::start().await;
    let postgres = app.postgres();
    let workspace = postgres
        .create_workspace(&CreateWorkspacePayload {
            id: None,
            slug: Some("repository-workspace".to_owned()),
            name: "Repository Workspace".to_owned(),
        })
        .await
        .expect("workspace creates");

    assert_eq!(
        postgres
            .get_workspace(workspace.id)
            .await
            .expect("workspace reads")
            .expect("workspace exists"),
        workspace
    );

    let updated = postgres
        .update_workspace(
            workspace.id,
            &UpdateWorkspacePayload {
                slug: None,
                name: "Renamed Workspace".to_owned(),
            },
        )
        .await
        .expect("workspace updates")
        .expect("workspace exists");
    assert_eq!(updated.slug, None);
    assert_eq!(updated.name, "Renamed Workspace");

    assert!(postgres
        .update_workspace(
            Uuid::new_v4().into(),
            &UpdateWorkspacePayload {
                slug: Some("missing-workspace".to_owned()),
                name: "Missing Workspace".to_owned(),
            },
        )
        .await
        .expect("missing workspace update resolves")
        .is_none());
    assert!(postgres
        .delete_workspace(workspace.id)
        .await
        .expect("workspace deletes"));
    assert!(postgres
        .get_workspace(workspace.id)
        .await
        .expect("deleted workspace reads")
        .is_none());
    assert!(!postgres
        .delete_workspace(workspace.id)
        .await
        .expect("second workspace delete resolves"));
}

#[tokio::test]
async fn attachment_scan_work_loads_pending_rows_by_attachment_and_quarantine_key() {
    let app = TestApp::start().await;
    let postgres = app.postgres();
    let context = repository_workspace_context(&app).await;
    let request = create_repository_evidence_request(postgres, context).await;
    let submission = create_repository_submission(postgres, context, request.id).await;
    let attachment = create_repository_attachment(postgres, context, submission.id).await;

    let work = postgres
        .load_pending_attachment_upload_work(attachment.id, &attachment.object_key)
        .await
        .expect("pending scan work loads")
        .expect("pending scan work exists");

    assert_eq!(work.workspace_id, request.workspace_id);
    assert_eq!(work.evidence_submission_id, submission.id);
    assert_eq!(work.evidence_attachment_id, attachment.id);
    assert_eq!(work.filename, attachment.filename);
    assert_eq!(work.content_type, attachment.content_type);
    assert_eq!(work.content_length, attachment.content_length);
    assert_eq!(work.object_key, attachment.object_key);
    assert_eq!(work.upload_status, AttachmentUploadStatus::PendingUpload);

    assert!(postgres
        .load_pending_attachment_upload_work(attachment.id, "stale-key")
        .await
        .expect("stale scan work lookup resolves")
        .is_none());
}

#[tokio::test]
async fn attachment_scan_handoff_is_atomic_idempotent_and_finalization_marks_uploaded() {
    let app = TestApp::start().await;
    let postgres = app.postgres();
    let context = repository_workspace_context(&app).await;
    let request = create_repository_evidence_request(postgres, context).await;
    let submission = create_repository_submission(postgres, context, request.id).await;
    let attachment = create_repository_attachment(postgres, context, submission.id).await;
    let quarantine_key = attachment.object_key.clone();
    let work = postgres
        .load_pending_attachment_upload_work(attachment.id, &quarantine_key)
        .await
        .expect("pending work loads")
        .expect("pending work exists");
    let request_id = Uuid::new_v4();

    let message = NewOutboxMessage {
        topic: TopicName::new(MESSAGE_BUS_TOPIC),
        event_type: "attachment.finalization_requested".to_owned(),
        aggregate_type: "evidence_attachment".to_owned(),
        aggregate_id: Uuid::from(attachment.id).to_string(),
        payload: serde_json::json!({
            "evidence_submission_id": Uuid::from(submission.id).to_string(),
            "object_key": quarantine_key,
        }),
        request_id: Some(request_id),
    };

    let first_work = work.clone();
    let first_message = message.clone();
    assert!(postgres
        .in_transaction(async move |context| {
            let updated = context.request_attachment_finalization(&first_work).await?;
            if updated {
                context.append_outbox_message(&first_message).await?;
            }
            Ok(updated)
        })
        .await
        .expect("clean scan hands off"));
    assert!(!postgres
        .in_transaction(async move |context| {
            let updated = context.request_attachment_finalization(&work).await?;
            if updated {
                context.append_outbox_message(&message).await?;
            }
            Ok(updated)
        })
        .await
        .expect("duplicate clean scan resolves"));

    let client = postgres.get().await.expect("connection opens");
    let outbox = client
        .query_one(
            r#"
SELECT event_type, aggregate_id, payload, request_id
FROM outbox_messages
WHERE event_type = 'attachment.finalization_requested'
  AND aggregate_id = $1
"#,
            &[&Uuid::from(attachment.id).to_string()],
        )
        .await
        .expect("finalization outbox message loads");
    assert_eq!(
        outbox.get::<_, String>("event_type"),
        "attachment.finalization_requested"
    );
    assert_eq!(
        outbox.get::<_, Option<Uuid>>("request_id"),
        Some(request_id)
    );
    assert_eq!(
        outbox.get::<_, serde_json::Value>("payload"),
        serde_json::json!({
            "evidence_submission_id": Uuid::from(submission.id).to_string(),
            "object_key": quarantine_key,
        })
    );
    drop(client);

    let finalizing = postgres
        .load_finalizing_attachment_upload_work(attachment.id, submission.id, &quarantine_key)
        .await
        .expect("finalizing work loads")
        .expect("finalizing work exists");
    assert_eq!(finalizing.upload_status, AttachmentUploadStatus::Finalizing);

    let final_key = format!(
        "workspaces/{}/evidence-submissions/{}/attachments/{}/{}",
        request.workspace_id, submission.id, attachment.id, attachment.filename
    );

    assert!(postgres
        .mark_attachment_uploaded(attachment.id, &quarantine_key, &final_key,)
        .await
        .expect("finalization marks uploaded"));

    let detail = postgres
        .in_workspace_context_read(context.workspace_id, async move |context| {
            context.get_evidence_submission(submission.id).await
        })
        .await
        .expect("submission detail resolves")
        .expect("submission detail exists");
    let finalized = detail
        .attachments
        .iter()
        .find(|candidate| candidate.id == attachment.id)
        .expect("attachment remains present");

    assert_eq!(finalized.object_key, final_key);
    assert_eq!(finalized.upload_status, AttachmentUploadStatus::Uploaded);

    assert!(!postgres
        .mark_attachment_uploaded(attachment.id, &quarantine_key, "stale-final-key",)
        .await
        .expect("duplicate clean scan resolves"));
}

#[tokio::test]
async fn attachment_scan_malicious_and_failed_updates_leave_object_key_quarantined() {
    let app = TestApp::start().await;
    let postgres = app.postgres();
    let context = repository_workspace_context(&app).await;
    let request = create_repository_evidence_request(postgres, context).await;
    let submission = create_repository_submission(postgres, context, request.id).await;
    let malicious = create_repository_attachment(postgres, context, submission.id).await;
    let failed = postgres
        .in_workspace_context(
            context.workspace_id,
            context.user_id,
            context.api_token_id,
            async move |context| {
                context
                    .create_evidence_attachment(&attachment_payload(submission.id, "failed-scan"))
                    .await
            },
        )
        .await
        .expect("second attachment creates");
    let malicious_key = malicious.object_key.clone();
    let failed_key = failed.object_key.clone();

    assert!(postgres
        .mark_attachment_contains_virus(malicious.id, &malicious_key)
        .await
        .expect("malicious scan marks"));
    assert!(postgres
        .mark_attachment_upload_failed(failed.id, &failed_key)
        .await
        .expect("failed scan marks"));

    let detail = postgres
        .in_workspace_context_read(context.workspace_id, async move |context| {
            context.get_evidence_submission(submission.id).await
        })
        .await
        .expect("submission detail resolves")
        .expect("submission detail exists");

    let malicious_detail = detail
        .attachments
        .iter()
        .find(|candidate| candidate.id == malicious.id)
        .expect("malicious attachment exists");
    assert_eq!(malicious_detail.object_key, malicious_key);
    assert_eq!(
        malicious_detail.upload_status,
        AttachmentUploadStatus::ContainsVirus
    );

    let failed_detail = detail
        .attachments
        .iter()
        .find(|candidate| candidate.id == failed.id)
        .expect("failed attachment exists");
    assert_eq!(failed_detail.object_key, failed_key);
    assert_eq!(
        failed_detail.upload_status,
        AttachmentUploadStatus::FailedUpload
    );

    assert!(!postgres
        .mark_attachment_upload_failed(failed.id, &failed_key)
        .await
        .expect("duplicate failed scan resolves"));
}

#[tokio::test]
async fn outbox_append_commits_atomically_with_domain_write() {
    let app = TestApp::start().await;
    let postgres = app.postgres();
    let context = repository_workspace_context(&app).await;

    let request = postgres
        .in_workspace_context(
            context.workspace_id,
            context.user_id,
            context.api_token_id,
            async move |context| {
                let request = context
                    .create_evidence_request(&CreateEvidenceRequestPayload {
                        title: "Outbox Atomic Request".to_owned(),
                        description: "Collect atomic evidence.".to_owned(),
                        collection_instructions: "Upload atomic evidence.".to_owned(),
                        cadence: EvidenceRequestCadence::Quarterly,
                        due_at: Utc::now() + Duration::days(7),
                        schedule_anchor_at: Utc::now(),
                        freshness_window_days: Some(90),
                        status: EvidenceRequestStatus::Active,
                    })
                    .await?;
                context
                    .append_outbox_message(&outbox_payload(
                        "evidence_request.created",
                        "evidence_request",
                        Uuid::from(request.id).to_string(),
                    ))
                    .await?;

                Ok(request)
            },
        )
        .await
        .expect("request and outbox commit");

    let rows = postgres
        .list_due_outbox_messages(Utc::now() + Duration::seconds(1), 10)
        .await
        .expect("outbox rows list");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].event_type, "evidence_request.created");
    assert_eq!(rows[0].aggregate_id, Uuid::from(request.id).to_string());
}

#[tokio::test]
async fn outbox_append_rolls_back_with_domain_write() {
    let app = TestApp::start().await;
    let postgres = app.postgres();
    let context = repository_workspace_context(&app).await;

    let result = postgres
        .in_workspace_context(
            context.workspace_id,
            context.user_id,
            context.api_token_id,
            async move |context| {
                context
                    .create_evidence_request(&CreateEvidenceRequestPayload {
                        title: "Rolled Back Outbox Request".to_owned(),
                        description: "Collect rollback evidence.".to_owned(),
                        collection_instructions: "Upload rollback evidence.".to_owned(),
                        cadence: EvidenceRequestCadence::Quarterly,
                        due_at: Utc::now() + Duration::days(7),
                        schedule_anchor_at: Utc::now(),
                        freshness_window_days: Some(90),
                        status: EvidenceRequestStatus::Active,
                    })
                    .await?;
                context
                    .append_outbox_message(&outbox_payload(
                        "evidence_request.created",
                        "evidence_request",
                        "rolled-back",
                    ))
                    .await?;

                Err::<(), proofplane::repository::Error>(proofplane::repository::Error::Conflict(
                    proofplane::repository::ConflictKind::WorkspaceSlugTaken,
                ))
            },
        )
        .await;

    assert!(matches!(
        result,
        Err(proofplane::repository::Error::Conflict(
            proofplane::repository::ConflictKind::WorkspaceSlugTaken
        ))
    ));
    let rows = postgres
        .list_due_outbox_messages(Utc::now() + Duration::seconds(1), 10)
        .await
        .expect("outbox rows list");
    assert!(rows.is_empty());
}

#[tokio::test]
async fn outbox_repository_lists_due_rows_deletes_successes_and_schedules_failures() {
    let app = TestApp::start().await;
    let postgres = app.postgres();
    let context = repository_workspace_context(&app).await;

    let first = append_outbox(postgres, context, "first").await;
    let second = append_outbox(postgres, context, "second").await;
    let future = append_outbox(postgres, context, "future").await;

    set_outbox_next_available_at(postgres, first.id, Utc::now() - Duration::minutes(5)).await;
    set_outbox_next_available_at(postgres, second.id, Utc::now() - Duration::minutes(1)).await;
    set_outbox_next_available_at(postgres, future.id, Utc::now() + Duration::hours(1)).await;

    let due = postgres
        .list_due_outbox_messages(Utc::now(), 10)
        .await
        .expect("due rows list");
    assert_eq!(
        due.iter()
            .map(|row| row.aggregate_id.as_str())
            .collect::<Vec<_>>(),
        vec!["first", "second"]
    );

    assert!(postgres
        .delete_outbox_message(first.id)
        .await
        .expect("published row deletes"));
    assert!(!postgres
        .delete_outbox_message(first.id)
        .await
        .expect("second delete is false"));

    let retry_at = Utc::now() + Duration::minutes(2);
    assert!(postgres
        .record_outbox_publish_failure(second.id, retry_at)
        .await
        .expect("failure records"));

    let client = postgres.get().await.expect("connection opens");
    let row = client
        .query_one(
            "SELECT attempt_count, next_available_at FROM outbox_messages WHERE id = $1",
            &[&second.id],
        )
        .await
        .expect("failed row reads");
    assert_eq!(row.get::<_, i32>("attempt_count"), 1);
    assert_eq!(
        row.get::<_, chrono::DateTime<Utc>>("next_available_at"),
        retry_at
    );

    let exhausted = postgres
        .list_exhausted_outbox_messages(1, 10)
        .await
        .expect("exhausted rows list");
    assert_eq!(
        exhausted
            .iter()
            .map(|row| row.aggregate_id.as_str())
            .collect::<Vec<_>>(),
        vec!["second"]
    );
}

async fn repository_workspace_context(app: &TestApp) -> RepositoryContext {
    let workspace = app
        .postgres()
        .create_workspace(&CreateWorkspacePayload {
            id: None,
            slug: None,
            name: "Repository Evidence Workspace".to_owned(),
        })
        .await
        .expect("workspace creates");
    let token = app
        .issue_api_token(
            workspace.id.into(),
            proofplane::domain::WorkspacePermission::ALL.to_vec(),
        )
        .await;

    RepositoryContext {
        workspace_id: workspace.id,
        user_id: token.user_id,
        api_token_id: token.token_id,
    }
}

async fn create_repository_evidence_request(
    postgres: &proofplane::repository::Postgres,
    context: RepositoryContext,
) -> proofplane::domain::EvidenceRequest {
    postgres
        .in_workspace_context(
            context.workspace_id,
            context.user_id,
            context.api_token_id,
            async move |context| {
                context
                    .create_evidence_request(&CreateEvidenceRequestPayload {
                        title: "Repository Evidence Request".to_owned(),
                        description: "Collect repository evidence.".to_owned(),
                        collection_instructions: "Upload the export.".to_owned(),
                        cadence: EvidenceRequestCadence::Quarterly,
                        due_at: Utc::now() + Duration::days(7),
                        schedule_anchor_at: Utc::now(),
                        freshness_window_days: Some(90),
                        status: EvidenceRequestStatus::Active,
                    })
                    .await
            },
        )
        .await
        .expect("evidence request creates")
}

async fn create_repository_submission(
    postgres: &proofplane::repository::Postgres,
    context: RepositoryContext,
    evidence_request_id: proofplane::domain::EvidenceRequestId,
) -> proofplane::domain::EvidenceSubmission {
    postgres
        .in_workspace_context(
            context.workspace_id,
            context.user_id,
            context.api_token_id,
            async move |context| {
                context
                    .create_evidence_submission(&submission_payload(evidence_request_id))
                    .await
            },
        )
        .await
        .expect("submission create resolves")
        .expect("submission creates")
}

async fn create_repository_attachment(
    postgres: &proofplane::repository::Postgres,
    context: RepositoryContext,
    submission_id: EvidenceSubmissionId,
) -> proofplane::domain::EvidenceAttachment {
    create_repository_attachment_with_suffix(postgres, context, submission_id, "detail").await
}

async fn create_repository_attachment_with_suffix(
    postgres: &proofplane::repository::Postgres,
    context: RepositoryContext,
    submission_id: EvidenceSubmissionId,
    suffix: &str,
) -> proofplane::domain::EvidenceAttachment {
    let suffix = suffix.to_owned();
    postgres
        .in_workspace_context(
            context.workspace_id,
            context.user_id,
            context.api_token_id,
            async move |context| {
                context
                    .create_evidence_attachment(&attachment_payload(submission_id, &suffix))
                    .await
            },
        )
        .await
        .expect("attachment creates")
}

async fn append_outbox(
    postgres: &proofplane::repository::Postgres,
    context: RepositoryContext,
    aggregate_id: &str,
) -> proofplane::repository::OutboxMessage {
    let aggregate_id = aggregate_id.to_owned();
    postgres
        .in_workspace_context(
            context.workspace_id,
            context.user_id,
            context.api_token_id,
            async move |context| {
                context
                    .append_outbox_message(&outbox_payload(
                        "attachment.scan_requested",
                        "evidence_attachment",
                        aggregate_id,
                    ))
                    .await
            },
        )
        .await
        .expect("outbox appends")
}

async fn set_outbox_next_available_at(
    postgres: &proofplane::repository::Postgres,
    id: i64,
    next_available_at: chrono::DateTime<Utc>,
) {
    let client = postgres.get().await.expect("connection opens");
    client
        .execute(
            "UPDATE outbox_messages SET next_available_at = $2 WHERE id = $1",
            &[&id, &next_available_at],
        )
        .await
        .expect("outbox next_available_at updates");
}

fn outbox_payload(
    event_type: &str,
    aggregate_type: &str,
    aggregate_id: impl Into<String>,
) -> NewOutboxMessage {
    NewOutboxMessage {
        topic: TopicName::new(MESSAGE_BUS_TOPIC),
        event_type: event_type.to_owned(),
        aggregate_type: aggregate_type.to_owned(),
        aggregate_id: aggregate_id.into(),
        payload: json!({ "id": "payload-id" }),
        request_id: None,
    }
}

fn submission_payload(
    evidence_request_id: proofplane::domain::EvidenceRequestId,
) -> CreateEvidenceSubmissionPayload {
    CreateEvidenceSubmissionPayload {
        evidence_request_id,
        coverage_start_at: Utc::now() - Duration::days(90),
        coverage_end_at: Utc::now(),
        source_system: "github".to_owned(),
        collection_method: "api_export".to_owned(),
    }
}

fn attachment_payload(
    evidence_submission_id: EvidenceSubmissionId,
    label: &str,
) -> CreateEvidenceAttachmentPayload {
    CreateEvidenceAttachmentPayload {
        evidence_submission_id,
        filename: format!("{label}.json"),
        content_type: "application/json".to_owned(),
        content_length: 42,
        object_key: format!("evidence/{label}/{}", Uuid::new_v4()),
        checksum_sha256: format!("{label}-sha256"),
        checksum_crc32c: format!("{label}-crc32c"),
    }
}
