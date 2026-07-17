use chrono::{Duration, Utc};
use proofplane::domain::{
    AgentConnectionId, CoverageWindow, CreateEvidencePayload, CreateEvidenceSubmissionPayload,
    CreateWorkspacePayload, EvidenceId, EvidenceStatus, SubmissionUploadStatus,
    UpdateWorkspacePayload, UserId,
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
    agent_connection_id: AgentConnectionId,
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
async fn submission_scan_work_loads_pending_rows_by_submission_and_quarantine_key() {
    let app = TestApp::start().await;
    let postgres = app.postgres();
    let context = repository_workspace_context(&app).await;
    let evidence = create_repository_evidence(postgres, context).await;
    let submission = create_repository_submission(postgres, context, evidence.id).await;

    let work = postgres
        .load_pending_submission_upload_work(submission.id, &submission.object_key)
        .await
        .expect("pending scan work loads")
        .expect("pending scan work exists");

    assert_eq!(work.workspace_id, evidence.workspace_id);
    assert_eq!(work.evidence_id, evidence.id);
    assert_eq!(work.evidence_submission_id, submission.id);
    assert_eq!(work.filename, submission.filename);
    assert_eq!(work.content_type, submission.content_type);
    assert_eq!(work.content_length, submission.content_length);
    assert_eq!(work.object_key, submission.object_key);
    assert_eq!(work.upload_status, SubmissionUploadStatus::PendingUpload);

    assert!(postgres
        .load_pending_submission_upload_work(submission.id, "stale-key")
        .await
        .expect("stale scan work lookup resolves")
        .is_none());
}

#[tokio::test]
async fn submission_scan_handoff_is_atomic_idempotent_and_finalization_marks_uploaded() {
    let app = TestApp::start().await;
    let postgres = app.postgres();
    let context = repository_workspace_context(&app).await;
    let evidence = create_repository_evidence(postgres, context).await;
    let submission = create_repository_submission(postgres, context, evidence.id).await;
    let quarantine_key = submission.object_key.clone();
    let work = postgres
        .load_pending_submission_upload_work(submission.id, &quarantine_key)
        .await
        .expect("pending work loads")
        .expect("pending work exists");
    let request_id = Uuid::new_v4();

    let message = NewOutboxMessage {
        topic: TopicName::new(MESSAGE_BUS_TOPIC),
        event_type: "submission.finalization_requested".to_owned(),
        aggregate_type: "evidence_submission".to_owned(),
        aggregate_id: Uuid::from(submission.id).to_string(),
        payload: serde_json::json!({
            "evidence_id": Uuid::from(evidence.id).to_string(),
            "object_key": quarantine_key,
        }),
        request_id: Some(request_id),
    };

    let first_work = work.clone();
    let first_message = message.clone();
    assert!(postgres
        .in_transaction(async move |context| {
            let updated = context.request_submission_finalization(&first_work).await?;
            if updated {
                context.append_outbox_message(&first_message).await?;
            }
            Ok(updated)
        })
        .await
        .expect("clean scan hands off"));
    assert!(!postgres
        .in_transaction(async move |context| {
            let updated = context.request_submission_finalization(&work).await?;
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
WHERE event_type = 'submission.finalization_requested'
  AND aggregate_id = $1
"#,
            &[&Uuid::from(submission.id).to_string()],
        )
        .await
        .expect("finalization outbox message loads");
    assert_eq!(
        outbox.get::<_, String>("event_type"),
        "submission.finalization_requested"
    );
    assert_eq!(
        outbox.get::<_, Option<Uuid>>("request_id"),
        Some(request_id)
    );
    assert_eq!(
        outbox.get::<_, serde_json::Value>("payload"),
        serde_json::json!({
            "evidence_id": Uuid::from(evidence.id).to_string(),
            "object_key": quarantine_key,
        })
    );
    drop(client);

    let finalizing = postgres
        .load_finalizing_submission_upload_work(submission.id, &quarantine_key)
        .await
        .expect("finalizing work loads")
        .expect("finalizing work exists");
    assert_eq!(finalizing.upload_status, SubmissionUploadStatus::Finalizing);

    let final_key = format!(
        "workspaces/{}/evidence/{}/submissions/{}/{}",
        evidence.workspace_id, evidence.id, submission.id, submission.filename
    );

    assert!(postgres
        .mark_submission_uploaded(submission.id, &quarantine_key, &final_key,)
        .await
        .expect("finalization marks uploaded"));

    let finalized = postgres
        .in_workspace_context_read(context.workspace_id, async move |context| {
            context.get_evidence_submission(submission.id).await
        })
        .await
        .expect("submission read resolves")
        .expect("submission exists");

    assert_eq!(finalized.object_key, final_key);
    assert_eq!(finalized.upload_status, SubmissionUploadStatus::Uploaded);

    assert!(!postgres
        .mark_submission_uploaded(submission.id, &quarantine_key, "stale-final-key",)
        .await
        .expect("duplicate clean scan resolves"));
}

#[tokio::test]
async fn submission_scan_malicious_and_failed_updates_leave_object_key_quarantined() {
    let app = TestApp::start().await;
    let postgres = app.postgres();
    let context = repository_workspace_context(&app).await;
    let evidence = create_repository_evidence(postgres, context).await;
    let malicious =
        create_repository_submission_with_suffix(postgres, context, evidence.id, "malicious-scan")
            .await;
    let failed =
        create_repository_submission_with_suffix(postgres, context, evidence.id, "failed-scan")
            .await;
    let malicious_key = malicious.object_key.clone();
    let failed_key = failed.object_key.clone();

    assert!(postgres
        .mark_submission_contains_virus(malicious.id, &malicious_key)
        .await
        .expect("malicious scan marks"));
    assert!(postgres
        .mark_submission_upload_failed(failed.id, &failed_key)
        .await
        .expect("failed scan marks"));

    let malicious_detail = postgres
        .in_workspace_context_read(context.workspace_id, async move |context| {
            context.get_evidence_submission(malicious.id).await
        })
        .await
        .expect("malicious submission read resolves")
        .expect("malicious submission exists");
    assert_eq!(malicious_detail.object_key, malicious_key);
    assert_eq!(
        malicious_detail.upload_status,
        SubmissionUploadStatus::ContainsVirus
    );

    let failed_detail = postgres
        .in_workspace_context_read(context.workspace_id, async move |context| {
            context.get_evidence_submission(failed.id).await
        })
        .await
        .expect("failed submission read resolves")
        .expect("failed submission exists");
    assert_eq!(failed_detail.object_key, failed_key);
    assert_eq!(
        failed_detail.upload_status,
        SubmissionUploadStatus::FailedUpload
    );

    assert!(!postgres
        .mark_submission_upload_failed(failed.id, &failed_key)
        .await
        .expect("duplicate failed scan resolves"));
}

#[tokio::test]
async fn outbox_append_commits_atomically_with_domain_write() {
    let app = TestApp::start().await;
    let postgres = app.postgres();
    let context = repository_workspace_context(&app).await;

    let request = postgres
        .in_agent_connection_workspace_context(
            context.workspace_id,
            context.user_id,
            context.agent_connection_id,
            async move |context| {
                let request = context
                    .create_evidence(&CreateEvidencePayload {
                        title: "Outbox Atomic Request".to_owned(),
                        description: "Collect atomic evidence.".to_owned(),
                        collection_instructions: "Upload atomic evidence.".to_owned(),
                        status: EvidenceStatus::Active,
                    })
                    .await?;
                context
                    .append_outbox_message(&outbox_payload(
                        "evidence.created",
                        "evidence",
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
    assert_eq!(rows[0].event_type, "evidence.created");
    assert_eq!(rows[0].aggregate_id, Uuid::from(request.id).to_string());
}

#[tokio::test]
async fn outbox_append_rolls_back_with_domain_write() {
    let app = TestApp::start().await;
    let postgres = app.postgres();
    let context = repository_workspace_context(&app).await;

    let result = postgres
        .in_agent_connection_workspace_context(
            context.workspace_id,
            context.user_id,
            context.agent_connection_id,
            async move |context| {
                context
                    .create_evidence(&CreateEvidencePayload {
                        title: "Rolled Back Outbox Request".to_owned(),
                        description: "Collect rollback evidence.".to_owned(),
                        collection_instructions: "Upload rollback evidence.".to_owned(),
                        status: EvidenceStatus::Active,
                    })
                    .await?;
                context
                    .append_outbox_message(&outbox_payload(
                        "evidence.created",
                        "evidence",
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
        agent_connection_id: token.token_id,
    }
}

async fn create_repository_evidence(
    postgres: &proofplane::repository::Postgres,
    context: RepositoryContext,
) -> proofplane::domain::Evidence {
    postgres
        .in_agent_connection_workspace_context(
            context.workspace_id,
            context.user_id,
            context.agent_connection_id,
            async move |context| {
                context
                    .create_evidence(&CreateEvidencePayload {
                        title: "Repository Evidence".to_owned(),
                        description: "Collect repository evidence.".to_owned(),
                        collection_instructions: "Upload the export.".to_owned(),
                        status: EvidenceStatus::Active,
                    })
                    .await
            },
        )
        .await
        .expect("evidence creates")
}

async fn create_repository_submission(
    postgres: &proofplane::repository::Postgres,
    context: RepositoryContext,
    evidence_id: EvidenceId,
) -> proofplane::domain::EvidenceSubmission {
    create_repository_submission_with_suffix(postgres, context, evidence_id, "detail").await
}

async fn create_repository_submission_with_suffix(
    postgres: &proofplane::repository::Postgres,
    context: RepositoryContext,
    evidence_id: EvidenceId,
    suffix: &str,
) -> proofplane::domain::EvidenceSubmission {
    let suffix = suffix.to_owned();
    postgres
        .in_agent_connection_workspace_context(
            context.workspace_id,
            context.user_id,
            context.agent_connection_id,
            async move |context| {
                context
                    .create_evidence_submission(
                        evidence_id,
                        repository_coverage(),
                        &submission_payload(&suffix),
                    )
                    .await
            },
        )
        .await
        .expect("submission create resolves")
        .expect("submission creates")
}

async fn append_outbox(
    postgres: &proofplane::repository::Postgres,
    context: RepositoryContext,
    aggregate_id: &str,
) -> proofplane::repository::OutboxMessage {
    let aggregate_id = aggregate_id.to_owned();
    postgres
        .in_agent_connection_workspace_context(
            context.workspace_id,
            context.user_id,
            context.agent_connection_id,
            async move |context| {
                context
                    .append_outbox_message(&outbox_payload(
                        "submission.scan_requested",
                        "evidence_submission",
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

fn repository_coverage() -> CoverageWindow {
    CoverageWindow::new(Utc::now() - Duration::days(90), Utc::now())
        .expect("coverage window is ordered")
}

fn submission_payload(label: &str) -> CreateEvidenceSubmissionPayload {
    CreateEvidenceSubmissionPayload {
        filename: format!("{label}.json"),
        content_type: "application/json".to_owned(),
        content_length: 42,
        object_key: format!("evidence/{label}/{}", Uuid::new_v4()),
        checksum_sha256: format!("{label}-sha256"),
        checksum_crc32c: format!("{label}-crc32c"),
    }
}
