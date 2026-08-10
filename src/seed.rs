use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use bytes::Bytes;
use chrono::{DateTime, Utc};
use futures_util::stream;
use secrecy::ExposeSecret;
use uuid::Uuid;

use crate::{
    authentication::AgentConnectionContext,
    config::{load_from_env, ConfigError, ObjectStorageConfig},
    domain::{
        AgentConnectionId, Evidence, EvidenceDefinition, EvidenceId, EvidenceStatus,
        ProvisionUserPayload, UserId, WorkspaceId, WorkspacePermission, WorkspacePermissions,
        WorkspaceRole,
    },
    object_storage::{
        FilesystemObjectStore, ObjectKey, ObjectStore, PutObjectRequest, StorageError,
    },
    observability,
    persistence::{self, NewWorkspaceMembership, Postgres},
};
use thiserror::Error;
use tracing::debug;

#[derive(Debug, Error)]
pub enum Error {
    #[error("postgres connection error")]
    DatabaseConnection(#[from] persistence::connection::Error),

    #[error("configuration error: {0}")]
    Config(#[source] Box<ConfigError>),

    #[error("observability initialization error")]
    Observability(#[from] observability::Error),

    #[error("database migration error")]
    Migrations(#[from] refinery::Error),

    #[error("repository error")]
    Repository(#[from] crate::persistence::Error),

    #[error("connection pool error")]
    Pool(#[from] deadpool_postgres::PoolError),

    #[error("database error")]
    Database(#[from] tokio_postgres::Error),

    #[error("seed timestamp parse error")]
    Timestamp(#[from] chrono::ParseError),

    #[error("object storage error")]
    ObjectStorage(#[from] StorageError),
}

const LOCAL_OWNER_AUTH0_SUB: &str = "auth0|local-owner";
const LOCAL_AGENT_CONNECTION_ID: &str = "00000000-0000-4000-8000-000000000302";
const DEMO_SUBMISSION_FILENAME: &str = "quarterly-access-review.csv";
const DEMO_SUBMISSION_CONTENT_TYPE: &str = "text/csv";
const DEMO_SUBMISSION_BYTES: &[u8] = b"user,email,system,reviewer,reviewed_at,decision\nLocal Owner,owner@proofplane.local,Production Console,System,2026-06-14T16:00:00Z,approved\nLocal Service Account,service@proofplane.local,Deployment Pipeline,System,2026-06-14T16:10:00Z,approved\n";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeedSummary {
    pub demo_document: DemoDocumentSeedStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DemoDocumentSeedStatus {
    Seeded,
    SkippedNonFilesystemStorage,
}

pub async fn run() -> Result<SeedSummary, Error> {
    let config = load_from_env().map_err(|error| Error::Config(Box::new(error)))?;
    observability::init_cli_tracing(&config.observability)?;

    let mut client = persistence::conn(config.postgres.expose_secret()).await?;

    debug!("running migrations");
    persistence::migrate(&mut client).await?;
    debug!("done running migrations");

    let pool = persistence::conn_pool(config.postgres.expose_secret(), 4).await?;
    let postgres = Postgres::new(pool);

    seed_local_data(&postgres, &config.object_storage).await
}

/// Seeds the deterministic local/demo records into an initialized repository.
///
/// This entry point accepts dependencies directly so the complete seed can be
/// exercised repeatedly by integration tests without process-global config.
pub async fn seed_local_data(
    postgres: &Postgres,
    object_storage: &ObjectStorageConfig,
) -> Result<SeedSummary, Error> {
    debug!("seeding local data");
    seed_workspace(postgres).await?;
    let owner_id = seed_local_owner(postgres).await?;
    let connection = seed_agent_connection(postgres, owner_id).await?;
    seed_evidence(postgres, connection).await?;
    seed_frameworks_and_controls(postgres).await?;
    seed_policies(postgres).await?;
    let demo_document = seed_demo_evidence_submission(postgres, object_storage, owner_id).await?;
    debug!("done seeding local data");

    Ok(SeedSummary { demo_document })
}

async fn seed_workspace(repository: &Postgres) -> Result<(), Error> {
    let client = repository.get().await?;
    for (id, slug, name) in [
        (
            local_authorized_workspace_id(),
            Some("local-workspace".to_owned()),
            "Local Workspace".to_owned(),
        ),
        (
            local_unauthorized_workspace_id(),
            Some("local-unauthorized-workspace".to_owned()),
            "Local Unauthorized Workspace".to_owned(),
        ),
    ] {
        client
            .execute(
                r#"INSERT INTO workspaces (id, slug, name)
VALUES ($1, $2, $3)
ON CONFLICT (id) DO UPDATE SET slug = EXCLUDED.slug, name = EXCLUDED.name"#,
                &[&Uuid::from(id), &slug, &name],
            )
            .await?;
    }

    Ok(())
}

async fn seed_local_owner(repository: &Postgres) -> Result<UserId, Error> {
    let user = repository
        .upsert_user_by_auth0_sub(&ProvisionUserPayload {
            auth0_sub: LOCAL_OWNER_AUTH0_SUB.to_owned(),
            email: Some("owner@proofplane.local".to_owned()),
            name: Some("Local Owner".to_owned()),
        })
        .await?;
    let workspace_id = local_authorized_workspace_id();

    if repository
        .get_membership_role(workspace_id, user.id)
        .await?
        .is_none()
    {
        repository
            .in_unit_of_work(async move |unit_of_work| {
                unit_of_work
                    .insert_workspace_membership(&NewWorkspaceMembership {
                        user_id: user.id,
                        workspace_id,
                        role: WorkspaceRole::Owner,
                    })
                    .await?;

                Ok(())
            })
            .await?;
    }

    Ok(user.id)
}

async fn seed_agent_connection(
    repository: &Postgres,
    user_id: UserId,
) -> Result<AgentConnectionContext, Error> {
    let workspace_id = local_authorized_workspace_id();
    let connection_id = local_agent_connection_id();
    let permissions = WorkspacePermission::ALL.to_vec();

    let client = repository.get().await?;
    client
        .execute(
            r#"
INSERT INTO agent_connections (
    id,
    user_id,
    workspace_id,
    auth0_subject,
    auth0_client_id,
    client_display_name,
    resource,
    status,
    pending_expires_at,
    activated_at
)
VALUES ($1, $2, $3, $4, 'local-seed-client', 'Local Seed MCP Client', 'http://127.0.0.1:3000/mcp', 'active', $5, now())
ON CONFLICT (id) DO UPDATE
SET user_id = EXCLUDED.user_id,
    workspace_id = EXCLUDED.workspace_id,
    auth0_subject = EXCLUDED.auth0_subject,
    auth0_client_id = EXCLUDED.auth0_client_id,
    client_display_name = EXCLUDED.client_display_name,
    resource = EXCLUDED.resource,
    status = EXCLUDED.status,
    pending_expires_at = EXCLUDED.pending_expires_at,
    activated_at = EXCLUDED.activated_at,
    revoked_at = NULL
"#,
            &[
                &Uuid::from(connection_id),
                &Uuid::from(user_id),
                &Uuid::from(workspace_id),
                &LOCAL_OWNER_AUTH0_SUB,
                &timestamp("2036-01-01T00:00:00Z")?,
            ],
        )
        .await?;
    client
        .execute(
            "DELETE FROM agent_connection_permissions WHERE agent_connection_id = $1",
            &[&Uuid::from(connection_id)],
        )
        .await?;
    for permission in permissions {
        client
            .execute(
                "INSERT INTO agent_connection_permissions (agent_connection_id, permission) VALUES ($1, $2)",
                &[&Uuid::from(connection_id), &permission.as_str()],
            )
            .await?;
    }

    Ok(AgentConnectionContext {
        user_id,
        connection_id,
        workspace_id,
        permissions: WorkspacePermissions::all(),
    })
}

async fn seed_evidence(
    repository: &Postgres,
    _connection: AgentConnectionContext,
) -> Result<(), Error> {
    let workspace_id = local_workspace_id();
    let seeds = demo_evidence()?;
    let existing = repository
        .workspace_reads(workspace_id)
        .await?
        .evidence()
        .list()
        .await?;

    repository
        .in_unit_of_work(async move |unit_of_work| {
            let workspace = unit_of_work.workspace(workspace_id);
            for seed in seeds {
                let existing_id = existing
                    .iter()
                    .find(|evidence| evidence.title == seed.title)
                    .map(|evidence| evidence.id);
                let definition = EvidenceDefinition::new(
                    seed.title,
                    seed.description,
                    seed.collection_instructions,
                )
                .into_result()
                .map_err(|_| {
                    crate::persistence::Error::InvariantViolation(
                        "seed evidence definition must be valid",
                    )
                })?;
                let evidence_repository = workspace.aggregates().evidence();
                if let Some(existing_id) = existing_id {
                    let mut aggregate = evidence_repository.get(existing_id).await?.ok_or(
                        crate::persistence::Error::InvariantViolation(
                            "listed seed evidence must be readable as an aggregate",
                        ),
                    )?;
                    aggregate
                        .replace(definition, seed.status, Utc::now())
                        .map_err(|_| {
                            crate::persistence::Error::InvariantViolation(
                                "seed evidence replacement must be valid",
                            )
                        })?;
                    evidence_repository.save(&aggregate).await?;
                } else {
                    let aggregate = Evidence::define(
                        EvidenceId::from(Uuid::new_v4()),
                        workspace_id,
                        definition,
                        seed.status,
                        Utc::now(),
                    );
                    evidence_repository.save(&aggregate).await?;
                }
            }

            Ok(())
        })
        .await?;

    Ok(())
}

async fn seed_frameworks_and_controls(repository: &Postgres) -> Result<(), Error> {
    let mut client = repository.get().await?;
    let transaction = client.transaction().await?;

    transaction
        .execute(
            r#"
INSERT INTO frameworks (id, code, name, description)
VALUES ($1, 'soc2', 'SOC 2', 'AICPA Trust Services Criteria for service organizations.')
ON CONFLICT (code) DO UPDATE
SET name = EXCLUDED.name,
    description = EXCLUDED.description
"#,
            &[&soc2_framework_id()],
        )
        .await?;

    for requirement in demo_soc2_requirements() {
        transaction
            .execute(
                r#"
INSERT INTO framework_requirements (id, framework_id, code, title, description)
VALUES ($1, $2, $3, $4, $5)
ON CONFLICT (framework_id, code) DO UPDATE
SET title = EXCLUDED.title,
    description = EXCLUDED.description
"#,
                &[
                    &requirement.id,
                    &soc2_framework_id(),
                    &requirement.code,
                    &requirement.title,
                    &requirement.description,
                ],
            )
            .await?;
    }

    for workspace_id in [
        local_authorized_workspace_id(),
        local_unauthorized_workspace_id(),
    ] {
        for control in demo_controls(workspace_id) {
            transaction
                .execute(
                    r#"
INSERT INTO controls (id, workspace_id, code, title, description)
VALUES ($1, $2, $3, $4, $5)
ON CONFLICT (workspace_id, code) DO UPDATE
SET title = EXCLUDED.title,
    description = EXCLUDED.description,
    updated_at = now()
"#,
                    &[
                        &control.id,
                        &Uuid::from(workspace_id),
                        &control.code,
                        &control.title,
                        &control.description,
                    ],
                )
                .await?;
            transaction
                .execute(
                    "DELETE FROM control_framework_requirement_mappings WHERE control_id = $1",
                    &[&control.id],
                )
                .await?;
            for requirement_id in control.requirement_ids {
                transaction
                    .execute(
                        r#"
INSERT INTO control_framework_requirement_mappings (control_id, framework_requirement_id)
VALUES ($1, $2)
ON CONFLICT DO NOTHING
"#,
                        &[&control.id, &requirement_id],
                    )
                    .await?;
            }
        }
    }

    transaction.commit().await?;

    Ok(())
}

async fn seed_policies(repository: &Postgres) -> Result<(), Error> {
    let mut client = repository.get().await?;
    let transaction = client.transaction().await?;
    let workspace_id = local_authorized_workspace_id();

    for policy in demo_policies(workspace_id) {
        let policy_id = policy.id;
        let description = policy.description;
        transaction
            .execute(
                r#"
INSERT INTO policies (id, workspace_id, name, description)
VALUES ($1, $2, $3, $4)
ON CONFLICT (id) DO UPDATE
SET workspace_id = EXCLUDED.workspace_id,
    name = EXCLUDED.name,
    description = EXCLUDED.description,
    archived_at = NULL,
    updated_at = CASE
        WHEN (policies.workspace_id, policies.name, policies.description, policies.archived_at)
             IS DISTINCT FROM
             (EXCLUDED.workspace_id, EXCLUDED.name, EXCLUDED.description, NULL::timestamptz)
        THEN now()
        ELSE policies.updated_at
    END
"#,
                &[
                    &policy_id,
                    &Uuid::from(workspace_id),
                    &policy.name,
                    &description,
                ],
            )
            .await?;

        let control_ids = policy
            .control_codes
            .iter()
            .map(|code| control_id(workspace_id, code))
            .collect::<Vec<_>>();
        transaction
            .execute(
                r#"
DELETE FROM policy_control_mappings
WHERE policy_id = $1
  AND NOT (control_id = ANY($2::uuid[]))
"#,
                &[&policy_id, &control_ids],
            )
            .await?;
        for control_id in control_ids {
            transaction
                .execute(
                    r#"
INSERT INTO policy_control_mappings (policy_id, control_id)
VALUES ($1, $2)
ON CONFLICT DO NOTHING
"#,
                    &[&policy_id, &control_id],
                )
                .await?;
        }
    }

    transaction.commit().await?;

    Ok(())
}

async fn seed_demo_evidence_submission(
    repository: &Postgres,
    object_storage: &ObjectStorageConfig,
    created_by_user_id: UserId,
) -> Result<DemoDocumentSeedStatus, Error> {
    let ObjectStorageConfig::Filesystem { root } = object_storage else {
        seed_demo_submission_row(repository).await?;
        return Ok(DemoDocumentSeedStatus::SkippedNonFilesystemStorage);
    };

    let object_store = FilesystemObjectStore::new(root).await?;
    let object_key = demo_document_object_key()?;
    let metadata = object_store
        .put_object(PutObjectRequest {
            key: object_key,
            content_type: DEMO_SUBMISSION_CONTENT_TYPE.to_owned(),
            chunks: stream::once(async { Ok(Bytes::from_static(DEMO_SUBMISSION_BYTES)) }),
        })
        .await?;

    upsert_demo_submission_and_document(repository, &metadata.sha256, created_by_user_id).await?;

    Ok(DemoDocumentSeedStatus::Seeded)
}

async fn seed_demo_submission_row(repository: &Postgres) -> Result<(), Error> {
    let mut client = repository.get().await?;
    let transaction = client.transaction().await?;
    let evidence_id = demo_evidence_id(&transaction).await?;
    upsert_demo_submission(&transaction, evidence_id).await?;
    transaction.commit().await?;

    Ok(())
}

async fn upsert_demo_submission_and_document(
    repository: &Postgres,
    checksum_sha256: &str,
    created_by_user_id: UserId,
) -> Result<(), Error> {
    let mut client = repository.get().await?;
    let transaction = client.transaction().await?;
    let evidence_id = demo_evidence_id(&transaction).await?;
    upsert_demo_submission(&transaction, evidence_id).await?;

    let document_id = demo_document_id();
    let document_object_key = demo_document_object_key()?.to_string();
    let checksum_crc32c = demo_document_crc32c_base64();
    let content_length = DEMO_SUBMISSION_BYTES.len() as i64;

    transaction
        .execute(
            r#"
INSERT INTO documents (
    id,
    workspace_id,
    owner_type,
    owner_id,
    created_by_user_id,
    filename,
    content_type,
    content_length,
    object_key,
    checksum_sha256,
    checksum_crc32c,
    upload_status
)
VALUES ($1, $2, 'evidence_submission', $3, $4, $5, $6, $7, $8, $9, $10, 'uploaded')
ON CONFLICT (id) DO UPDATE
SET workspace_id = EXCLUDED.workspace_id,
    owner_type = EXCLUDED.owner_type,
    owner_id = EXCLUDED.owner_id,
    created_by_user_id = EXCLUDED.created_by_user_id,
    filename = EXCLUDED.filename,
    content_type = EXCLUDED.content_type,
    content_length = EXCLUDED.content_length,
    object_key = EXCLUDED.object_key,
    checksum_sha256 = EXCLUDED.checksum_sha256,
    checksum_crc32c = EXCLUDED.checksum_crc32c,
    upload_status = EXCLUDED.upload_status
"#,
            &[
                &document_id,
                &Uuid::from(local_authorized_workspace_id()),
                &demo_submission_id(),
                &Uuid::from(created_by_user_id),
                &DEMO_SUBMISSION_FILENAME,
                &DEMO_SUBMISSION_CONTENT_TYPE,
                &content_length,
                &document_object_key,
                &checksum_sha256,
                &checksum_crc32c,
            ],
        )
        .await?;

    transaction.commit().await?;

    Ok(())
}

async fn demo_evidence_id(transaction: &tokio_postgres::Transaction<'_>) -> Result<Uuid, Error> {
    let row = transaction
        .query_one(
            r#"
SELECT id
FROM evidence
WHERE workspace_id = $1
  AND title = 'Quarterly access review'
"#,
            &[&Uuid::from(local_authorized_workspace_id())],
        )
        .await?;

    Ok(row.get("id"))
}

async fn upsert_demo_submission(
    transaction: &tokio_postgres::Transaction<'_>,
    evidence_id: Uuid,
) -> Result<(), Error> {
    transaction
        .execute(
            r#"
INSERT INTO evidence_submissions (
    id,
    evidence_id,
    submitted_by_agent_connection_id,
    received_at,
    valid_from,
    valid_until
)
VALUES ($1, $2, $3, $4, $5, $6)
ON CONFLICT (id) DO UPDATE
SET evidence_id = EXCLUDED.evidence_id,
    submitted_by_agent_connection_id = EXCLUDED.submitted_by_agent_connection_id,
    received_at = EXCLUDED.received_at,
    valid_from = EXCLUDED.valid_from,
    valid_until = EXCLUDED.valid_until
"#,
            &[
                &demo_submission_id(),
                &evidence_id,
                &Uuid::from(local_agent_connection_id()),
                &timestamp("2026-06-14T16:30:00Z")?,
                &timestamp("2026-04-01T00:00:00Z")?,
                &timestamp("2026-06-30T23:59:59Z")?,
            ],
        )
        .await?;

    Ok(())
}

struct SeedEvidence {
    title: String,
    description: String,
    collection_instructions: String,
    status: EvidenceStatus,
}

struct SeedFrameworkRequirement {
    id: Uuid,
    code: &'static str,
    title: &'static str,
    description: &'static str,
}

struct SeedControl {
    id: Uuid,
    code: &'static str,
    title: &'static str,
    description: &'static str,
    requirement_ids: Vec<Uuid>,
}

struct SeedPolicy {
    id: Uuid,
    name: &'static str,
    description: Option<&'static str>,
    control_codes: Vec<&'static str>,
}

fn demo_evidence() -> Result<Vec<SeedEvidence>, Error> {
    Ok(vec![
        SeedEvidence {
            title: "Quarterly access review".to_owned(),
            description: "Confirm user access reviews are completed for production systems."
                .to_owned(),
            collection_instructions:
                "Export the completed access review report from the identity provider and include reviewer sign-off."
                    .to_owned(),
            status: EvidenceStatus::Active,
        },
        SeedEvidence {
            title: "Monthly vulnerability scan".to_owned(),
            description:
                "Confirm vulnerability scans are performed for the production environment."
                    .to_owned(),
            collection_instructions:
                "Attach the monthly scanner summary showing scan scope, date, and critical findings."
                    .to_owned(),
            status: EvidenceStatus::Active,
        },
        SeedEvidence {
            title: "Annual incident response tabletop".to_owned(),
            description: "Confirm the incident response tabletop exercise is completed annually."
                .to_owned(),
            collection_instructions:
                "Attach the exercise agenda, participant list, findings, and remediation actions."
                    .to_owned(),
            status: EvidenceStatus::Paused,
        },
    ])
}

fn demo_soc2_requirements() -> Vec<SeedFrameworkRequirement> {
    vec![
        SeedFrameworkRequirement {
            id: soc2_requirement_id("CC6.1"),
            code: "CC6.1",
            title: "Logical access security",
            description:
                "Logical access security software, infrastructure, and architectures protect information assets.",
        },
        SeedFrameworkRequirement {
            id: soc2_requirement_id("CC6.2"),
            code: "CC6.2",
            title: "Access credentials",
            description:
                "New internal and external users are registered and authorized before credentials are issued.",
        },
        SeedFrameworkRequirement {
            id: soc2_requirement_id("CC7.1"),
            code: "CC7.1",
            title: "System monitoring",
            description:
                "Detection and monitoring procedures identify changes that could impair system objectives.",
        },
        SeedFrameworkRequirement {
            id: soc2_requirement_id("CC7.4"),
            code: "CC7.4",
            title: "Incident response",
            description:
                "Security incidents are responded to, mitigated, and resolved according to response procedures.",
        },
    ]
}

fn demo_controls(workspace_id: WorkspaceId) -> Vec<SeedControl> {
    vec![
        SeedControl {
            id: control_id(workspace_id, "PP-AC-01"),
            code: "PP-AC-01",
            title: "Quarterly access review",
            description:
                "Review production system access quarterly and retain reviewer sign-off.",
            requirement_ids: vec![
                soc2_requirement_id("CC6.1"),
                soc2_requirement_id("CC6.2"),
            ],
        },
        SeedControl {
            id: control_id(workspace_id, "PP-VM-01"),
            code: "PP-VM-01",
            title: "Monthly vulnerability scanning",
            description:
                "Run vulnerability scans for production assets and track remediation of critical findings.",
            requirement_ids: vec![soc2_requirement_id("CC7.1")],
        },
        SeedControl {
            id: control_id(workspace_id, "PP-IR-01"),
            code: "PP-IR-01",
            title: "Incident response tabletop",
            description:
                "Exercise the incident response plan annually and track remediation actions.",
            requirement_ids: vec![soc2_requirement_id("CC7.4")],
        },
    ]
}

fn demo_policies(workspace_id: WorkspaceId) -> Vec<SeedPolicy> {
    vec![
        SeedPolicy {
            id: policy_id(workspace_id, "acceptable-use"),
            name: "Acceptable Use Policy",
            description: None,
            control_codes: vec![],
        },
        SeedPolicy {
            id: policy_id(workspace_id, "incident-response"),
            name: "Incident Response Policy",
            description: Some(
                "Defines how security incidents are reported, contained, investigated, and resolved.",
            ),
            control_codes: vec!["PP-IR-01"],
        },
        SeedPolicy {
            id: policy_id(workspace_id, "information-security"),
            name: "Information Security Policy",
            description: Some(
                "Establishes safeguards for access control, vulnerability management, and protection of company information.",
            ),
            control_codes: vec!["PP-AC-01", "PP-VM-01"],
        },
    ]
}

fn soc2_framework_id() -> Uuid {
    seed_uuid("framework:soc2")
}

fn soc2_requirement_id(code: &str) -> Uuid {
    seed_uuid(&format!("soc2:{code}"))
}

fn control_id(workspace_id: WorkspaceId, code: &str) -> Uuid {
    seed_uuid(&format!(
        "workspace:{}:control:{code}",
        Uuid::from(workspace_id)
    ))
}

fn policy_id(workspace_id: WorkspaceId, slug: &str) -> Uuid {
    seed_uuid(&format!(
        "workspace:{}:policy:{slug}",
        Uuid::from(workspace_id)
    ))
}

fn demo_submission_id() -> Uuid {
    seed_uuid("demo:evidence-submission:quarterly-access-review")
}

fn demo_document_id() -> Uuid {
    seed_uuid("demo:evidence-document:quarterly-access-review")
}

fn demo_document_object_key() -> Result<ObjectKey, StorageError> {
    ObjectKey::parse(format!(
        "workspaces/{}/evidence-submissions/{}/documents/{}/{}",
        Uuid::from(local_authorized_workspace_id()),
        demo_submission_id(),
        demo_document_id(),
        DEMO_SUBMISSION_FILENAME
    ))
}

fn demo_document_crc32c_base64() -> String {
    BASE64_STANDARD.encode(crc32c::crc32c(DEMO_SUBMISSION_BYTES).to_be_bytes())
}

fn seed_uuid(name: &str) -> Uuid {
    Uuid::new_v5(&seed_namespace(), name.as_bytes())
}

fn seed_namespace() -> Uuid {
    Uuid::parse_str("60dcb0ee-3c16-4767-bdda-25cb3bfaf300").unwrap()
}

fn local_workspace_id() -> WorkspaceId {
    local_authorized_workspace_id()
}

fn local_authorized_workspace_id() -> WorkspaceId {
    WorkspaceId::from(Uuid::parse_str("00000000-0000-4000-8000-000000000001").unwrap())
}

fn local_unauthorized_workspace_id() -> WorkspaceId {
    WorkspaceId::from(Uuid::parse_str("00000000-0000-4000-8000-000000000002").unwrap())
}

fn local_agent_connection_id() -> AgentConnectionId {
    AgentConnectionId::from(Uuid::parse_str(LOCAL_AGENT_CONNECTION_ID).unwrap())
}

fn timestamp(value: &str) -> Result<DateTime<Utc>, chrono::ParseError> {
    DateTime::parse_from_rfc3339(value).map(|datetime| datetime.with_timezone(&Utc))
}

#[cfg(test)]
mod tests {
    use super::{ConfigError, Error};

    #[test]
    fn configuration_error_includes_its_cause() {
        let error = Error::Config(Box::new(ConfigError::MissingEnv("PROOFPLANE_CONFIG")));

        assert_eq!(
            error.to_string(),
            "configuration error: environment variable PROOFPLANE_CONFIG is required"
        );
    }
}
