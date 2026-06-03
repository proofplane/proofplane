use std::time::Duration as StdDuration;

use proofplane::{
    config,
    dequeuer::{self, OutboxDequeuer, OutboxDequeuerConfig},
    observability,
    pubsub::UnavailablePublisher,
    repository::Postgres,
    store, VERSION,
};
use secrecy::ExposeSecret;
use thiserror::Error;
use tracing::{debug, error, info};

const POSTGRES_POOL_SIZE: usize = 2;

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

    #[error("outbox dequeuer error")]
    Dequeuer(#[from] dequeuer::OutboxDequeuerError),
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
    drop(client);

    let pool = store::conn_pool(config.postgres.expose_secret(), POSTGRES_POOL_SIZE).await?;
    let postgres = Postgres::new(pool);
    let publisher = UnavailablePublisher;
    let dequeuer_config = OutboxDequeuerConfig {
        max_attempts: config.worker.retry_attempts.into(),
        poll_interval: StdDuration::from_secs(1),
        ..OutboxDequeuerConfig::default()
    };
    let dequeuer = OutboxDequeuer::new(&postgres, &publisher, dequeuer_config);

    info!(
        binary = "dequeuer",
        version = VERSION,
        "{}",
        dequeuer::startup_message()
    );

    dequeuer
        .run_until_cancelled(async {
            if let Err(error) = tokio::signal::ctrl_c().await {
                error!(%error, "failed to listen for shutdown signal");
            }
        })
        .await?;

    info!(binary = "dequeuer", "shutdown signal received");

    Ok(())
}
