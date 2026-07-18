use proofplane::{
    domain::{ControlId, CreatePolicyPayload, PolicyId, UpdatePolicyPayload},
    repository::ArchivePolicyResult,
    services::policies::{PolicyMutationError, PolicyService},
};
use uuid::Uuid;

use super::support::TestApp;

#[tokio::test]
async fn policy_lifecycle_normalizes_metadata_and_preserves_independent_state() {
    let app = TestApp::builder()
        .workspace("workspace", "Policy lifecycle workspace")
        .with_control("PP-B", "Second control", vec![])
        .with_control("PP-A", "First control", vec![])
        .with_default_membership()
        .workspace("other", "Other policy workspace")
        .with_control("PP-X", "Other control", vec![])
        .with_default_membership()
        .build()
        .await;
    let workspace_id = app.workspace_id("workspace");
    let context = app.agent_connection_context(workspace_id);
    let service = PolicyService::new(app.postgres_arc());
    let control_a = ControlId::from(app.control_id("workspace", "PP-A"));
    let control_b = ControlId::from(app.control_id("workspace", "PP-B"));
    let other_control = ControlId::from(app.control_id("other", "PP-X"));

    let created = service
        .create(
            context,
            CreatePolicyPayload {
                name: "  Zulu policy  ".to_owned(),
                description: Some("  Original description.  ".to_owned()),
                control_ids: vec![control_b, control_a],
            },
        )
        .await
        .expect("policy creates");
    assert_eq!(created.policy.name, "Zulu policy");
    assert_eq!(
        created.policy.description.as_deref(),
        Some("Original description.")
    );
    assert_eq!(mapping_codes(&created), ["PP-A", "PP-B"]);

    let alpha = service
        .create(
            context,
            CreatePolicyPayload {
                name: "alpha policy".to_owned(),
                description: None,
                control_ids: vec![],
            },
        )
        .await
        .expect("unmapped policy creates");
    let listed = service.list(context).await.expect("policies list");
    assert_eq!(
        listed.iter().map(|policy| policy.id).collect::<Vec<_>>(),
        [alpha.policy.id, created.policy.id]
    );

    let updated_at = created.policy.updated_at;
    assert!(service
        .detach_from_control(context, created.policy.id, control_a)
        .await
        .expect("mapping detaches"));
    assert!(!service
        .detach_from_control(context, created.policy.id, control_a)
        .await
        .expect("missing mapping resolves"));
    let attached = service
        .attach_to_control(context, created.policy.id, control_a)
        .await
        .expect("mapping attaches")
        .expect("policy and control are visible");
    assert_eq!(attached.control.id, control_a);
    assert!(matches!(
        service
            .attach_to_control(context, created.policy.id, control_a)
            .await,
        Err(PolicyMutationError::MappingExists)
    ));
    assert!(service
        .attach_to_control(context, created.policy.id, other_control)
        .await
        .expect("cross-workspace mapping resolves")
        .is_none());

    let after_mapping = service
        .get(context, created.policy.id)
        .await
        .expect("policy reads")
        .expect("policy exists");
    assert_eq!(after_mapping.policy.updated_at, updated_at);
    assert_eq!(mapping_codes(&after_mapping), ["PP-A", "PP-B"]);

    insert_policy_document(&app, created.policy.id, "uploaded").await;
    let updated = service
        .update(
            context,
            created.policy.id,
            UpdatePolicyPayload {
                name: "  Renamed policy  ".to_owned(),
                description: None,
            },
        )
        .await
        .expect("policy updates")
        .expect("policy exists");
    assert_eq!(updated.policy.name, "Renamed policy");
    assert_eq!(updated.policy.description, None);
    assert_eq!(mapping_codes(&updated), ["PP-A", "PP-B"]);
    assert_eq!(active_document_count(&app, created.policy.id).await, 1);
}

#[tokio::test]
async fn policy_creation_rolls_back_invalid_references_and_enforces_active_names() {
    let app = TestApp::builder()
        .workspace("workspace", "Policy validation workspace")
        .with_control("PP-A", "First control", vec![])
        .with_default_membership()
        .workspace("other", "Other validation workspace")
        .with_control("PP-X", "Other control", vec![])
        .with_default_membership()
        .build()
        .await;
    let workspace_id = app.workspace_id("workspace");
    let other_workspace_id = app.workspace_id("other");
    let context = app.agent_connection_context(workspace_id);
    let other_context = app.agent_connection_context(other_workspace_id);
    let service = PolicyService::new(app.postgres_arc());
    let control = ControlId::from(app.control_id("workspace", "PP-A"));
    let other_control = ControlId::from(app.control_id("other", "PP-X"));

    assert!(matches!(
        service
            .create(
                context,
                CreatePolicyPayload {
                    name: "Duplicate references".to_owned(),
                    description: None,
                    control_ids: vec![control, control],
                },
            )
            .await,
        Err(PolicyMutationError::Validation(_))
    ));
    assert_eq!(policy_count(&app, workspace_id).await, 0);

    for invalid_control in [ControlId::from(Uuid::new_v4()), other_control] {
        assert!(matches!(
            service
                .create(
                    context,
                    CreatePolicyPayload {
                        name: format!("Invalid reference {invalid_control}"),
                        description: None,
                        control_ids: vec![control, invalid_control],
                    },
                )
                .await,
            Err(PolicyMutationError::InvalidControlReferences)
        ));
    }
    assert_eq!(policy_count(&app, workspace_id).await, 0);

    let original = service
        .create(
            context,
            CreatePolicyPayload {
                name: "Access Policy".to_owned(),
                description: None,
                control_ids: vec![control],
            },
        )
        .await
        .expect("original policy creates");
    assert!(matches!(
        service
            .create(
                context,
                CreatePolicyPayload {
                    name: "access policy".to_owned(),
                    description: None,
                    control_ids: vec![],
                },
            )
            .await,
        Err(PolicyMutationError::NameTaken)
    ));
    service
        .create(
            other_context,
            CreatePolicyPayload {
                name: "ACCESS POLICY".to_owned(),
                description: None,
                control_ids: vec![other_control],
            },
        )
        .await
        .expect("same name in another workspace creates");

    assert!(matches!(
        service.archive(context, original.policy.id).await,
        Ok(ArchivePolicyResult::Archived { .. })
    ));
    service
        .create(
            context,
            CreatePolicyPayload {
                name: "access policy".to_owned(),
                description: None,
                control_ids: vec![],
            },
        )
        .await
        .expect("archived name can be reused");
}

