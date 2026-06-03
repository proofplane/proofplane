use chrono::{Duration, Utc};
use proofplane::domain::{
    ActorId, ActorKind, AttachmentScanStatus, CreateActorPayload, CreateApiCredentialPayload,
    CreateControlPayload, CreateEvidenceAttachmentPayload,
    CreateEvidenceRequestControlMappingPayload, CreateEvidenceRequestPayload,
    CreateEvidenceSubmissionPayload, CreateWorkspacePayload, EvidenceRequestCadence,
    EvidenceRequestStatus, EvidenceSubmissionId, FrameworkRequirementId, UpdateActorPayload,
    UpdateApiCredentialPayload, UpdateControlPayload, UpdateWorkspacePayload,
};
use proofplane::pubsub::TopicName;
use proofplane::repository::NewOutboxMessage;
use proofplane::routes::authentication::ActorContext;
use serde_json::json;
use uuid::Uuid;

use super::support::{cc61_id, cc71_id, TestApp, INTEGRATION_ACTOR_ID};

#[tokio::test]
async fn actor_repository_crud_uses_typed_rows() {
    let app = TestApp::start().await;
    let postgres = app.postgres();
    let actor = postgres
        .create_actor(&CreateActorPayload {
            id: Some(ActorId::from(Uuid::new_v4())),
            kind: ActorKind::HumanUser,
            display_name: "Repository Human".to_owned(),
        })
        .await
        .expect("actor creates");

    assert_eq!(
        postgres
            .get_actor(actor.id)
            .await
            .expect("actor reads")
            .expect("actor exists"),
        actor
    );
    assert!(postgres
        .list_actors()
        .await
        .expect("actors list")
        .contains(&actor));

    let updated = postgres
        .update_actor(
            actor.id,
            &UpdateActorPayload {
                kind: ActorKind::ServiceAccount,
                display_name: "Repository Service".to_owned(),
            },
        )
        .await
        .expect("actor updates")
        .expect("actor exists");
    assert_eq!(updated.kind, ActorKind::ServiceAccount);
    assert_eq!(updated.display_name, "Repository Service");
    assert!(postgres
        .list_actors()
        .await
        .expect("actors list")
        .contains(&updated));

    assert!(postgres
        .update_actor(
            ActorId::from(Uuid::new_v4()),
            &UpdateActorPayload {
                kind: ActorKind::System,
                display_name: "Missing".to_owned(),
            },
        )
        .await
        .expect("missing actor update resolves")
        .is_none());
    assert!(postgres
        .delete_actor(actor.id)
        .await
        .expect("actor deletes"));
    assert!(postgres
        .get_actor(actor.id)
        .await
        .expect("deleted actor reads")
        .is_none());
    assert!(!postgres
        .delete_actor(actor.id)
        .await
        .expect("second actor delete resolves"));
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
        .list_workspaces()
        .await
        .expect("workspaces list")
        .contains(&updated));

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
async fn api_credential_repository_crud_uses_lifecycle_fields() {
    let app = TestApp::start().await;
    let postgres = app.postgres();
    let actor = postgres
        .create_actor(&CreateActorPayload {
            id: None,
            kind: ActorKind::Integration,
            display_name: "Credential Actor".to_owned(),
        })
        .await
        .expect("credential actor creates");
    let credential = postgres
        .create_api_credential(&CreateApiCredentialPayload {
            id: "repository-api-key".to_owned(),
            actor_id: actor.id,
            name: "Repository API Key".to_owned(),
            key_id: "first-key-id".to_owned(),
            credential_hash: "first-credential-hash".to_owned(),
            expires_at: Some(Utc::now() + Duration::days(1)),
            revoked_at: None,
        })
        .await
        .expect("API credential creates");

    assert_eq!(
        postgres
            .get_api_credential(&credential.id)
            .await
            .expect("API credential reads")
            .expect("API credential exists"),
        credential
    );

    let updated = postgres
        .update_api_credential(
            &credential.id,
            &UpdateApiCredentialPayload {
                name: "Rotated Repository API Key".to_owned(),
                key_id: "rotated-key-id".to_owned(),
                credential_hash: "rotated-credential-hash".to_owned(),
                expires_at: None,
                revoked_at: Some(Utc::now()),
            },
        )
        .await
        .expect("API credential updates")
        .expect("API credential exists");
    assert_eq!(updated.credential_hash, "rotated-credential-hash");
    assert_eq!(updated.key_id, "rotated-key-id");
    assert!(updated.expires_at.is_none());
    assert!(updated.revoked_at.is_some());
    let actor_with_credential = postgres
        .actor_with_api_credential(actor.id)
        .await
        .expect("actor credential reads")
        .expect("actor exists");
    assert_eq!(actor_with_credential.actor, actor);
    assert_eq!(actor_with_credential.api_credential, updated.clone());
    assert!(postgres
        .list_api_credentials()
        .await
        .expect("API credentials list")
        .contains(&updated));

    assert!(postgres
        .update_api_credential(
            "missing-api-key",
            &UpdateApiCredentialPayload {
                name: "Missing API Key".to_owned(),
                key_id: "missing-key-id".to_owned(),
                credential_hash: "missing-credential-hash".to_owned(),
                expires_at: None,
                revoked_at: None,
            },
        )
        .await
        .expect("missing API credential update resolves")
        .is_none());
    assert!(postgres
        .delete_api_credential(&credential.id)
        .await
        .expect("API credential deletes"));
    assert!(postgres
        .get_api_credential(&credential.id)
        .await
        .expect("deleted API credential reads")
        .is_none());
    assert!(!postgres
        .delete_api_credential(&credential.id)
        .await
        .expect("second API credential delete resolves"));
    assert!(postgres
        .delete_actor(actor.id)
        .await
        .expect("credential actor deletes"));
}

