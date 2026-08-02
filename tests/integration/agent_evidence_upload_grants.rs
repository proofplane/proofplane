use chrono::{Duration, TimeZone, Utc};
use proofplane::{
    domain::{
        AgentEvidenceUploadDeclaration, AgentEvidenceUploadGrant, CoverageWindow, EvidenceId,
    },
    services::agent_evidence_upload_grants::AgentEvidenceUploadGrantError,
};
use secrecy::ExposeSecret;
use serde_json::json;
use tokio::sync::oneshot;
use uuid::Uuid;

use super::support::TestApp;

#[tokio::test]
async fn machine_grant_persists_declared_metadata_and_provenance() {
    let app = TestApp::builder()
        .workspace("workspace", "Machine upload workspace")
        .with_default_membership()
        .build()
        .await;
    let workspace_id = app.workspace_id("workspace");
    let evidence_id = create_evidence(&app, workspace_id, "Machine evidence", "paused").await;
    let coverage = coverage();
    let declaration = declaration();

    let issued = app
        .agent_evidence_upload_grant_service()
        .issue(
            &app.agent_connection_context(workspace_id),
            evidence_id.into(),
            coverage,
            declaration.clone(),
        )
        .await
        .expect("machine grant issues");
    let authority = app
        .agent_evidence_upload_grant_service()
        .credential_verifier()
        .verify(issued.credential.expose_secret())
        .expect("machine credential verifies");
    let verified = app
        .postgres()
        .agent_evidence_upload_grants()
        .get(issued.grant.id(), issued.grant.workspace_id())
        .await
        .expect("grant loads")
        .expect("grant exists");
    verified
        .matches_authority(&authority)
        .expect("authority matches");

    assert_eq!(verified, issued.grant);
    assert_eq!(verified.workspace_id(), workspace_id.into());
    assert_eq!(verified.evidence_id(), evidence_id.into());
    assert_eq!(verified.coverage(), coverage);
    assert_eq!(verified.declaration(), &declaration);
    assert_eq!(verified.issued_by_user_id(), app.user_id().into());
    assert_eq!(
        verified.issued_via_agent_connection_id(),
        app.api_token_id().into()
    );
    assert_ne!(
        verified.submission_id().to_string(),
        issued.grant.id().to_string()
    );
    assert!(verified.expires_at() > verified.issued_at());
    assert!(verified.completed_at().is_none());
    assert!(verified.document_id().is_none());
}

#[tokio::test]
async fn machine_grant_conceals_missing_and_cross_workspace_evidence_without_persisting() {
    let app = TestApp::builder()
        .workspace("workspace", "Machine upload workspace")
        .with_default_membership()
        .workspace("other", "Other workspace")
        .with_default_membership()
        .build()
        .await;
    let workspace_id = app.workspace_id("workspace");
    let other_workspace_id = app.workspace_id("other");
    let other_evidence_id =
        create_evidence(&app, other_workspace_id, "Other evidence", "retired").await;
    let service = app.agent_evidence_upload_grant_service();

    for evidence_id in [Uuid::new_v4(), other_evidence_id] {
        assert!(matches!(
            service
                .issue(
                    &app.agent_connection_context(workspace_id),
                    EvidenceId::from(evidence_id),
                    coverage(),
                    declaration(),
                )
                .await,
            Err(AgentEvidenceUploadGrantError::Unavailable)
        ));
    }

    let count: i64 = app
        .postgres()
        .get()
        .await
        .expect("database opens")
        .query_one("SELECT count(*) FROM agent_evidence_upload_grants", &[])
        .await
        .expect("grant count loads")
        .get(0);
    assert_eq!(count, 0);
}

#[tokio::test]
async fn machine_grant_accepts_every_existing_evidence_status() {
    let app = TestApp::builder()
        .workspace("workspace", "Machine upload workspace")
        .with_default_membership()
        .build()
        .await;
    let workspace_id = app.workspace_id("workspace");
    let service = app.agent_evidence_upload_grant_service();

    for status in ["active", "paused", "retired"] {
        let evidence_id =
            create_evidence(&app, workspace_id, &format!("{status} evidence"), status).await;
        let issued = service
            .issue(
                &app.agent_connection_context(workspace_id),
                evidence_id.into(),
                coverage(),
                declaration(),
            )
            .await
            .expect("existing evidence is eligible");
        assert_eq!(issued.grant.evidence_id(), evidence_id.into());
    }
}

