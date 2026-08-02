use chrono::{Duration, Utc};
use proofplane::{
    domain::{
        AgentPolicyDocumentUploadDeclaration, AgentPolicyDocumentUploadGrant, CreatePolicyPayload,
        PolicyId, WorkspacePermission,
    },
    services::{
        agent_policy_document_upload_grants::AgentPolicyDocumentUploadGrantError,
        policies::PolicyService,
    },
};
use secrecy::ExposeSecret;
use tokio::sync::oneshot;
use uuid::Uuid;

use super::support::TestApp;

#[tokio::test]
async fn policy_machine_grant_repository_scopes_reads_and_persists_the_full_snapshot() {
    let app = TestApp::builder()
        .workspace("workspace", "Policy machine grant workspace")
        .with_default_membership()
        .build()
        .await;
    let workspace_id = app.workspace_id("workspace");
    let connection = app.agent_connection_context(workspace_id);
    let policy = PolicyService::new(app.postgres_arc())
        .create(
            connection,
            CreatePolicyPayload {
                name: "Machine policy".to_owned(),
                description: None,
                control_ids: vec![],
            },
        )
        .await
        .expect("policy creates")
        .policy;
    let declaration = AgentPolicyDocumentUploadDeclaration::new(
        "policy.pdf".to_owned(),
        "application/pdf".to_owned(),
        483_920,
        None,
        25 * 1024 * 1024,
    )
    .into_result()
    .expect("declaration is valid");

    let issued = app
        .agent_policy_document_upload_grant_service()
        .issue(&connection, policy.id, declaration.clone())
        .await
        .expect("policy machine grant issues");
    let persisted = app
        .postgres()
        .agent_policy_document_upload_grants()
        .get(issued.grant.id(), workspace_id.into())
        .await
        .expect("grant reads")
        .expect("grant exists");
    assert!(app
        .postgres()
        .agent_policy_document_upload_grants()
        .get(issued.grant.id(), Uuid::new_v4().into())
        .await
        .expect("tenant-scoped lookup resolves")
        .is_none());

    assert_eq!(persisted, issued.grant);
    assert_eq!(persisted.policy_id(), policy.id);
    assert_eq!(persisted.declaration(), &declaration);
    assert_eq!(persisted.issued_by_user_id(), app.user_id().into());
    assert_eq!(
        persisted.issued_via_agent_connection_id(),
        app.api_token_id().into()
    );

    app.postgres()
        .get()
        .await
        .expect("database opens")
        .execute(
            "UPDATE agent_policy_document_upload_grants SET filename = 'tampered.pdf' WHERE id = $1",
            &[&Uuid::from(issued.grant.id())],
        )
        .await
        .expect("snapshot field is tampered for the protection test");
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
                let repository = context.agent_policy_document_upload_grants();
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
async fn policy_machine_grant_conceals_unavailable_policies_without_persisting() {
    let app = TestApp::builder()
        .workspace("workspace", "Policy machine grant workspace")
        .with_default_membership()
        .workspace("other", "Other policy workspace")
        .with_default_membership()
        .build()
        .await;
    let workspace_id = app.workspace_id("workspace");
    let other_workspace_id = app.workspace_id("other");
    let connection = app.agent_connection_context(workspace_id);
    let archived = create_policy(&app, workspace_id, "Archived policy").await;
    PolicyService::new(app.postgres_arc())
        .archive(connection, archived)
        .await
        .expect("policy archives");
    let cross_workspace = create_policy(&app, other_workspace_id, "Other policy").await;
    let service = app.agent_policy_document_upload_grant_service();

    for policy_id in [PolicyId::from(Uuid::new_v4()), archived, cross_workspace] {
        assert!(matches!(
            service
                .issue(
                    &app.agent_connection_context(workspace_id),
                    policy_id,
                    declaration(),
                )
                .await,
            Err(AgentPolicyDocumentUploadGrantError::Unavailable)
        ));
    }

    let count: i64 = app
        .postgres()
        .get()
        .await
        .expect("database opens")
        .query_one(
            "SELECT count(*) FROM agent_policy_document_upload_grants",
            &[],
        )
        .await
        .expect("grant count loads")
        .get(0);
    assert_eq!(count, 0);
}

#[tokio::test]
async fn policy_machine_grant_rejects_a_current_document_without_replacing_it() {
    let app = TestApp::builder()
        .workspace("workspace", "Policy machine grant workspace")
        .with_default_membership()
        .build()
        .await;
    let workspace_id = app.workspace_id("workspace");
    let policy_id = create_policy(&app, workspace_id, "Documented policy").await;
    let client = app.postgres().get().await.expect("database opens");
    let document_id: Uuid = client
        .query_one(
            r#"
INSERT INTO documents (
    workspace_id, owner_type, owner_id, filename, content_type, content_length,
    object_key, checksum_sha256, checksum_crc32c, created_by_user_id, upload_status
)
VALUES ($1, 'policy', $2, 'existing.pdf', 'application/pdf', 8,
        'quarantine/existing', 'checksum', 'crc32c', $3, 'uploaded')
RETURNING id
"#,
            &[&workspace_id, &Uuid::from(policy_id), &app.user_id()],
        )
        .await
        .expect("current document inserts")
        .get("id");

    assert!(matches!(
        app.agent_policy_document_upload_grant_service()
            .issue(
                &app.agent_connection_context(workspace_id),
                policy_id,
                declaration(),
            )
            .await,
        Err(AgentPolicyDocumentUploadGrantError::CurrentDocument)
    ));
    let row = client
        .query_one(
            r#"
SELECT
    (SELECT count(*) FROM agent_policy_document_upload_grants) AS grant_count,
    (SELECT count(*) FROM documents WHERE id = $1 AND archived = false) AS document_count
"#,
            &[&document_id],
        )
        .await
        .expect("outcome reads");
    assert_eq!(row.get::<_, i64>("grant_count"), 0);
    assert_eq!(row.get::<_, i64>("document_count"), 1);
}

#[tokio::test]
async fn policy_machine_credential_rejects_tampering_row_mismatch_and_expiry() {
    let app = TestApp::builder()
        .workspace("workspace", "Policy machine grant workspace")
        .with_default_membership()
        .build()
        .await;
    let workspace_id = app.workspace_id("workspace");
    let policy_id = create_policy(&app, workspace_id, "Credential policy").await;
    let other_policy_id = create_policy(&app, workspace_id, "Other credential policy").await;
    let service = app.agent_policy_document_upload_grant_service();
    let issued = service
        .issue(
            &app.agent_connection_context(workspace_id),
            policy_id,
            declaration(),
        )
        .await
        .expect("grant issues");
    let token = issued.credential.expose_secret();
    let authority = service
        .credential_verifier()
        .verify(token)
        .expect("credential verifies");
    assert_eq!(issued.grant.matches_authority(&authority), Ok(()));
    assert!(matches!(
        service.credential_verifier().verify(&tamper(token)),
        Err(AgentPolicyDocumentUploadGrantError::Unavailable)
    ));

    let client = app.postgres().get().await.expect("database opens");
    client
        .execute(
            "UPDATE agent_policy_document_upload_grants SET policy_id = $2 WHERE id = $1",
            &[&Uuid::from(issued.grant.id()), &Uuid::from(other_policy_id)],
        )
        .await
        .expect("persisted authority changes");
    let mismatched = app
        .postgres()
        .agent_policy_document_upload_grants()
        .get(issued.grant.id(), workspace_id.into())
        .await
        .expect("grant reads")
        .expect("grant exists");
    assert!(mismatched.matches_authority(&authority).is_err());
    client
        .execute(
            "UPDATE agent_policy_document_upload_grants SET policy_id = $2 WHERE id = $1",
            &[&Uuid::from(issued.grant.id()), &Uuid::from(policy_id)],
        )
        .await
        .expect("persisted authority restores");
    client
        .execute(
            "UPDATE agent_policy_document_upload_grants SET issued_at = now() - interval '10 minutes', expires_at = now() - interval '5 minutes' WHERE id = $1",
            &[&Uuid::from(issued.grant.id())],
        )
        .await
        .expect("grant expires");
    let expired = app
        .postgres()
        .agent_policy_document_upload_grants()
        .get(issued.grant.id(), workspace_id.into())
        .await
        .expect("grant reads")
        .expect("grant exists");
    assert!(expired.ensure_pending_at(chrono::Utc::now()).is_err());
}

#[tokio::test]
async fn policy_machine_repository_rejects_a_same_id_cross_workspace_collision() {
    let app = TestApp::builder()
        .workspace("workspace", "Policy machine grant workspace")
        .with_default_membership()
        .workspace("other", "Other policy workspace")
        .with_default_membership()
        .build()
        .await;
    let workspace_id = app.workspace_id("workspace");
    let other_workspace_id = app.workspace_id("other");
    let policy_id = create_policy(&app, workspace_id, "Original policy").await;
    let other_policy_id = create_policy(&app, other_workspace_id, "Other policy").await;
    let issued = app
        .agent_policy_document_upload_grant_service()
        .issue(
            &app.agent_connection_context(workspace_id),
            policy_id,
            declaration(),
        )
        .await
        .expect("original grant issues");
    let other_credential = app
        .issue_api_token(other_workspace_id, WorkspacePermission::ALL.to_vec())
        .await;
    let issued_at = Utc::now();
    let colliding = AgentPolicyDocumentUploadGrant::issue(
        issued.grant.id(),
        other_workspace_id.into(),
        other_policy_id,
        declaration(),
        other_credential.user_id,
        other_credential.token_id,
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
                    .agent_policy_document_upload_grants()
                    .save(&colliding)
                    .await
            },
        )
        .await;
    assert!(save.is_err());

    let original = app
        .postgres()
        .agent_policy_document_upload_grants()
        .get(issued.grant.id(), workspace_id.into())
        .await
        .expect("original grant reads")
        .expect("original grant remains");
    assert_eq!(original.policy_id(), policy_id);
}