#[tokio::test]
async fn api_credential_repository_enforces_one_credential_per_actor() {
    let app = TestApp::start().await;
    let postgres = app.postgres();
    let actor = postgres
        .create_actor(&CreateActorPayload {
            id: None,
            kind: ActorKind::Integration,
            display_name: "Single Credential Actor".to_owned(),
        })
        .await
        .expect("credential actor creates");

    for (id, key_id) in [
        ("first-api-key", "first-key-id"),
        ("second-api-key", "second-key-id"),
    ] {
        let result = postgres
            .create_api_credential(&CreateApiCredentialPayload {
                id: id.to_owned(),
                actor_id: actor.id,
                name: id.to_owned(),
                key_id: key_id.to_owned(),
                credential_hash: format!("{id}-hash"),
                expires_at: None,
                revoked_at: None,
            })
            .await;

        if id == "first-api-key" {
            result.expect("first API credential creates");
        } else {
            result.expect_err("second API credential violates actor constraint");
        }
    }
}

#[tokio::test]
async fn control_repository_create_returns_created_control_and_replace_masks_missing_rows() {
    let app = TestApp::builder().with_soc2_reference_data().build().await;
    let postgres = app.postgres();
    let actor = repository_actor_context(&app).await;
    let other_actor = repository_actor_context(&app).await;

    let created = postgres
        .in_actor_context(actor.clone(), async move |context| {
            context
                .create_control(&control_payload_with_requirements(
                    "PP-CTRL-01",
                    vec![cc71_id().into(), cc61_id().into()],
                ))
                .await
        })
        .await
        .expect("control creates");

    assert_eq!(created.workspace_id, actor.workspace_id);
    assert_eq!(created.code, "PP-CTRL-01");
    assert_eq!(
        requirement_ids(&created.framework_requirements),
        vec![cc61_id(), cc71_id()]
    );

    let updated = postgres
        .in_actor_context(actor.clone(), async move |context| {
            context
                .replace_control(
                    created.id,
                    &update_control_payload_with_requirements("PP-CTRL-02", vec![cc71_id().into()]),
                )
                .await
        })
        .await
        .expect("control replace resolves")
        .expect("control replaces");

    assert_eq!(updated.id, created.id);
    assert_eq!(updated.code, "PP-CTRL-02");
    assert_eq!(
        requirement_ids(&updated.framework_requirements),
        vec![cc71_id()]
    );

    let missing = postgres
        .in_actor_context(actor.clone(), async move |context| {
            context
                .replace_control(Uuid::new_v4().into(), &update_control_payload("PP-MISSING"))
                .await
        })
        .await
        .expect("missing control replace resolves");
    assert!(missing.is_none());

    // Ensure that replacing a control with an ID that the actor doesn't have access to
    // doesn't leak information about the workspace.
    let cross_workspace = postgres
        .in_actor_context(other_actor, async move |context| {
            context
                .replace_control(created.id, &update_control_payload("PP-CROSS"))
                .await
        })
        .await
        .expect("cross-workspace control replace resolves");
    assert!(cross_workspace.is_none());
}