#[tokio::test]
async fn grant_repository_scopes_reads_and_persists_the_full_aggregate_snapshot() {
    let app = TestApp::builder()
        .workspace("workspace", "Machine upload workspace")
        .with_default_membership()
        .workspace("other", "Other workspace")
        .with_default_membership()
        .build()
        .await;
    let workspace_id = app.workspace_id("workspace");
    let evidence_id = create_evidence(&app, workspace_id, "Machine evidence", "active").await;
    let issued = app
        .agent_evidence_upload_grant_service()
        .issue(
            &app.agent_connection_context(workspace_id),
            evidence_id.into(),
            coverage(),
            declaration(),
        )
        .await
        .expect("machine grant issues");

    assert!(app
        .postgres()
        .agent_evidence_upload_grants()
        .get(issued.grant.id(), app.workspace_id("other").into())
        .await
        .expect("tenant-scoped lookup succeeds")
        .is_none());

    app.postgres()
        .get()
        .await
        .expect("database opens")
        .execute(
            "UPDATE agent_evidence_upload_grants SET filename = 'tampered.pdf' WHERE id = $1",
            &[&Uuid::from(issued.grant.id())],
        )
        .await
        .expect("immutable field is tampered for the protection test");
    let grant_id = issued.grant.id();
    let grant_workspace_id = issued.grant.workspace_id();
    let issued_by_user_id = issued.grant.issued_by_user_id();
    let issued_via_agent_connection_id = issued.grant.issued_via_agent_connection_id();
    let grant = issued.grant;
    let round_tripped = app
        .postgres()
        .in_agent_connection_workspace_context(
            grant_workspace_id,
            issued_by_user_id,
            issued_via_agent_connection_id,
            async move |context| {
                let repository = context.agent_evidence_upload_grants();
                repository.save(&grant).await?;
                let reloaded = repository.get(grant_id, grant_workspace_id).await?;
                Ok(reloaded == Some(grant))
            },
        )
        .await
        .expect("full-snapshot save completes");
    assert!(round_tripped);
}

#[tokio::test]
async fn grant_repository_does_not_overwrite_a_same_id_grant_in_another_workspace() {
    let app = TestApp::builder()
        .workspace("workspace", "Machine upload workspace")
        .with_default_membership()
        .workspace("other", "Other workspace")
        .with_default_membership()
        .build()
        .await;
    let workspace_id = app.workspace_id("workspace");
    let other_workspace_id = app.workspace_id("other");
    let evidence_id = create_evidence(&app, workspace_id, "Machine evidence", "active").await;
    let other_evidence_id =
        create_evidence(&app, other_workspace_id, "Other evidence", "active").await;
    let issued = app
        .agent_evidence_upload_grant_service()
        .issue(
            &app.agent_connection_context(workspace_id),
            evidence_id.into(),
            coverage(),
            declaration(),
        )
        .await
        .expect("machine grant issues");
    let issued_at = Utc::now();
    let colliding = AgentEvidenceUploadGrant::issue(
        issued.grant.id(),
        Uuid::new_v4().into(),
        other_workspace_id.into(),
        other_evidence_id.into(),
        coverage(),
        declaration(),
        app.user_id().into(),
        app.api_token_id().into(),
        issued_at,
        issued_at + Duration::minutes(5),
    )
    .expect("colliding aggregate is valid in isolation");

    let save = app
        .postgres()
        .in_agent_connection_workspace_context(
            colliding.workspace_id(),
            colliding.issued_by_user_id(),
            colliding.issued_via_agent_connection_id(),
            async move |context| {
                context
                    .agent_evidence_upload_grants()
                    .save(&colliding)
                    .await
            },
        )
        .await;
    assert!(save.is_err());

    let original = app
        .postgres()
        .agent_evidence_upload_grants()
        .get(issued.grant.id(), workspace_id.into())
        .await
        .expect("original grant lookup succeeds")
        .expect("original grant remains available");
    assert_eq!(original.evidence_id(), evidence_id.into());
}

