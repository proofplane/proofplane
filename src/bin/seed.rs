use api_keys_simplified::{Environment, ExposeSecret};
use chrono::{DateTime, Utc};
use proofplane::{
    authentication::ApiKeyManager,
    authorization::spicedb::{ClientError, SpiceDbClient},
    config::{load_from_env, SpiceDbConfig},
    domain::{
        ActorKind, CreateActorPayload, CreateApiCredentialPayload, CreateEvidenceRequestPayload,
        CreateWorkspacePayload, EvidenceRequestCadence, EvidenceRequestStatus, UpdateActorPayload,
        UpdateApiCredentialPayload, UpdateEvidenceRequestPayload, UpdateWorkspacePayload,
        WorkspaceId,
    },
    observability,
    repository::Postgres,
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

    #[error("seed timestamp parse error")]
    Timestamp(#[from] chrono::ParseError),

    #[error("SpiceDB error")]
    SpiceDb(#[from] ClientError),

    #[error("API key generation error")]
    ApiKey(#[from] proofplane::authentication::Error),
}

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
    debug!("done seeding local data");

    println!("Proofplane {VERSION} local seed complete");
    println!(
        "Seeded local workspace, actors, API credential, SpiceDB membership, and demo evidence requests"
    );
    println!("local system actor API key (rotated by this seed run): {api_key}");

    Ok(())
}

async fn seed_local_data(repository: &Postgres) -> Result<String, Error> {
    seed_workspace(repository).await?;
    seed_actors(repository).await?;
    let api_key = seed_api_credential(repository).await?;

    seed_evidence_requests(repository).await?;

    Ok(api_key)
}

async fn seed_workspace(repository: &Postgres) -> Result<(), Error> {
    let id = local_workspace_id();
    let slug = Some("local-workspace".to_owned());
    let name = "Local Workspace".to_owned();

    if repository.get_workspace(id).await?.is_some() {
        repository
            .update_workspace(id, &UpdateWorkspacePayload { slug, name })
            .await?;

        return Ok(());
    }

    repository
        .create_workspace(&CreateWorkspacePayload {
            id: Some(id),
            slug,
            name,
        })
        .await?;

    Ok(())
}

async fn seed_actors(repository: &Postgres) -> Result<(), Error> {
    for (id, kind, display_name) in [
        ("local-human-user", ActorKind::HumanUser, "Local Human User"),
        ("local-ai-agent", ActorKind::AiAgent, "Local AI Agent"),
        (
            "local-service-account",
            ActorKind::ServiceAccount,
            "Local Service Account",
        ),
        (
            "local-integration",
            ActorKind::Integration,
            "Local Integration",
        ),
        (
            "local-policy-automation",
            ActorKind::PolicyAutomation,
            "Local Policy Automation",
        ),
        ("system-actor", ActorKind::System, "System"),
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
                id: id.to_owned(),
                kind,
                display_name: display_name.to_owned(),
            })
            .await?;
    }

    Ok(())
}

async fn seed_api_credential(repository: &Postgres) -> Result<String, Error> {
    let id = "local-api-key";
    let actor_id = "system-actor".to_owned();
    let name = "Local API Key".to_owned();
    let issued = ApiKeyManager::new()?.issue(Environment::dev())?;

    if repository.get_api_credential(id).await?.is_some() {
        repository
            .update_api_credential(
                id,
                &UpdateApiCredentialPayload {
                    actor_id,
                    name,
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
        .write_workspace_membership(local_workspace_id(), "system-actor")
        .await?;

    Ok(())
}

async fn seed_evidence_requests(repository: &Postgres) -> Result<(), Error> {
    let workspace_id = local_workspace_id();
    let seeds = demo_evidence_requests()?;

    repository
        .in_workspace(workspace_id, async move |context| {
            let existing = context.list_evidence_requests().await?;

            for seed in seeds {
                if let Some(existing_request) =
                    existing.iter().find(|request| request.title == seed.title)
                {
                    let update = seed.clone().into_update();
                    context
                        .replace_evidence_request(existing_request.id, &update)
                        .await?;
                } else {
                    let request = seed.into_new(workspace_id);
                    context.create_evidence_request(&request).await?;
                }
            }

            Ok(())
        })
        .await?;

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

impl SeedEvidenceRequest {
    fn into_new(self, workspace_id: WorkspaceId) -> CreateEvidenceRequestPayload {
        CreateEvidenceRequestPayload {
            workspace_id,
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

fn local_workspace_id() -> WorkspaceId {
    WorkspaceId::from(Uuid::parse_str("00000000-0000-4000-8000-000000000001").unwrap())
}

fn timestamp(value: &str) -> Result<DateTime<Utc>, chrono::ParseError> {
    DateTime::parse_from_rfc3339(value).map(|datetime| datetime.with_timezone(&Utc))
}
