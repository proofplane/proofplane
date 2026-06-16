use chrono::{Duration, Utc};
use proofplane::domain::{
    ActorId, ActorKind, AttachmentUploadStatus, CreateActorPayload, CreateApiCredentialPayload,
    CreateControlPayload, CreateEvidenceAttachmentPayload,
    CreateEvidenceRequestControlMappingPayload, CreateEvidenceRequestPayload,
    CreateEvidenceSubmissionPayload, CreateWorkspacePayload, EvidenceRequestCadence,
    EvidenceRequestStatus, EvidenceSubmissionId, FrameworkRequirementId, ProvisionUserPayload,
    UpdateActorPayload, UpdateApiCredentialPayload, UpdateControlPayload, UpdateWorkspacePayload,
    WorkspacePermission,
};
use proofplane::pubsub::{TopicName, MESSAGE_BUS_TOPIC};
use proofplane::repository::NewOutboxMessage;
use serde_json::json;
use uuid::Uuid;

use super::support::{cc61_id, cc71_id, TestApp, INTEGRATION_ACTOR_ID};

#[derive(Clone, Copy)]
struct RepositoryActor {
    workspace_id: proofplane::domain::WorkspaceId,
    actor_id: ActorId,
}

#[tokio::test]
async fn user_repository_upsert_provisions_once_and_preserves_profile() {
    let app = TestApp::start().await;
    let postgres = app.postgres();

    let created = postgres
        .upsert_user_by_auth0_sub(&ProvisionUserPayload {
            auth0_sub: "auth0|repo-user".to_owned(),
            email: Some("repo@example.com".to_owned()),
            name: Some("Repo User".to_owned()),
        })
        .await
        .expect("user provisions");

    assert_eq!(created.auth0_sub, "auth0|repo-user");
    assert_eq!(created.email.as_deref(), Some("repo@example.com"));
    assert_eq!(created.name.as_deref(), Some("Repo User"));

    let reprovisioned = postgres
        .upsert_user_by_auth0_sub(&ProvisionUserPayload {
            auth0_sub: "auth0|repo-user".to_owned(),
            email: None,
            name: None,
        })
        .await
        .expect("user re-provisions");

    assert_eq!(reprovisioned.id, created.id);
    assert_eq!(reprovisioned.email.as_deref(), Some("repo@example.com"));
    assert_eq!(reprovisioned.name.as_deref(), Some("Repo User"));

    assert_eq!(
        postgres
            .get_user(created.id)
            .await
            .expect("user reads")
            .expect("user exists"),
        reprovisioned
    );
}