#[tokio::test]
async fn transactional_grant_reads_hold_a_row_lock_while_verification_reads_do_not() {
    let app = TestApp::builder()
        .workspace("workspace", "Machine upload workspace")
        .with_default_membership()
        .build()
        .await;
    let workspace_id = app.workspace_id("workspace");
    let evidence_id = create_evidence(&app, workspace_id, "Machine evidence", "active").await;
    let issued = app
        .agent_evidence_upload_grant_service()
        .issue(
            &app.agent_connection_context(workspace_id),
            evidence_id.into(),
            coverage(),
            declaration(),
        )
        .await
        .expect("machine grant issues");
    let grant_id = issued.grant.id();
    let user_id = issued.grant.issued_by_user_id();
    let connection_id = issued.grant.issued_via_agent_connection_id();

    let postgres = app.postgres_arc();
    let (locked_tx, locked_rx) = oneshot::channel();
    let (release_tx, release_rx) = oneshot::channel();
    let lock_holder = tokio::spawn(async move {
        postgres
            .in_agent_connection_workspace_context(
                workspace_id.into(),
                user_id,
                connection_id,
                async move |context| {
                    let loaded = context
                        .agent_evidence_upload_grants()
                        .get(grant_id, workspace_id.into())
                        .await?;
                    assert!(loaded.is_some());
                    locked_tx.send(()).expect("lock acquisition is observed");
                    release_rx.await.expect("lock release is requested");
                    Ok(())
                },
            )
            .await
    });
    locked_rx.await.expect("transaction acquires the row lock");

    let contender = app.postgres().get().await.expect("database opens");
    contender
        .batch_execute("SET lock_timeout = '100ms'")
        .await
        .expect("lock timeout configures");
    contender
        .execute(
            "UPDATE agent_evidence_upload_grants SET filename = filename WHERE id = $1",
            &[&Uuid::from(grant_id)],
        )
        .await
        .expect_err("transaction-backed get holds the row lock");

    release_tx.send(()).expect("transaction is released");
    lock_holder
        .await
        .expect("lock task joins")
        .expect("lock transaction commits");

    app.postgres()
        .agent_evidence_upload_grants()
        .get(grant_id, workspace_id.into())
        .await
        .expect("verification read succeeds")
        .expect("grant exists");
    contender
        .execute(
            "UPDATE agent_evidence_upload_grants SET filename = filename WHERE id = $1",
            &[&Uuid::from(grant_id)],
        )
        .await
        .expect("verification read leaves no row lock behind");
}

#[tokio::test]
async fn machine_grant_schema_rejects_completion_at_the_expiry_boundary() {
    let app = TestApp::builder()
        .workspace("workspace", "Machine upload workspace")
        .with_default_membership()
        .build()
        .await;
    let workspace_id = app.workspace_id("workspace");
    let evidence_id = create_evidence(&app, workspace_id, "Machine evidence", "active").await;
    let issued = app
        .agent_evidence_upload_grant_service()
        .issue(
            &app.agent_connection_context(workspace_id),
            evidence_id.into(),
            coverage(),
            declaration(),
        )
        .await
        .expect("machine grant issues");
    let client = app.postgres().get().await.expect("database opens");
    let document_id: Uuid = client
        .query_one(
            r#"
INSERT INTO documents (
    workspace_id, owner_type, owner_id, filename, content_type, content_length,
    object_key, checksum_sha256, checksum_crc32c, created_by_user_id
)
VALUES ($1, 'evidence_submission', $2, 'boundary.pdf', 'application/pdf', 1,
        $3, 'sha256', 'crc32c', $4)
RETURNING id
"#,
            &[
                &workspace_id,
                &Uuid::from(issued.grant.submission_id()),
                &format!("quarantine/{}/boundary", issued.grant.id()),
                &app.user_id(),
            ],
        )
        .await
        .expect("document fixture inserts")
        .get("id");

    client
        .execute(
            "UPDATE agent_evidence_upload_grants SET completed_at = expires_at, document_id = $2 WHERE id = $1",
            &[&Uuid::from(issued.grant.id()), &document_id],
        )
        .await
        .expect_err("completion at expiry violates the snapshot constraint");
}

