use refinery::Target;
use testcontainers::{runners::AsyncRunner, ContainerAsync};
use testcontainers_modules::postgres;
use tokio_postgres::NoTls;
use uuid::Uuid;

use proofplane::store;

#[tokio::test]
async fn owned_documents_migration_preserves_legacy_document_rows() {
    let container = postgres::Postgres::default()
        .start()
        .await
        .expect("Postgres test container starts");
    let mut client = connect(&container).await;

    store::migration_runner()
        .set_target(Target::Version(6))
        .run_async(&mut client)
        .await
        .expect("legacy migrations run");

    let workspace_id = Uuid::parse_str("00000000-0000-4000-8000-000000000701").unwrap();
    let submission_id = Uuid::parse_str("00000000-0000-4000-8000-000000000703").unwrap();
    let evidence_document_id = Uuid::parse_str("00000000-0000-4000-8000-000000000704").unwrap();
    let policy_id = Uuid::parse_str("00000000-0000-4000-8000-000000000705").unwrap();
    let policy_document_id = Uuid::parse_str("00000000-0000-4000-8000-000000000706").unwrap();
    let policy_created_at = "2026-07-17T12:00:00Z"
        .parse::<chrono::DateTime<chrono::Utc>>()
        .unwrap();

    client
        .batch_execute(
            r#"
INSERT INTO workspaces (id, name)
VALUES ('00000000-0000-4000-8000-000000000701', 'Migration workspace');
INSERT INTO evidence_requests (
    id, workspace_id, title, description, collection_instructions, cadence,
    due_at, schedule_anchor_at, status
) VALUES (
    '00000000-0000-4000-8000-000000000702',
    '00000000-0000-4000-8000-000000000701',
    'Request', 'Description', 'Collect it', 'once', now(), now(), 'active'
);
INSERT INTO users (id, auth0_sub)
VALUES ('00000000-0000-4000-8000-000000000707', 'migration-user');
INSERT INTO agent_connections (
    id, user_id, workspace_id, auth0_subject, auth0_client_id,
    client_display_name, resource, status, pending_expires_at, activated_at
) VALUES (
    '00000000-0000-4000-8000-000000000708',
    '00000000-0000-4000-8000-000000000707',
    '00000000-0000-4000-8000-000000000701',
    'migration-user', 'migration-client', 'Migration client',
    'https://proofplane.local', 'active', now() + interval '1 hour', now()
);
INSERT INTO evidence_submissions (
    id, evidence_request_id, coverage_start_at, coverage_end_at, source_system,
    collection_method, submitted_by_agent_connection_id
) VALUES (
    '00000000-0000-4000-8000-000000000703',
    '00000000-0000-4000-8000-000000000702',
    now(), now(), 'system', 'manual', '00000000-0000-4000-8000-000000000708'
);
INSERT INTO evidence_documents (
    id, evidence_submission_id, created_by_user_id, filename, content_type, content_length, object_key,
    checksum_sha256, checksum_crc32c, archived, upload_status
) VALUES (
    '00000000-0000-4000-8000-000000000704',
    '00000000-0000-4000-8000-000000000703',
    '00000000-0000-4000-8000-000000000707',
    'evidence.txt', 'text/plain', 8, 'legacy/evidence', 'e-sha', 'e-crc', true, 'failed'
);
INSERT INTO policies (id, workspace_id, name)
VALUES (
    '00000000-0000-4000-8000-000000000705',
    '00000000-0000-4000-8000-000000000701',
    'Policy'
);
INSERT INTO policy_documents (
    id, policy_id, created_by_user_id, filename, content_type, content_length, object_key,
    checksum_sha256, checksum_crc32c, upload_status, created_at
) VALUES (
    '00000000-0000-4000-8000-000000000706',
    '00000000-0000-4000-8000-000000000705',
    '00000000-0000-4000-8000-000000000707',
    'policy.pdf', 'application/pdf', 10, 'legacy/policy', 'p-sha', 'p-crc',
    'uploaded', '2026-07-17T12:00:00Z'
);
"#,
        )
        .await
        .expect("legacy document rows insert");

    store::migration_runner()
        .run_async(&mut client)
        .await
        .expect("owned documents migration runs");

    let rows = client
        .query(
            r#"
SELECT id, workspace_id, owner_type, owner_id, filename, object_key, archived,
       created_by_user_id, upload_status, created_at
FROM documents
ORDER BY owner_type
"#,
            &[],
        )
        .await
        .expect("migrated documents load");
    assert_eq!(rows.len(), 2);

    let evidence = &rows[0];
    assert_eq!(evidence.get::<_, Uuid>("id"), evidence_document_id);
    assert_eq!(evidence.get::<_, Uuid>("workspace_id"), workspace_id);
    assert_eq!(
        evidence.get::<_, String>("owner_type"),
        "evidence_submission"
    );
    assert_eq!(evidence.get::<_, Uuid>("owner_id"), submission_id);
    assert_eq!(
        evidence.get::<_, Uuid>("created_by_user_id"),
        Uuid::parse_str("00000000-0000-4000-8000-000000000707").unwrap()
    );
    assert_eq!(evidence.get::<_, String>("filename"), "evidence.txt");
    assert_eq!(evidence.get::<_, String>("object_key"), "legacy/evidence");
    assert!(evidence.get::<_, bool>("archived"));
    assert_eq!(evidence.get::<_, String>("upload_status"), "failed");

    let policy = &rows[1];
    assert_eq!(policy.get::<_, Uuid>("id"), policy_document_id);
    assert_eq!(policy.get::<_, String>("owner_type"), "policy");
    assert_eq!(policy.get::<_, Uuid>("owner_id"), policy_id);
    assert_eq!(
        policy.get::<_, Uuid>("created_by_user_id"),
        Uuid::parse_str("00000000-0000-4000-8000-000000000707").unwrap()
    );
    assert_eq!(policy.get::<_, String>("upload_status"), "uploaded");
    assert_eq!(
        policy.get::<_, chrono::DateTime<chrono::Utc>>("created_at"),
        policy_created_at
    );

    let old_tables = client
        .query_one(
            "SELECT to_regclass('evidence_documents')::text, to_regclass('policy_documents')::text",
            &[],
        )
        .await
        .expect("legacy table state loads");
    assert_eq!(old_tables.get::<_, Option<String>>(0), None);
    assert_eq!(old_tables.get::<_, Option<String>>(1), None);
}

async fn connect(container: &ContainerAsync<postgres::Postgres>) -> tokio_postgres::Client {
    let host = container
        .get_host()
        .await
        .expect("Postgres test container has a host");
    let port = container
        .get_host_port_ipv4(5432)
        .await
        .expect("Postgres test container exposes Postgres");
    let database_url = format!("postgres://postgres:postgres@{host}:{port}/postgres");
    let (client, connection) = tokio_postgres::connect(&database_url, NoTls)
        .await
        .expect("fixture database connection opens");
    tokio::spawn(async move {
        connection.await.expect("fixture database connection runs");
    });
    client
}