#[tokio::test]
async fn actor_repository_crud_uses_typed_rows() {
    let app = TestApp::start().await;
    let postgres = app.postgres();
    let workspace = postgres
        .create_workspace(&CreateWorkspacePayload {
            id: None,
            slug: None,
            name: "Repository Actor Workspace".to_owned(),
        })
        .await
        .expect("workspace creates");
    let actor = postgres
        .create_actor(&CreateActorPayload {
            id: Some(ActorId::from(Uuid::new_v4())),
            kind: ActorKind::HumanUser,
            display_name: "Repository Human".to_owned(),
            workspace_id: workspace.id,
            created_by_user_id: None,
            permissions: vec![WorkspacePermission::ReadControls],
        })
        .await
        .expect("actor creates");
    assert_eq!(actor.workspace_id, workspace.id);

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
                workspace_id: workspace.id,
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
                workspace_id: workspace.id,
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
    let workspace = postgres
        .create_workspace(&CreateWorkspacePayload {
            id: None,
            slug: None,
            name: "Repository Credential Workspace".to_owned(),
        })
        .await
        .expect("workspace creates");
    let actor = postgres
        .create_actor(&CreateActorPayload {
            id: None,
            kind: ActorKind::Integration,
            display_name: "Credential Actor".to_owned(),
            workspace_id: workspace.id,
            created_by_user_id: None,
            permissions: WorkspacePermission::ALL.to_vec(),
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
    let (found_actor, found_credential, found_permissions) = postgres
        .actor_credential_by_key_id(actor.id, "rotated-key-id")
        .await
        .expect("actor credential reads")
        .expect("actor exists");
    assert_eq!(found_actor, actor);
    assert_eq!(found_credential, updated.clone());
    assert!(found_permissions.has(WorkspacePermission::ReadControls));
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
async fn api_credential_repository_allows_multiple_credentials_per_actor() {
    let app = TestApp::start().await;
    let postgres = app.postgres();
    let workspace = postgres
        .create_workspace(&CreateWorkspacePayload {
            id: None,
            slug: None,
            name: "Multi Credential Workspace".to_owned(),
        })
        .await
        .expect("workspace creates");
    let actor = postgres
        .create_actor(&CreateActorPayload {
            id: None,
            kind: ActorKind::Integration,
            display_name: "Multi Credential Actor".to_owned(),
            workspace_id: workspace.id,
            created_by_user_id: None,
            permissions: WorkspacePermission::ALL.to_vec(),
        })
        .await
        .expect("credential actor creates");

    for (id, key_id) in [
        ("first-api-key", "first-key-id"),
        ("second-api-key", "second-key-id"),
    ] {
        postgres
            .create_api_credential(&CreateApiCredentialPayload {
                id: id.to_owned(),
                actor_id: actor.id,
                name: id.to_owned(),
                key_id: key_id.to_owned(),
                credential_hash: format!("{id}-hash"),
                expires_at: None,
                revoked_at: None,
            })
            .await
            .expect("API credential creates");
    }

    // Both live credentials resolve by their own key_id, scoped to the actor.
    for key_id in ["first-key-id", "second-key-id"] {
        assert!(postgres
            .actor_credential_by_key_id(actor.id, key_id)
            .await
            .expect("credential resolves")
            .is_some());
    }
}

#[tokio::test]
async fn control_repository_create_returns_created_control_and_replace_masks_missing_rows() {
    let app = TestApp::builder().with_soc2_reference_data().build().await;
    let postgres = app.postgres();
    let actor = repository_actor_context(&app).await;
    let other_actor = repository_actor_context(&app).await;

    let created = postgres
        .in_actor_context(actor.workspace_id, actor.actor_id, async move |context| {
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
        .in_actor_context(actor.workspace_id, actor.actor_id, async move |context| {
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
        .in_actor_context(actor.workspace_id, actor.actor_id, async move |context| {
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
        .in_actor_context(
            other_actor.workspace_id,
            other_actor.actor_id,
            async move |context| {
                context
                    .replace_control(created.id, &update_control_payload("PP-CROSS"))
                    .await
            },
        )
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
    let request = create_repository_evidence_request(postgres, actor).await;
    let control = postgres
        .in_actor_context(actor.workspace_id, actor.actor_id, async move |context| {
            context.create_control(&control_payload("PP-MAP-01")).await
        })
        .await
        .expect("control creates");
    let other_control = postgres
        .in_actor_context(
            other_actor.workspace_id,
            other_actor.actor_id,
            async move |context| context.create_control(&control_payload("PP-MAP-02")).await,
        )
        .await
        .expect("other control creates");

    let created = postgres
        .in_actor_context(actor.workspace_id, actor.actor_id, async move |context| {
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
        .in_actor_context(actor.workspace_id, actor.actor_id, async move |context| {
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
        .in_actor_context(actor.workspace_id, actor.actor_id, async move |context| {
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
    let request = create_repository_evidence_request(postgres, actor).await;

    let submission = postgres
        .in_actor_context(actor.workspace_id, actor.actor_id, async move |context| {
            context
                .create_evidence_submission(&submission_payload(request.id))
                .await
        })
        .await
        .expect("submission create resolves")
        .expect("submission creates");

    assert_eq!(submission.evidence_request_id, request.id);
    assert_eq!(submission.submitted_by, actor.actor_id);
    assert_eq!(submission.source_system, "github");

    let missing = postgres
        .in_actor_context(actor.workspace_id, actor.actor_id, async move |context| {
            context
                .create_evidence_submission(&submission_payload(Uuid::new_v4().into()))
                .await
        })
        .await
        .expect("missing request create resolves");
    assert!(missing.is_none());

    let cross_workspace = postgres
        .in_actor_context(
            other_actor.workspace_id,
            other_actor.actor_id,
            async move |context| {
                context
                    .create_evidence_submission(&submission_payload(request.id))
                    .await
            },
        )
        .await
        .expect("cross-workspace request create resolves");
    assert!(cross_workspace.is_none());
}

#[tokio::test]
async fn evidence_submission_detail_starts_with_empty_attachment_list() {
    let app = TestApp::start().await;
    let postgres = app.postgres();
    let actor = repository_actor_context(&app).await;
    let request = create_repository_evidence_request(postgres, actor).await;
    let submission = create_repository_submission(postgres, actor, request.id).await;

    let detail = postgres
        .in_actor_context_read(actor.workspace_id, actor.actor_id, async move |context| {
            context.get_evidence_submission(submission.id).await
        })
        .await
        .expect("submission detail resolves")
        .expect("submission detail exists");

    assert_eq!(detail.submission, submission);
    assert!(detail.attachments.is_empty());
}

#[tokio::test]
async fn evidence_attachment_create_scopes_to_workspace_and_creates_pending_upload() {
    let app = TestApp::start().await;
    let postgres = app.postgres();
    let actor = repository_actor_context(&app).await;
    let other_actor = repository_actor_context(&app).await;
    let request = create_repository_evidence_request(postgres, actor).await;
    let submission = create_repository_submission(postgres, actor, request.id).await;

    let attachment = postgres
        .in_actor_context(actor.workspace_id, actor.actor_id, async move |context| {
            context
                .create_evidence_attachment(&attachment_payload(submission.id, "first"))
                .await
        })
        .await
        .expect("attachment creates");

    assert_eq!(attachment.evidence_submission_id, submission.id);
    assert_eq!(
        attachment.upload_status,
        AttachmentUploadStatus::PendingUpload
    );

    let missing = postgres
        .in_actor_context(actor.workspace_id, actor.actor_id, async move |context| {
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
        .in_actor_context(
            other_actor.workspace_id,
            other_actor.actor_id,
            async move |context| {
                context
                    .create_evidence_attachment(&attachment_payload(
                        submission.id,
                        "cross-workspace",
                    ))
                    .await
            },
        )
        .await;
    assert!(matches!(
        cross_workspace,
        Err(proofplane::repository::Error::InvariantViolation(
            "attachment insert requires an existing workspace-scoped submission"
        ))
    ));
}

#[tokio::test]
async fn evidence_submission_detail_includes_attachments_with_upload_status() {
    let app = TestApp::start().await;
    let postgres = app.postgres();
    let actor = repository_actor_context(&app).await;
    let request = create_repository_evidence_request(postgres, actor).await;
    let submission = create_repository_submission(postgres, actor, request.id).await;
    let attachment = create_repository_attachment(postgres, actor, submission.id).await;

    let detail = postgres
        .in_actor_context_read(actor.workspace_id, actor.actor_id, async move |context| {
            context.get_evidence_submission(submission.id).await
        })
        .await
        .expect("submission detail resolves")
        .expect("submission detail exists");

    assert_eq!(detail.submission, submission);
    assert_eq!(detail.attachments, vec![attachment]);
}

#[tokio::test]
async fn attachment_scan_work_loads_pending_rows_by_attachment_and_quarantine_key() {
    let app = TestApp::start().await;
    let postgres = app.postgres();
    let actor = repository_actor_context(&app).await;
    let request = create_repository_evidence_request(postgres, actor).await;
    let submission = create_repository_submission(postgres, actor, request.id).await;
    let attachment = create_repository_attachment(postgres, actor, submission.id).await;

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
    let actor = repository_actor_context(&app).await;
    let request = create_repository_evidence_request(postgres, actor).await;
    let submission = create_repository_submission(postgres, actor, request.id).await;
    let attachment = create_repository_attachment(postgres, actor, submission.id).await;
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
        .in_actor_context_read(actor.workspace_id, actor.actor_id, async move |context| {
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
    let actor = repository_actor_context(&app).await;
    let request = create_repository_evidence_request(postgres, actor).await;
    let submission = create_repository_submission(postgres, actor, request.id).await;
    let malicious = create_repository_attachment(postgres, actor, submission.id).await;
    let failed = postgres
        .in_actor_context(actor.workspace_id, actor.actor_id, async move |context| {
            context
                .create_evidence_attachment(&attachment_payload(submission.id, "failed-scan"))
                .await
        })
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
        .in_actor_context_read(actor.workspace_id, actor.actor_id, async move |context| {
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
async fn latest_evidence_submission_for_request_returns_newest_visible_submission() {
    let app = TestApp::start().await;
    let postgres = app.postgres();
    let actor = repository_actor_context(&app).await;
    let other_actor = repository_actor_context(&app).await;
    let request = create_repository_evidence_request(postgres, actor).await;
    let first = create_repository_submission(postgres, actor, request.id).await;
    let latest = create_repository_submission(postgres, actor, request.id).await;

    set_submission_received_at(postgres, first.id, Utc::now() - Duration::days(1)).await;
    set_submission_received_at(postgres, latest.id, Utc::now()).await;

    let detail = postgres
        .in_actor_context_read(actor.workspace_id, actor.actor_id, async move |context| {
            context
                .latest_evidence_submission_for_request(request.id)
                .await
        })
        .await
        .expect("latest submission resolves")
        .expect("latest submission exists");
    assert_eq!(detail.submission.id, latest.id);

    let missing = postgres
        .in_actor_context_read(actor.workspace_id, actor.actor_id, async move |context| {
            context
                .latest_evidence_submission_for_request(Uuid::new_v4().into())
                .await
        })
        .await
        .expect("missing latest resolves");
    assert!(missing.is_none());

    let cross_workspace = postgres
        .in_actor_context_read(
            other_actor.workspace_id,
            other_actor.actor_id,
            async move |context| {
                context
                    .latest_evidence_submission_for_request(request.id)
                    .await
            },
        )
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
        .in_actor_context(actor.workspace_id, actor.actor_id, async move |context| {
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
        .in_actor_context(actor.workspace_id, actor.actor_id, async move |context| {
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
        })
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
    let actor = repository_actor_context(&app).await;

    let first = append_outbox(postgres, actor, "first").await;
    let second = append_outbox(postgres, actor, "second").await;
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

async fn repository_actor_context(app: &TestApp) -> RepositoryActor {
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

    RepositoryActor {
        workspace_id: workspace.id,
        actor_id,
    }
}

async fn create_repository_evidence_request(
    postgres: &proofplane::repository::Postgres,
    actor: RepositoryActor,
) -> proofplane::domain::EvidenceRequest {
    postgres
        .in_actor_context(actor.workspace_id, actor.actor_id, async move |context| {
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
    actor: RepositoryActor,
    evidence_request_id: proofplane::domain::EvidenceRequestId,
) -> proofplane::domain::EvidenceSubmission {
    postgres
        .in_actor_context(actor.workspace_id, actor.actor_id, async move |context| {
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
    actor: RepositoryActor,
    submission_id: EvidenceSubmissionId,
) -> proofplane::domain::EvidenceAttachment {
    postgres
        .in_actor_context(actor.workspace_id, actor.actor_id, async move |context| {
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
    actor: RepositoryActor,
    aggregate_id: &str,
) -> proofplane::repository::OutboxMessage {
    let aggregate_id = aggregate_id.to_owned();
    postgres
        .in_actor_context(actor.workspace_id, actor.actor_id, async move |context| {
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
