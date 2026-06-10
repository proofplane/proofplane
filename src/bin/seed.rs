use api_keys_simplified::{Environment, ExposeSecret};
use chrono::{DateTime, Utc};
use proofplane::{
    authentication::ApiKeyManager,
    authorization::spicedb::{ClientError, SpiceDbClient},
    config::{load_from_env, SpiceDbConfig},
    domain::{
        ActorId, ActorKind, CreateActorPayload, CreateApiCredentialPayload,
        CreateEvidenceRequestPayload, CreateWorkspacePayload, EvidenceRequestCadence,
        EvidenceRequestStatus, ProvisionUserPayload, UpdateActorPayload,
        UpdateApiCredentialPayload, UpdateEvidenceRequestPayload, UpdateWorkspacePayload,
        WorkspaceId, WorkspaceRole,
    },
    observability,
    repository::{NewWorkspaceMembership, Postgres},
    store, VERSION,
};
use thiserror::Error;
use tracing::debug;
use uuid::Uuid;

#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        eprintln!("{e}");
        std::process::exit(1);
    }
}

#[derive(Debug, Error)]
enum Error {
    #[error("postgres connection error")]
    StoreConnection(#[from] store::conn::Error),

    #[error("database migration error")]
    Migrations(#[from] refinery::Error),

    #[error("repository error")]
    Repository(#[from] proofplane::repository::Error),

    #[error("connection pool error")]
    Pool(#[from] deadpool_postgres::PoolError),

    #[error("database error")]
    Database(#[from] tokio_postgres::Error),

    #[error("seed timestamp parse error")]
    Timestamp(#[from] chrono::ParseError),

    #[error("SpiceDB error")]
    SpiceDb(#[from] ClientError),

    #[error("API key generation error")]
    ApiKey(#[from] proofplane::authentication::Error),
}

const LOCAL_HUMAN_USER_ACTOR_ID: &str = "00000000-0000-4000-8000-000000000101";
const LOCAL_AI_AGENT_ACTOR_ID: &str = "00000000-0000-4000-8000-000000000102";
const LOCAL_SERVICE_ACCOUNT_ACTOR_ID: &str = "00000000-0000-4000-8000-000000000103";
const LOCAL_INTEGRATION_ACTOR_ID: &str = "00000000-0000-4000-8000-000000000104";
const LOCAL_POLICY_AUTOMATION_ACTOR_ID: &str = "00000000-0000-4000-8000-000000000105";
const SYSTEM_ACTOR_ID: &str = "00000000-0000-4000-8000-000000000106";
const LOCAL_OWNER_AUTH0_SUB: &str = "auth0|local-owner";

async fn run() -> Result<(), Error> {
    let config = match load_from_env() {
        Ok(config) => config,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    };

    if let Err(error) = observability::init_cli_tracing(&config.observability) {
        eprintln!("{error}");
        std::process::exit(1);
    }

    let mut client = store::conn(config.postgres.expose_secret()).await?;

    debug!("running migrations");
    store::migrate(&mut client).await?;
    debug!("done running migrations");

    let pool = store::conn_pool(config.postgres.expose_secret(), 4).await?;
    let postgres = Postgres::new(pool);

    debug!("seeding local data");
    let api_key = seed_local_data(&postgres).await?;
    seed_local_membership(&config.spicedb).await?;
    seed_local_owner(&postgres, &config.spicedb).await?;
    debug!("done seeding local data");

    println!("Proofplane {VERSION} local seed complete");
    println!(
        "Seeded local workspaces, actors, API credential, authorized SpiceDB membership, demo evidence requests, and SOC 2 controls"
    );
    println!(
        "authorized workspace: {}",
        Uuid::from(local_authorized_workspace_id())
    );
    println!(
        "unauthorized workspace: {}",
        Uuid::from(local_unauthorized_workspace_id())
    );
    println!("local system actor API key (rotated by this seed run): {api_key}");

    Ok(())
}

async fn seed_local_data(repository: &Postgres) -> Result<String, Error> {
    seed_workspace(repository).await?;
    seed_actors(repository).await?;
    let api_key = seed_api_credential(repository).await?;

    seed_evidence_requests(repository).await?;
    seed_frameworks_and_controls(repository).await?;

    Ok(api_key)
}

async fn seed_workspace(repository: &Postgres) -> Result<(), Error> {
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
        if repository.get_workspace(id).await?.is_some() {
            repository
                .update_workspace(id, &UpdateWorkspacePayload { slug, name })
                .await?;

            continue;
        }

        repository
            .create_workspace(&CreateWorkspacePayload {
                id: Some(id),
                slug,
                name,
            })
            .await?;
    }

    Ok(())
}

async fn seed_actors(repository: &Postgres) -> Result<(), Error> {
    for (id, kind, display_name) in [
        (
            actor_id(LOCAL_HUMAN_USER_ACTOR_ID),
            ActorKind::HumanUser,
            "Local Human User",
        ),
        (
            actor_id(LOCAL_AI_AGENT_ACTOR_ID),
            ActorKind::AiAgent,
            "Local AI Agent",
        ),
        (
            actor_id(LOCAL_SERVICE_ACCOUNT_ACTOR_ID),
            ActorKind::ServiceAccount,
            "Local Service Account",
        ),
        (
            actor_id(LOCAL_INTEGRATION_ACTOR_ID),
            ActorKind::Integration,
            "Local Integration",
        ),
        (
            actor_id(LOCAL_POLICY_AUTOMATION_ACTOR_ID),
            ActorKind::PolicyAutomation,
            "Local Policy Automation",
        ),
        (actor_id(SYSTEM_ACTOR_ID), ActorKind::System, "System"),
    ] {
        if repository.get_actor(id).await?.is_some() {
            repository
                .update_actor(
                    id,
                    &UpdateActorPayload {
                        kind,
                        display_name: display_name.to_owned(),
                    },
                )
                .await?;

            continue;
        }

        repository
            .create_actor(&CreateActorPayload {
                id: Some(id),
                kind,
                display_name: display_name.to_owned(),
            })
            .await?;
    }

    Ok(())
}

async fn seed_api_credential(repository: &Postgres) -> Result<String, Error> {
    let id = "local-api-key";
    let actor_id = actor_id(SYSTEM_ACTOR_ID);
    let name = "Local API Key".to_owned();
    let issued = ApiKeyManager::new()?.issue(Environment::dev())?;

    if repository.get_api_credential(id).await?.is_some() {
        repository
            .update_api_credential(
                id,
                &UpdateApiCredentialPayload {
                    name: name.clone(),
                    key_id: issued.key_id.clone(),
                    credential_hash: issued.credential_hash.clone(),
                    expires_at: None,
                    revoked_at: None,
                },
            )
            .await?;
    } else {
        repository
            .create_api_credential(&CreateApiCredentialPayload {
                id: id.to_owned(),
                actor_id,
                name,
                key_id: issued.key_id.clone(),
                credential_hash: issued.credential_hash.clone(),
                expires_at: None,
                revoked_at: None,
            })
            .await?;
    }

    Ok(issued.raw_key.expose_secret().to_owned())
}

async fn seed_local_membership(config: &SpiceDbConfig) -> Result<(), Error> {
    let client = SpiceDbClient::from_config(config).await?;
    client
        .write_workspace_membership(local_authorized_workspace_id(), SYSTEM_ACTOR_ID)
        .await?;

    Ok(())
}

async fn seed_local_owner(repository: &Postgres, config: &SpiceDbConfig) -> Result<(), Error> {
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
            .in_transaction(async move |context| {
                context
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

    let client = SpiceDbClient::from_config(config).await?;
    client
        .write_workspace_user_role(
            workspace_id,
            &Uuid::from(user.id).to_string(),
            WorkspaceRole::Owner,
        )
        .await?;

    Ok(())
}

async fn seed_evidence_requests(repository: &Postgres) -> Result<(), Error> {
    let workspace_id = local_workspace_id();
    let actor_id = actor_id(SYSTEM_ACTOR_ID);
    let seeds = demo_evidence_requests()?;
    let existing = repository
        .in_actor_context_read(workspace_id, actor_id, async |context| {
            context.list_evidence_requests().await
        })
        .await?;

    repository
        .in_actor_context(workspace_id, actor_id, async move |context| {
            for seed in seeds {
                if let Some(existing_request) =
                    existing.iter().find(|request| request.title == seed.title)
                {
                    let update = seed.clone().into_update();
                    context
                        .replace_evidence_request(existing_request.id, &update)
                        .await?;
                } else {
                    let request = seed.into_new();
                    context.create_evidence_request(&request).await?;
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

#[derive(Clone)]
struct SeedEvidenceRequest {
    title: String,
    description: String,
    collection_instructions: String,
    cadence: EvidenceRequestCadence,
    due_at: DateTime<Utc>,
    schedule_anchor_at: DateTime<Utc>,
    freshness_window_days: Option<i32>,
    status: EvidenceRequestStatus,
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

impl SeedEvidenceRequest {
    fn into_new(self) -> CreateEvidenceRequestPayload {
        CreateEvidenceRequestPayload {
            title: self.title,
            description: self.description,
            collection_instructions: self.collection_instructions,
            cadence: self.cadence,
            due_at: self.due_at,
            schedule_anchor_at: self.schedule_anchor_at,
            freshness_window_days: self.freshness_window_days,
            status: self.status,
        }
    }

    fn into_update(self) -> UpdateEvidenceRequestPayload {
        UpdateEvidenceRequestPayload {
            title: self.title,
            description: self.description,
            collection_instructions: self.collection_instructions,
            cadence: self.cadence,
            due_at: self.due_at,
            schedule_anchor_at: self.schedule_anchor_at,
            freshness_window_days: self.freshness_window_days,
            status: self.status,
        }
    }
}

fn demo_evidence_requests() -> Result<Vec<SeedEvidenceRequest>, Error> {
    Ok(vec![
        SeedEvidenceRequest {
            title: "Quarterly access review".to_owned(),
            description: "Confirm user access reviews are completed for production systems."
                .to_owned(),
            collection_instructions:
                "Export the completed access review report from the identity provider and include reviewer sign-off."
                    .to_owned(),
            cadence: EvidenceRequestCadence::Quarterly,
            due_at: timestamp("2026-06-30T17:00:00Z")?,
            schedule_anchor_at: timestamp("2026-03-31T17:00:00Z")?,
            freshness_window_days: Some(90),
            status: EvidenceRequestStatus::Active,
        },
        SeedEvidenceRequest {
            title: "Monthly vulnerability scan".to_owned(),
            description:
                "Confirm vulnerability scans are performed for the production environment."
                    .to_owned(),
            collection_instructions:
                "Attach the monthly scanner summary showing scan scope, date, and critical findings."
                    .to_owned(),
            cadence: EvidenceRequestCadence::Monthly,
            due_at: timestamp("2026-05-31T17:00:00Z")?,
            schedule_anchor_at: timestamp("2026-05-01T17:00:00Z")?,
            freshness_window_days: Some(30),
            status: EvidenceRequestStatus::Active,
        },
        SeedEvidenceRequest {
            title: "Annual incident response tabletop".to_owned(),
            description: "Confirm the incident response tabletop exercise is completed annually."
                .to_owned(),
            collection_instructions:
                "Attach the exercise agenda, participant list, findings, and remediation actions."
                    .to_owned(),
            cadence: EvidenceRequestCadence::Annually,
            due_at: timestamp("2026-12-15T17:00:00Z")?,
            schedule_anchor_at: timestamp("2026-01-15T17:00:00Z")?,
            freshness_window_days: Some(365),
            status: EvidenceRequestStatus::Paused,
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

fn actor_id(value: &str) -> ActorId {
    ActorId::from(Uuid::parse_str(value).expect("seed actor ID is a UUID"))
}

fn timestamp(value: &str) -> Result<DateTime<Utc>, chrono::ParseError> {
    DateTime::parse_from_rfc3339(value).map(|datetime| datetime.with_timezone(&Utc))
}