#[tokio::test]
async fn mapping_repository_create_returns_mapping_and_masks_guarded_absence() {
    let app = TestApp::start().await;
    let postgres = app.postgres();
    let actor = repository_actor_context(&app).await;
    let other_actor = repository_actor_context(&app).await;
    let request = create_repository_evidence_request(postgres, actor.clone()).await;
    let control = postgres
        .in_actor_context(actor.clone(), async move |context| {
            context.create_control(&control_payload("PP-MAP-01")).await
        })
        .await
        .expect("control creates");
    let other_control = postgres
        .in_actor_context(other_actor.clone(), async move |context| {
            context.create_control(&control_payload("PP-MAP-02")).await
        })
        .await
        .expect("other control creates");

    let created = postgres
        .in_actor_context(actor.clone(), async move |context| {
            context
                .create_evidence_request_control_mapping(&mapping_payload(
                    request.id,
                    control.id,
                    "Repository mapping rationale.",
                ))
                .await
        })
        .await
        .expect("mapping create resolves")
        .expect("mapping creates");

    assert_eq!(created.evidence_request_id, request.id);
    assert_eq!(created.control.id, control.id);
    assert_eq!(created.rationale, "Repository mapping rationale.");

    let missing_request = postgres
        .in_actor_context(actor.clone(), async move |context| {
            context
                .create_evidence_request_control_mapping(&mapping_payload(
                    Uuid::new_v4().into(),
                    control.id,
                    "Missing request.",
                ))
                .await
        })
        .await
        .expect("missing request mapping create resolves");
    assert!(missing_request.is_none());

    // Ensure that creating mappings in a workspace the actor doesn't have access to
    // doesn't leak information.
    let cross_workspace_control = postgres
        .in_actor_context(actor.clone(), async move |context| {
            context
                .create_evidence_request_control_mapping(&mapping_payload(
                    request.id,
                    other_control.id,
                    "Cross-workspace control.",
                ))
                .await
        })
        .await
        .expect("cross-workspace control mapping create resolves");
    assert!(cross_workspace_control.is_none());
}

#[tokio::test]
async fn evidence_submission_create_scopes_to_workspace_and_records_context_actor() {
    let app = TestApp::start().await;
    let postgres = app.postgres();
    let actor = repository_actor_context(&app).await;
    let other_actor = repository_actor_context(&app).await;
    let request = create_repository_evidence_request(postgres, actor.clone()).await;

    let submission = postgres
        .in_actor_context(actor.clone(), async move |context| {
            context
                .create_evidence_submission(&submission_payload(request.id))
                .await
        })
        .await
        .expect("submission create resolves")
        .expect("submission creates");

    assert_eq!(submission.evidence_request_id, request.id);
    assert_eq!(submission.submitted_by, actor.id);
    assert_eq!(submission.source_system, "github");
    assert_eq!(submission.provenance, json!({ "run_id": "123" }));

    let missing = postgres
        .in_actor_context(actor.clone(), async move |context| {
            context
                .create_evidence_submission(&submission_payload(Uuid::new_v4().into()))
                .await
        })
        .await
        .expect("missing request create resolves");
    assert!(missing.is_none());

    let cross_workspace = postgres
        .in_actor_context(other_actor, async move |context| {
            context
                .create_evidence_submission(&submission_payload(request.id))
                .await
        })
        .await
        .expect("cross-workspace request create resolves");
    assert!(cross_workspace.is_none());
}

#[tokio::test]
async fn evidence_submission_detail_starts_with_empty_attachment_list() {
    let app = TestApp::start().await;
    let postgres = app.postgres();
    let actor = repository_actor_context(&app).await;
    let request = create_repository_evidence_request(postgres, actor.clone()).await;
    let submission = create_repository_submission(postgres, actor.clone(), request.id).await;

    let detail = postgres
        .in_actor_context_read(actor, async move |context| {
            context.get_evidence_submission(submission.id).await
        })
        .await
        .expect("submission detail resolves")
        .expect("submission detail exists");

    assert_eq!(detail.submission, submission);
    assert!(detail.attachments.is_empty());
}

