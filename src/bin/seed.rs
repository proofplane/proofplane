use chrono::{DateTime, Utc};
use proofplane::{
    config,
    domain::{
        DomainError, EvidenceRequestCadence, EvidenceRequestStatus, EvidenceRequestUpdate,
        NewEvidenceRequest, WorkspaceId,
    },
    migrations, observability,
    repository::{EvidenceRequestRepository, Postgres},
    store, VERSION,
};
use secrecy::ExposeSecret;
use thiserror::Error;
use tokio_postgres::Client;
use tracing::{debug, error, info};
use uuid::Uuid;

#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        error!("{}", e);
        std::process::exit(1);
    }
}

#[derive(Debug, Error)]
enum Error {
    #[error("postgres connection error")]
    StoreConnection(#[from] store::conn::Error),

    #[error("database migration error")]
    Migrations(#[from] refinery::Error),

    #[error("seed data error")]
    SeedData(#[from] tokio_postgres::Error),

    #[error("repository error")]
    Repository(#[from] proofplane::repository::Error),

    #[error("seed validation error")]
    SeedValidation(Vec<DomainError>),

    #[error("seed timestamp parse error")]
    Timestamp(#[from] chrono::ParseError),
}

async fn run() -> Result<(), Error> {
    let config = match config::load_from_env() {
        Ok(config) => config,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    };

    if let Err(error) = observability::init_tracing(&config.observability) {
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
    seed_local_data(&client, &postgres).await?;
    debug!("done seeding local data");

    info!(
        binary = "seed",
        version = VERSION,
        "{}",
        migrations::startup_message()
    );

    Ok(())
}

async fn seed_local_data(
    client: &Client,
    evidence_requests: &impl EvidenceRequestRepository,
) -> Result<(), Error> {
    client
        .batch_execute(
            r#"
INSERT INTO workspaces (id, slug, name)
VALUES ('00000000-0000-4000-8000-000000000001', 'local-workspace', 'Local Workspace')
ON CONFLICT (id) DO NOTHING;

INSERT INTO actors (id, workspace_id, actor_type, display_name)
VALUES ('system-actor', '00000000-0000-4000-8000-000000000001', 'system', 'System')
ON CONFLICT (id) DO NOTHING;

INSERT INTO api_credentials (id, actor_id, name, credential_hash)
VALUES ('local-api-key', 'system-actor', 'Local API Key', 'local-development-credential-hash')
ON CONFLICT (id) DO NOTHING;
"#,
        )
        .await?;

    seed_evidence_requests(evidence_requests).await
}

async fn seed_evidence_requests(repository: &impl EvidenceRequestRepository) -> Result<(), Error> {
    let workspace_id = local_workspace_id();
    let existing = repository
        .list_evidence_requests_by_workspace(&workspace_id)
        .await?;

    for seed in demo_evidence_requests()? {
        if let Some(existing_request) = existing.iter().find(|request| request.title == seed.title)
        {
            let update = seed.clone().into_update().validate().into_result()?;
            repository
                .replace_evidence_request(existing_request.id, &update)
                .await?;
        } else {
            let request = seed.into_new(workspace_id).validate().into_result()?;
            repository.create_evidence_request(&request).await?;
        }
    }

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
    fn into_new(self, workspace_id: WorkspaceId) -> NewEvidenceRequest {
        NewEvidenceRequest {
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

    fn into_update(self) -> EvidenceRequestUpdate {
        EvidenceRequestUpdate {
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

impl From<Vec<DomainError>> for Error {
    fn from(errors: Vec<DomainError>) -> Self {
        Self::SeedValidation(errors)
    }
}
