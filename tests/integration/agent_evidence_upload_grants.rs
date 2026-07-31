use chrono::{TimeZone, Utc};
use proofplane::{
    domain::{AgentEvidenceUploadDeclaration, CoverageWindow, EvidenceId},
    services::agent_evidence_upload_grants::AgentEvidenceUploadGrantError,
};
use secrecy::ExposeSecret;
use serde_json::json;
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
    let verified = app
        .agent_evidence_upload_grant_service()
        .verify(issued.grant.id, issued.credential.expose_secret())
        .await
        .expect("machine grant verifies");

    assert_eq!(verified, issued.grant);
    assert_eq!(verified.workspace_id, workspace_id.into());
    assert_eq!(verified.evidence_id, evidence_id.into());
    assert_eq!(verified.coverage, coverage);
    assert_eq!(verified.declaration, declaration);
    assert_eq!(verified.issued_by_user_id, app.user_id().into());
    assert_eq!(
        verified.issued_via_agent_connection_id,
        app.api_token_id().into()
    );
    assert_ne!(
        verified.submission_id.to_string(),
        issued.grant.id.to_string()
    );
    assert!(verified.expires_at > verified.issued_at);
    assert!(verified.completed_at.is_none());
    assert!(verified.document_id.is_none());
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
        assert_eq!(issued.grant.evidence_id, evidence_id.into());
    }
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
        service.verify(Uuid::new_v4().into(), token).await,
        Err(AgentEvidenceUploadGrantError::Unavailable)
    ));
    assert!(matches!(
        service.verify(issued.grant.id, &tamper(token)).await,
        Err(AgentEvidenceUploadGrantError::Unavailable)
    ));

    let mismatched_submission_id = Uuid::new_v4();
    app.postgres()
        .get()
        .await
        .expect("database opens")
        .execute(
            "UPDATE agent_evidence_upload_grants SET submission_id = $2 WHERE id = $1",
            &[&Uuid::from(issued.grant.id), &mismatched_submission_id],
        )
        .await
        .expect("persisted grant is changed");
    assert!(matches!(
        service.verify(issued.grant.id, token).await,
        Err(AgentEvidenceUploadGrantError::Unavailable)
    ));
    app.postgres()
        .get()
        .await
        .expect("database opens")
        .execute(
            "UPDATE agent_evidence_upload_grants SET submission_id = $2 WHERE id = $1",
            &[
                &Uuid::from(issued.grant.id),
                &Uuid::from(issued.grant.submission_id),
            ],
        )
        .await
        .expect("persisted grant is restored");

    app.postgres()
        .get()
        .await
        .expect("database opens")
        .execute(
            "UPDATE agent_evidence_upload_grants SET issued_at = now() - interval '10 minutes', expires_at = now() - interval '5 minutes' WHERE id = $1",
            &[&Uuid::from(issued.grant.id)],
        )
        .await
        .expect("grant expires");
    assert!(matches!(
        service.verify(issued.grant.id, token).await,
        Err(AgentEvidenceUploadGrantError::Unavailable)
    ));

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
        service.verify(issued.grant.id, &human_token).await,
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