#[tokio::test]
async fn evidence_attachment_create_scopes_to_workspace_and_creates_pending_scan() {
    let app = TestApp::start().await;
    let postgres = app.postgres();
    let actor = repository_actor_context(&app).await;
    let other_actor = repository_actor_context(&app).await;
    let request = create_repository_evidence_request(postgres, actor.clone()).await;
    let submission = create_repository_submission(postgres, actor.clone(), request.id).await;

    let attachment = postgres
        .in_actor_context(actor.clone(), async move |context| {
            context
                .create_evidence_attachment(&attachment_payload(submission.id, "first"))
                .await
        })
        .await
        .expect("attachment creates");

    assert_eq!(attachment.attachment.evidence_submission_id, submission.id);
    assert_eq!(
        attachment.scan.evidence_attachment_id,
        attachment.attachment.id
    );
    assert_eq!(attachment.scan.scan_status, AttachmentScanStatus::Pending);
    assert!(attachment.scan.scanner_name.is_none());

    let missing = postgres
        .in_actor_context(actor.clone(), async move |context| {
            context
                .create_evidence_attachment(&attachment_payload(Uuid::new_v4().into(), "missing"))
                .await
        })
        .await;
    assert!(matches!(
        missing,
        Err(proofplane::repository::Error::InvariantViolation(
            "attachment insert requires an existing workspace-scoped submission"
        ))
    ));

    let cross_workspace = postgres
        .in_actor_context(other_actor, async move |context| {
            context
                .create_evidence_attachment(&attachment_payload(submission.id, "cross-workspace"))
                .await
        })
        .await;
    assert!(matches!(
        cross_workspace,
        Err(proofplane::repository::Error::InvariantViolation(
            "attachment insert requires an existing workspace-scoped submission"
        ))
    ));
}

#[tokio::test]
async fn evidence_submission_detail_includes_attachments_with_scan_rows() {
    let app = TestApp::start().await;
    let postgres = app.postgres();
    let actor = repository_actor_context(&app).await;
    let request = create_repository_evidence_request(postgres, actor.clone()).await;
    let submission = create_repository_submission(postgres, actor.clone(), request.id).await;
    let attachment = create_repository_attachment(postgres, actor.clone(), submission.id).await;

    let detail = postgres
        .in_actor_context_read(actor, async move |context| {
            context.get_evidence_submission(submission.id).await
        })
        .await
        .expect("submission detail resolves")
        .expect("submission detail exists");

    assert_eq!(detail.submission, submission);
    assert_eq!(detail.attachments, vec![attachment]);
}

#[tokio::test]
async fn latest_evidence_submission_for_request_returns_newest_visible_submission() {
    let app = TestApp::start().await;
    let postgres = app.postgres();
    let actor = repository_actor_context(&app).await;
    let other_actor = repository_actor_context(&app).await;
    let request = create_repository_evidence_request(postgres, actor.clone()).await;
    let first = create_repository_submission(postgres, actor.clone(), request.id).await;
    let latest = create_repository_submission(postgres, actor.clone(), request.id).await;

    set_submission_received_at(postgres, first.id, Utc::now() - Duration::days(1)).await;
    set_submission_received_at(postgres, latest.id, Utc::now()).await;

    let detail = postgres
        .in_actor_context_read(actor.clone(), async move |context| {
            context
                .latest_evidence_submission_for_request(request.id)
                .await
        })
        .await
        .expect("latest submission resolves")
        .expect("latest submission exists");
    assert_eq!(detail.submission.id, latest.id);

    let missing = postgres
        .in_actor_context_read(actor.clone(), async move |context| {
            context
                .latest_evidence_submission_for_request(Uuid::new_v4().into())
                .await
        })
        .await
        .expect("missing latest resolves");
    assert!(missing.is_none());

    let cross_workspace = postgres
        .in_actor_context_read(other_actor, async move |context| {
            context
                .latest_evidence_submission_for_request(request.id)
                .await
        })
        .await
        .expect("cross-workspace latest resolves");
    assert!(cross_workspace.is_none());
}