#[tokio::test]
async fn policy_transactional_grant_reads_hold_a_row_lock_while_verification_reads_do_not() {
    let app = TestApp::builder()
        .workspace("workspace", "Policy machine grant workspace")
        .with_default_membership()
        .build()
        .await;
    let workspace_id = app.workspace_id("workspace");
    let policy_id = create_policy(&app, workspace_id, "Locking policy").await;
    let issued = app
        .agent_policy_document_upload_grant_service()
        .issue(
            &app.agent_connection_context(workspace_id),
            policy_id,
            declaration(),
        )
        .await
        .expect("grant issues");
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
                        .agent_policy_document_upload_grants()
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
            "UPDATE agent_policy_document_upload_grants SET filename = filename WHERE id = $1",
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
        .agent_policy_document_upload_grants()
        .get(grant_id, workspace_id.into())
        .await
        .expect("verification read succeeds")
        .expect("grant exists");
    contender
        .execute(
            "UPDATE agent_policy_document_upload_grants SET filename = filename WHERE id = $1",
            &[&Uuid::from(grant_id)],
        )
        .await
        .expect("verification read leaves no row lock behind");
}

#[tokio::test]
async fn completed_policy_machine_grant_snapshot_round_trips_immediately_before_expiry() {
    let app = TestApp::builder()
        .workspace("workspace", "Policy machine grant workspace")
        .with_default_membership()
        .build()
        .await;
    let workspace_id = app.workspace_id("workspace");
    let policy_id = create_policy(&app, workspace_id, "Completed policy").await;
    let issued = app
        .agent_policy_document_upload_grant_service()
        .issue(
            &app.agent_connection_context(workspace_id),
            policy_id,
            declaration(),
        )
        .await
        .expect("grant issues");
    let document_id: Uuid = app
        .postgres()
        .get()
        .await
        .expect("database opens")
        .query_one(
            r#"
INSERT INTO documents (
    workspace_id, owner_type, owner_id, filename, content_type, content_length,
    object_key, checksum_sha256, checksum_crc32c, created_by_user_id
)
VALUES ($1, 'policy', $2, 'completed.pdf', 'application/pdf', 1,
        $3, 'sha256', 'crc32c', $4)
RETURNING id
"#,
            &[
                &workspace_id,
                &Uuid::from(policy_id),
                &format!("quarantine/{}/completed", issued.grant.id()),
                &app.user_id(),
            ],
        )
        .await
        .expect("document fixture inserts")
        .get("id");
    let mut grant = issued.grant;
    grant
        .complete(
            document_id.into(),
            grant.expires_at() - Duration::milliseconds(1),
        )
        .expect("completion immediately before expiry is valid");
    let grant_id = grant.id();
    let user_id = grant.issued_by_user_id();
    let connection_id = grant.issued_via_agent_connection_id();

    let round_trips = app
        .postgres()
        .in_agent_connection_workspace_context(
            workspace_id.into(),
            user_id,
            connection_id,
            async move |context| {
                let repository = context.agent_policy_document_upload_grants();
                repository.save(&grant).await?;
                Ok(repository.get(grant_id, workspace_id.into()).await? == Some(grant))
            },
        )
        .await
        .expect("completed snapshot saves and reloads");
    assert!(round_trips);
}

