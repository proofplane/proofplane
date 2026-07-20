use chrono::{DateTime, Utc};
use proofplane::{
    config::ObjectStorageConfig,
    seed::{seed_local_data, DemoDocumentSeedStatus},
};
use uuid::Uuid;

use crate::support::TestApp;

const LOCAL_WORKSPACE_ID: &str = "00000000-0000-4000-8000-000000000001";

#[derive(Debug, Clone, PartialEq, Eq)]
struct SeedPolicyState {
    id: Uuid,
    name: String,
    description: Option<String>,
    updated_at: DateTime<Utc>,
    mappings: Vec<(String, DateTime<Utc>)>,
    document_count: i64,
}

#[tokio::test]
async fn local_seed_converges_policy_fixtures_without_churning_existing_demo_data() {
    let app = TestApp::start().await;
    let storage = ObjectStorageConfig::Filesystem {
        root: app.object_storage_root().to_path_buf(),
    };
    let workspace_id = local_workspace_id();

    let first_summary = seed_local_data(app.postgres(), &storage)
        .await
        .expect("first local seed succeeds");
    assert_eq!(first_summary.demo_document, DemoDocumentSeedStatus::Seeded);

    let initial_policies = policy_states(&app, workspace_id).await;
    assert_representative_policy_catalog(&initial_policies);
    let initial_evidence_ids = evidence_ids(&app, workspace_id).await;
    assert_eq!(initial_evidence_ids.len(), 3);
    assert_existing_demo_identifiers(&app, workspace_id).await;

    let unrelated_policy_id = Uuid::new_v4();
    let access_control_id = control_id(workspace_id, "PP-AC-01");
    let client = app.postgres().get().await.expect("database opens");
    client
        .execute(
            "INSERT INTO policies (id, workspace_id, name, description) VALUES ($1, $2, 'User policy', 'Must survive reseeding')",
            &[&unrelated_policy_id, &workspace_id],
        )
        .await
        .expect("unrelated policy inserts");
    client
        .execute(
            "INSERT INTO policy_control_mappings (policy_id, control_id) VALUES ($1, $2)",
            &[&unrelated_policy_id, &access_control_id],
        )
        .await
        .expect("unrelated mapping inserts");

    client
        .execute(
            "UPDATE policies SET description = 'drifted', archived_at = now() WHERE id = $1",
            &[&policy_id(workspace_id, "incident-response")],
        )
        .await
        .expect("seeded policy metadata drifts");
    client
        .execute(
            "INSERT INTO policy_control_mappings (policy_id, control_id) VALUES ($1, $2)",
            &[
                &policy_id(workspace_id, "acceptable-use"),
                &access_control_id,
            ],
        )
        .await
        .expect("extra seeded mapping inserts");
    client
        .execute(
            "DELETE FROM policy_control_mappings WHERE policy_id = $1",
            &[&policy_id(workspace_id, "incident-response")],
        )
        .await
        .expect("required seeded mapping deletes");

    seed_local_data(app.postgres(), &storage)
        .await
        .expect("second local seed succeeds");
    let converged = policy_states(&app, workspace_id).await;
    assert_representative_policy_catalog(&converged);
    let unrelated = converged
        .iter()
        .find(|policy| policy.id == unrelated_policy_id)
        .expect("unrelated policy remains");
    assert_eq!(
        unrelated.description.as_deref(),
        Some("Must survive reseeding")
    );
    assert_eq!(
        unrelated
            .mappings
            .iter()
            .map(|(code, _)| code.as_str())
            .collect::<Vec<_>>(),
        ["PP-AC-01"]
    );
    assert_eq!(evidence_ids(&app, workspace_id).await, initial_evidence_ids);

    seed_local_data(app.postgres(), &storage)
        .await
        .expect("third local seed succeeds");
    assert_eq!(policy_states(&app, workspace_id).await, converged);
    assert_eq!(evidence_ids(&app, workspace_id).await, initial_evidence_ids);
    assert_existing_demo_identifiers(&app, workspace_id).await;
}

