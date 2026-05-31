use chrono::{Duration, Utc};
use proofplane::domain::{
    ActorId, ActorKind, AttachmentScanStatus, CreateActorPayload, CreateApiCredentialPayload,
    CreateEvidenceAttachmentPayload, CreateEvidenceRequestPayload, CreateEvidenceSubmissionPayload,
    CreateWorkspacePayload, EvidenceRequestCadence, EvidenceRequestStatus, EvidenceSubmissionId,
    UpdateActorPayload, UpdateApiCredentialPayload, UpdateWorkspacePayload,
};
use proofplane::routes::authentication::ActorContext;
use serde_json::json;
use uuid::Uuid;

use super::support::{TestApp, INTEGRATION_ACTOR_ID};

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
        .expect("attachment create resolves")
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
        .await
        .expect("missing submission attachment create resolves");
    assert!(missing.is_none());

    let cross_workspace = postgres
        .in_actor_context(other_actor, async move |context| {
            context
                .create_evidence_attachment(&attachment_payload(submission.id, "cross-workspace"))
                .await
        })
        .await
        .expect("cross-workspace submission attachment create resolves");
    assert!(cross_workspace.is_none());
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
        .expect("attachment create resolves")
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