#[tokio::test]
async fn policy_archive_blocks_in_progress_documents_and_hides_retained_rows() {
    let app = TestApp::builder()
        .workspace("workspace", "Policy archival workspace")
        .with_control("PP-A", "First control", vec![])
        .with_default_membership()
        .workspace("other", "Other archival workspace")
        .with_default_membership()
        .build()
        .await;
    let workspace_id = app.workspace_id("workspace");
    let context = app.agent_connection_context(workspace_id);
    let other_context = app.agent_connection_context(app.workspace_id("other"));
    let service = PolicyService::new(app.postgres_arc());
    let control = ControlId::from(app.control_id("workspace", "PP-A"));
    let policy = service
        .create(
            context,
            CreatePolicyPayload {
                name: "Archival policy".to_owned(),
                description: None,
                control_ids: vec![control],
            },
        )
        .await
        .expect("policy creates");
    let document_id = insert_policy_document(&app, policy.policy.id, "pending").await;

    assert_eq!(
        service
            .archive(context, policy.policy.id)
            .await
            .expect("archive resolves"),
        ArchivePolicyResult::DocumentInProgress
    );
    set_document_status(&app, document_id, "finalizing").await;
    assert_eq!(
        service
            .archive(context, policy.policy.id)
            .await
            .expect("archive resolves"),
        ArchivePolicyResult::DocumentInProgress
    );
    set_document_status(&app, document_id, "contains_virus").await;
    assert!(matches!(
        service.archive(context, policy.policy.id).await,
        Ok(ArchivePolicyResult::Archived { policy_id, .. }) if policy_id == policy.policy.id
    ));

    assert!(service
        .get(context, policy.policy.id)
        .await
        .expect("archived read resolves")
        .is_none());
    assert!(service
        .get(other_context, policy.policy.id)
        .await
        .expect("cross-workspace read resolves")
        .is_none());
    assert!(service
        .update(
            context,
            policy.policy.id,
            UpdatePolicyPayload {
                name: "Cannot update".to_owned(),
                description: None,
            },
        )
        .await
        .expect("archived update resolves")
        .is_none());
    assert!(service
        .attach_to_control(context, policy.policy.id, control)
        .await
        .expect("archived attach resolves")
        .is_none());
    assert!(!service
        .detach_from_control(context, policy.policy.id, control)
        .await
        .expect("archived detach resolves"));
    assert_eq!(
        service
            .archive(context, policy.policy.id)
            .await
            .expect("second archive resolves"),
        ArchivePolicyResult::NotFound
    );
    assert_eq!(retained_mapping_count(&app, policy.policy.id).await, 1);
    assert_eq!(active_document_count(&app, policy.policy.id).await, 1);
}

fn mapping_codes(detail: &proofplane::projections::policy_projection::PolicyDetail) -> Vec<&str> {
    let policy = &detail.policy;
    policy
        .control_mappings
        .iter()
        .map(|mapping| mapping.control.code.as_str())
        .collect()
}

async fn insert_policy_document(app: &TestApp, policy_id: PolicyId, status: &str) -> Uuid {
    let client = app.postgres().get().await.expect("connection opens");
    let id = Uuid::new_v4();
    let object_key = format!("policy-test/{id}");
    client
        .execute(
            r#"
INSERT INTO documents (
    id, workspace_id, owner_type, owner_id, filename, content_type, content_length, object_key,
    checksum_sha256, checksum_crc32c, upload_status
)
SELECT $1, p.workspace_id, 'policy', p.id, 'policy.pdf', 'application/pdf', 10,
       $3, 'sha256', 'crc32c', $4
FROM policies p
WHERE p.id = $2
"#,
            &[&id, &Uuid::from(policy_id), &object_key, &status],
        )
        .await
        .expect("policy document inserts");
    id
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

async fn policy_count(app: &TestApp, workspace_id: Uuid) -> i64 {
    app.postgres()
        .get()
        .await
        .expect("connection opens")
        .query_one(
            "SELECT count(*) AS count FROM policies WHERE workspace_id = $1",
            &[&workspace_id],
        )
        .await
        .expect("policy count reads")
        .get("count")
}

async fn retained_mapping_count(app: &TestApp, policy_id: PolicyId) -> i64 {
    app.postgres()
        .get()
        .await
        .expect("connection opens")
        .query_one(
            "SELECT count(*) AS count FROM policy_control_mappings WHERE policy_id = $1",
            &[&Uuid::from(policy_id)],
        )
        .await
        .expect("mapping count reads")
        .get("count")
}

async fn active_document_count(app: &TestApp, policy_id: PolicyId) -> i64 {
    app.postgres()
        .get()
        .await
        .expect("connection opens")
        .query_one(
            "SELECT count(*) AS count FROM documents WHERE owner_type = 'policy' AND owner_id = $1 AND archived = false",
            &[&Uuid::from(policy_id)],
        )
        .await
        .expect("document count reads")
        .get("count")
}