fn assert_representative_policy_catalog(policies: &[SeedPolicyState]) {
    let seeded = policies
        .iter()
        .filter(|policy| policy.name != "User policy")
        .collect::<Vec<_>>();
    assert_eq!(
        seeded
            .iter()
            .map(|policy| policy.name.as_str())
            .collect::<Vec<_>>(),
        [
            "Acceptable Use Policy",
            "Incident Response Policy",
            "Information Security Policy",
        ]
    );

    let expected = [
        ("acceptable-use", false, Vec::<&str>::new()),
        ("incident-response", true, vec!["PP-IR-01"]),
        ("information-security", true, vec!["PP-AC-01", "PP-VM-01"]),
    ];
    for (policy, (slug, described, mappings)) in seeded.into_iter().zip(expected) {
        assert_eq!(policy.id, policy_id(local_workspace_id(), slug));
        assert_eq!(policy.description.is_some(), described);
        assert_eq!(
            policy
                .mappings
                .iter()
                .map(|(code, _)| code.as_str())
                .collect::<Vec<_>>(),
            mappings
        );
        assert_eq!(policy.document_count, 0);
    }
}

async fn policy_states(app: &TestApp, workspace_id: Uuid) -> Vec<SeedPolicyState> {
    let client = app.postgres().get().await.expect("database opens");
    let rows = client
        .query(
            r#"
SELECT p.id, p.name, p.description, p.updated_at,
       (SELECT count(*) FROM documents d WHERE d.owner_type = 'policy' AND d.owner_id = p.id) AS document_count
FROM policies p
WHERE p.workspace_id = $1
ORDER BY lower(p.name), p.id
"#,
            &[&workspace_id],
        )
        .await
        .expect("seeded policies load");

    let mut policies = Vec::with_capacity(rows.len());
    for row in rows {
        let policy_id = row.get("id");
        let mappings = client
            .query(
                r#"
SELECT c.code, m.created_at
FROM policy_control_mappings m
JOIN controls c ON c.id = m.control_id
WHERE m.policy_id = $1
ORDER BY c.code, c.id
"#,
                &[&policy_id],
            )
            .await
            .expect("seeded policy mappings load")
            .into_iter()
            .map(|mapping| (mapping.get("code"), mapping.get("created_at")))
            .collect();
        policies.push(SeedPolicyState {
            id: policy_id,
            name: row.get("name"),
            description: row.get("description"),
            updated_at: row.get("updated_at"),
            mappings,
            document_count: row.get("document_count"),
        });
    }

    policies
}

async fn evidence_ids(app: &TestApp, workspace_id: Uuid) -> Vec<(String, Uuid)> {
    app.postgres()
        .get()
        .await
        .expect("database opens")
        .query(
            "SELECT title, id FROM evidence WHERE workspace_id = $1 ORDER BY title, id",
            &[&workspace_id],
        )
        .await
        .expect("demo evidence load")
        .into_iter()
        .map(|row| (row.get("title"), row.get("id")))
        .collect()
}

async fn assert_existing_demo_identifiers(app: &TestApp, workspace_id: Uuid) {
    let client = app.postgres().get().await.expect("database opens");
    let control_ids =
        ["PP-AC-01", "PP-VM-01", "PP-IR-01"].map(|code| control_id(workspace_id, code));
    let control_count: i64 = client
        .query_one(
            "SELECT count(*) FROM controls WHERE id = ANY($1::uuid[])",
            &[&control_ids.as_slice()],
        )
        .await
        .expect("demo controls count")
        .get(0);
    assert_eq!(control_count, 3);

    let submission_count: i64 = client
        .query_one(
            "SELECT count(*) FROM evidence_submissions WHERE id = $1",
            &[&seed_uuid(
                "demo:evidence-submission:quarterly-access-review",
            )],
        )
        .await
        .expect("demo evidence submission counts")
        .get(0);
    assert_eq!(submission_count, 1);
}

fn local_workspace_id() -> Uuid {
    Uuid::parse_str(LOCAL_WORKSPACE_ID).expect("local workspace ID is valid")
}

fn control_id(workspace_id: Uuid, code: &str) -> Uuid {
    seed_uuid(&format!("workspace:{workspace_id}:control:{code}"))
}

fn policy_id(workspace_id: Uuid, slug: &str) -> Uuid {
    seed_uuid(&format!("workspace:{workspace_id}:policy:{slug}"))
}

fn seed_uuid(name: &str) -> Uuid {
    let namespace =
        Uuid::parse_str("60dcb0ee-3c16-4767-bdda-25cb3bfaf300").expect("seed namespace is valid");
    Uuid::new_v5(&namespace, name.as_bytes())
}