#[tokio::test]
async fn machine_grant_verification_rejects_tampering_mismatch_expiry_and_wrong_purpose() {
    let app = TestApp::builder()
        .workspace("workspace", "Machine upload workspace")
        .with_default_membership()
        .build()
        .await;
    let workspace_id = app.workspace_id("workspace");
    let evidence_id = create_evidence(&app, workspace_id, "Machine evidence", "active").await;
    let connection = app.agent_connection_context(workspace_id);
    let service = app.agent_evidence_upload_grant_service();
    let issued = service
        .issue(&connection, evidence_id.into(), coverage(), declaration())
        .await
        .expect("machine grant issues");
    let token = issued.credential.expose_secret();
    let issued_debug = format!("{issued:?}");
    assert!(!issued_debug.contains(token));
    assert!(!issued_debug.contains("access-review.pdf"));
    assert!(!issued_debug.contains("application/pdf"));

    assert!(matches!(
        service.credential_verifier().verify(&tamper(token)),
        Err(AgentEvidenceUploadGrantError::Unavailable)
    ));
    assert!(matches!(
        service.credential_verifier().verify(&tamper(token)),
        Err(AgentEvidenceUploadGrantError::Unavailable)
    ));

    let mismatched_submission_id = Uuid::new_v4();
    app.postgres()
        .get()
        .await
        .expect("database opens")
        .execute(
            "UPDATE agent_evidence_upload_grants SET submission_id = $2 WHERE id = $1",
            &[&Uuid::from(issued.grant.id()), &mismatched_submission_id],
        )
        .await
        .expect("persisted grant is changed");
    let mismatched = app
        .postgres()
        .agent_evidence_upload_grants()
        .get(issued.grant.id(), issued.grant.workspace_id())
        .await
        .expect("grant loads")
        .expect("grant exists");
    assert!(mismatched
        .matches_authority(&service.credential_verifier().verify(token).unwrap())
        .is_err());
    app.postgres()
        .get()
        .await
        .expect("database opens")
        .execute(
            "UPDATE agent_evidence_upload_grants SET submission_id = $2 WHERE id = $1",
            &[
                &Uuid::from(issued.grant.id()),
                &Uuid::from(issued.grant.submission_id()),
            ],
        )
        .await
        .expect("persisted grant is restored");

    let oversized_content_type = format!("application/{}", "a".repeat(244));
    app.postgres()
        .get()
        .await
        .expect("database opens")
        .execute(
            "UPDATE agent_evidence_upload_grants SET content_type = $2 WHERE id = $1",
            &[&Uuid::from(issued.grant.id()), &oversized_content_type],
        )
        .await
        .expect_err("database constraint rejects an oversized content type");

    app.postgres()
        .get()
        .await
        .expect("database opens")
        .execute(
            "UPDATE agent_evidence_upload_grants SET issued_at = now() - interval '10 minutes', expires_at = now() - interval '5 minutes' WHERE id = $1",
            &[&Uuid::from(issued.grant.id())],
        )
        .await
        .expect("grant expires");
    let expired = app
        .postgres()
        .agent_evidence_upload_grants()
        .get(issued.grant.id(), issued.grant.workspace_id())
        .await
        .expect("grant loads")
        .expect("grant exists");
    assert!(expired.ensure_pending_at(Utc::now()).is_err());

    let human = app
        .document_upload_grant_service()
        .issue(&connection, evidence_id.into(), coverage())
        .await
        .expect("human grant issues");
    let human_token = human
        .url
        .query_pairs()
        .find(|(name, _)| name == "token")
        .map(|(_, value)| value.into_owned())
        .expect("human token is present");
    assert!(matches!(
        service.credential_verifier().verify(&human_token),
        Err(AgentEvidenceUploadGrantError::Unavailable)
    ));
}

fn coverage() -> CoverageWindow {
    CoverageWindow::new(
        Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
        Utc.with_ymd_and_hms(2026, 3, 31, 23, 59, 59).unwrap(),
    )
    .unwrap()
}

fn declaration() -> AgentEvidenceUploadDeclaration {
    AgentEvidenceUploadDeclaration::new(
        "access-review.pdf".to_owned(),
        "application/pdf".to_owned(),
        483_920,
        Some("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad".to_owned()),
        25 * 1024 * 1024,
    )
    .into_result()
    .unwrap()
}

async fn create_evidence(app: &TestApp, workspace_id: Uuid, title: &str, status: &str) -> Uuid {
    app.create_evidence(
        workspace_id,
        &json!({
            "title": title,
            "description": format!("Collect {title}."),
            "collection_instructions": format!("Upload {title}."),
            "status": status,
        }),
    )
    .await["id"]
        .as_str()
        .and_then(|id| Uuid::parse_str(id).ok())
        .expect("evidence id")
}

fn tamper(token: &str) -> String {
    let mut bytes = token.as_bytes().to_vec();
    let index = bytes.len() / 2;
    bytes[index] = if bytes[index] == b'A' { b'B' } else { b'A' };
    String::from_utf8(bytes).expect("token remains UTF-8")
}