#[tokio::test]
async fn policy_machine_grant_schema_rejects_invalid_metadata_and_lifecycle() {
    let app = TestApp::builder()
        .workspace("workspace", "Policy machine grant workspace")
        .with_default_membership()
        .build()
        .await;
    let workspace_id = app.workspace_id("workspace");
    let policy_id = create_policy(&app, workspace_id, "Constrained policy").await;
    let issued = app
        .agent_policy_document_upload_grant_service()
        .issue(
            &app.agent_connection_context(workspace_id),
            policy_id,
            declaration(),
        )
        .await
        .expect("grant issues");
    let client = app.postgres().get().await.expect("database opens");
    let grant_id = Uuid::from(issued.grant.id());
    let document_id: Uuid = client
        .query_one(
            r#"
INSERT INTO documents (
    workspace_id, owner_type, owner_id, filename, content_type, content_length,
    object_key, checksum_sha256, checksum_crc32c, created_by_user_id
)
VALUES ($1, 'policy', $2, 'boundary.pdf', 'application/pdf', 1,
        $3, 'sha256', 'crc32c', $4)
RETURNING id
"#,
            &[
                &workspace_id,
                &Uuid::from(policy_id),
                &format!("quarantine/{}/boundary", issued.grant.id()),
                &app.user_id(),
            ],
        )
        .await
        .expect("document fixture inserts")
        .get("id");

    for statement in [
        "UPDATE agent_policy_document_upload_grants SET filename = '' WHERE id = $1",
        "UPDATE agent_policy_document_upload_grants SET content_type = ' application/pdf' WHERE id = $1",
        "UPDATE agent_policy_document_upload_grants SET expected_content_length = -1 WHERE id = $1",
        "UPDATE agent_policy_document_upload_grants SET expected_sha256 = decode('00', 'hex') WHERE id = $1",
        "UPDATE agent_policy_document_upload_grants SET expires_at = issued_at WHERE id = $1",
        "UPDATE agent_policy_document_upload_grants SET completed_at = now() WHERE id = $1",
    ] {
        client
            .execute(statement, &[&grant_id])
            .await
            .expect_err("database constraint rejects invalid grant state");
    }

    let oversized_content_type = format!("application/{}", "a".repeat(244));
    client
        .execute(
            "UPDATE agent_policy_document_upload_grants SET content_type = $2 WHERE id = $1",
            &[&grant_id, &oversized_content_type],
        )
        .await
        .expect_err("database constraint rejects an oversized content type");
    client
        .execute(
            "UPDATE agent_policy_document_upload_grants SET completed_at = expires_at, document_id = $2 WHERE id = $1",
            &[&grant_id, &document_id],
        )
        .await
        .expect_err("completion at expiry violates the snapshot constraint");
}

fn declaration() -> AgentPolicyDocumentUploadDeclaration {
    AgentPolicyDocumentUploadDeclaration::new(
        "policy.pdf".to_owned(),
        "application/pdf".to_owned(),
        483_920,
        None,
        25 * 1024 * 1024,
    )
    .into_result()
    .expect("declaration is valid")
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

fn tamper(token: &str) -> String {
    let mut bytes = token.as_bytes().to_vec();
    let index = bytes.len() / 2;
    bytes[index] = if bytes[index] == b'A' { b'B' } else { b'A' };
    String::from_utf8(bytes).expect("token remains UTF-8")
}