#[tokio::test]
async fn outbox_append_commits_atomically_with_domain_write() {
    let app = TestApp::start().await;
    let postgres = app.postgres();
    let actor = repository_actor_context(&app).await;

    let request = postgres
        .in_actor_context(actor.clone(), async move |context| {
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
        })
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
    let actor = repository_actor_context(&app).await;

    let result = postgres
        .in_actor_context(actor.clone(), async move |context| {
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
                "force rollback",
            ))
        })
        .await;

    assert!(matches!(
        result,
        Err(proofplane::repository::Error::Conflict("force rollback"))
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
    let actor = repository_actor_context(&app).await;

    let first = append_outbox(postgres, actor.clone(), "first").await;
    let second = append_outbox(postgres, actor.clone(), "second").await;
    let future = append_outbox(postgres, actor, "future").await;

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

async fn repository_actor_context(app: &TestApp) -> ActorContext {
    let workspace = app
        .postgres()
        .create_workspace(&CreateWorkspacePayload {
            id: None,
            slug: None,
            name: "Repository Evidence Workspace".to_owned(),
        })
        .await
        .expect("workspace creates");
    let actor_id = ActorId::from(Uuid::parse_str(INTEGRATION_ACTOR_ID).unwrap());

    ActorContext::new(workspace.id, actor_id)
}

async fn create_repository_evidence_request(
    postgres: &proofplane::repository::Postgres,
    actor: ActorContext,
) -> proofplane::domain::EvidenceRequest {
    postgres
        .in_actor_context(actor, async move |context| {
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
        })
        .await
        .expect("evidence request creates")
}

async fn create_repository_submission(
    postgres: &proofplane::repository::Postgres,
    actor: ActorContext,
    evidence_request_id: proofplane::domain::EvidenceRequestId,
) -> proofplane::domain::EvidenceSubmission {
    postgres
        .in_actor_context(actor, async move |context| {
            context
                .create_evidence_submission(&submission_payload(evidence_request_id))
                .await
        })
        .await
        .expect("submission create resolves")
        .expect("submission creates")
}

async fn create_repository_attachment(
    postgres: &proofplane::repository::Postgres,
    actor: ActorContext,
    submission_id: EvidenceSubmissionId,
) -> proofplane::domain::EvidenceAttachmentWithScan {
    postgres
        .in_actor_context(actor, async move |context| {
            context
                .create_evidence_attachment(&attachment_payload(submission_id, "detail"))
                .await
        })
        .await
        .expect("attachment creates")
}

async fn set_submission_received_at(
    postgres: &proofplane::repository::Postgres,
    submission_id: EvidenceSubmissionId,
    received_at: chrono::DateTime<Utc>,
) {
    let client = postgres.get().await.expect("connection opens");
    client
        .execute(
            "UPDATE evidence_submissions SET received_at = $2 WHERE id = $1",
            &[&Uuid::from(submission_id), &received_at],
        )
        .await
        .expect("submission received_at updates");
}

async fn append_outbox(
    postgres: &proofplane::repository::Postgres,
    actor: ActorContext,
    aggregate_id: &str,
) -> proofplane::repository::OutboxMessage {
    let aggregate_id = aggregate_id.to_owned();
    postgres
        .in_actor_context(actor, async move |context| {
            context
                .append_outbox_message(&outbox_payload(
                    "attachment.scan_requested",
                    "evidence_attachment",
                    aggregate_id,
                ))
                .await
        })
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
        topic: TopicName::new("integration-outbox"),
        event_type: event_type.to_owned(),
        aggregate_type: aggregate_type.to_owned(),
        aggregate_id: aggregate_id.into(),
        payload: json!({ "id": "payload-id" }),
        attributes: json!({ "source": "integration-test" }),
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
        provenance: json!({ "run_id": "123" }),
    }
}

fn control_payload(code: &str) -> CreateControlPayload {
    control_payload_with_requirements(code, Vec::new())
}

fn control_payload_with_requirements(
    code: &str,
    framework_requirement_ids: Vec<FrameworkRequirementId>,
) -> CreateControlPayload {
    CreateControlPayload {
        code: code.to_owned(),
        title: format!("Repository control {code}"),
        description: format!("Repository control description for {code}."),
        framework_requirement_ids,
    }
}

fn update_control_payload(code: &str) -> UpdateControlPayload {
    update_control_payload_with_requirements(code, Vec::new())
}

fn update_control_payload_with_requirements(
    code: &str,
    framework_requirement_ids: Vec<FrameworkRequirementId>,
) -> UpdateControlPayload {
    UpdateControlPayload {
        code: code.to_owned(),
        title: format!("Updated repository control {code}"),
        description: format!("Updated repository control description for {code}."),
        framework_requirement_ids,
    }
}

fn requirement_ids(requirements: &[proofplane::domain::FrameworkRequirement]) -> Vec<Uuid> {
    requirements
        .iter()
        .map(|requirement| Uuid::from(requirement.id))
        .collect()
}

fn mapping_payload(
    evidence_request_id: proofplane::domain::EvidenceRequestId,
    control_id: proofplane::domain::ControlId,
    rationale: &str,
) -> CreateEvidenceRequestControlMappingPayload {
    CreateEvidenceRequestControlMappingPayload {
        evidence_request_id,
        control_id,
        rationale: rationale.to_owned(),
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
