use proofplane::{config, migrations, observability, store, VERSION};
use secrecy::ExposeSecret;
use thiserror::Error;
use tokio_postgres::Client;
use tracing::{debug, error, info};

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

    debug!("seeding local data");
    seed_local_data(&client).await?;
    debug!("done seeding local data");

    info!(
        binary = "seed",
        version = VERSION,
        "{}",
        migrations::startup_message()
    );
    info!(
        binary = "seed",
        version = VERSION,
        "proofplane seed scaffold ready"
    );

    Ok(())
}

// TODO: Update this to use real repository interface instead of raw SQL.
async fn seed_local_data(client: &Client) -> Result<(), tokio_postgres::Error> {
    client
        .batch_execute(
            r#"
INSERT INTO workspaces (id, name)
VALUES ('local-workspace', 'Local Workspace')
ON CONFLICT (id) DO NOTHING;

INSERT INTO actors (id, workspace_id, actor_type, display_name)
VALUES ('system-actor', 'local-workspace', 'system', 'System')
ON CONFLICT (id) DO NOTHING;

INSERT INTO api_credentials (id, actor_id, name, credential_hash)
VALUES ('local-api-key', 'system-actor', 'Local API Key', 'local-development-credential-hash')
ON CONFLICT (id) DO NOTHING;
"#,
        )
        .await
}
