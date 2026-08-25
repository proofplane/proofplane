use std::time::Duration as StdDuration;

use proofplane::{
    config,
    dequeuer::{self, OutboxDequeuer, OutboxDequeuerConfig},
    observability,
    persistence::{self, PoolBounds, PoolRuntime, Postgres},
    pubsub::{self, ClientMode, GoogleCloudPublisher},
    VERSION,
};
use secrecy::ExposeSecret;
use thiserror::Error;
use tracing::{error, info};

#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        error!("{:#}", anyhow::Error::from(e));
        std::process::exit(1);
    }
}

#[derive(Debug, Error)]
enum Error {
    #[error("postgres connection error")]
    DatabaseConnection(#[from] persistence::connection::Error),

    #[error("database pool error")]
    DatabasePool(#[from] deadpool_postgres::PoolError),

    #[error("database schema revision error")]
    SchemaRevision(#[from] persistence::SchemaRevisionError),

    #[error("outbox dequeuer error")]
    Dequeuer(#[from] dequeuer::OutboxDequeuerError),

    #[error("pubsub error")]
    PubSub(#[from] pubsub::PubSubError),
}

async fn run() -> Result<(), Error> {
    let config = match config::load_from_env() {
        Ok(config) => config,
        Err(error) => {
            eprintln!("{:#}", anyhow::Error::from(error));
            std::process::exit(1);
        }
    };

    if let Err(error) = observability::init_tracing(&config.observability) {
        eprintln!("{:#}", anyhow::Error::from(error));
        std::process::exit(1);
    }

    let pool = persistence::conn_pool(
        config.database.url.expose_secret(),
        &config.database.tls,
        PoolBounds::from_config(&config.database.pool, PoolRuntime::Dequeuer),
    )
    .await?;
    let client = pool.get().await?;
    persistence::check_schema_revision(&client).await?;
    drop(client);
    let postgres = Postgres::new(pool);

    // Logged before the client opens, so a credential or permission failure
    // arrives under the mode that produced it.
    let mode = ClientMode::from_env();
    info!(
        binary = "dequeuer",
        version = VERSION,
        pubsub_mode = mode.as_str(),
        pubsub_project_id = %config.pubsub.project_id,
        pubsub_emulator_host = mode.emulator_host().unwrap_or("none"),
        "pubsub client mode selected"
    );

    // The publisher opens before the first outbox poll, so a missing credential
    // stops the process with no row claimed. A denied publish cannot be seen
    // here: the dequeuer holds `pubsub.topics.publish` and no read permission,
    // so it appears on the first row as a retried publish failure.
    let publisher = GoogleCloudPublisher::new(&config.pubsub.project_id, &mode).await?;
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
