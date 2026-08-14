use std::{env, time::Duration as StdDuration};

use proofplane::{
    config,
    dequeuer::{self, OutboxDequeuer, OutboxDequeuerConfig},
    observability,
    persistence::{self, Postgres},
    pubsub::{self, ensure_worker_subscription, GoogleCloudPublisher},
    VERSION,
};
use secrecy::ExposeSecret;
use thiserror::Error;
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
    DatabaseConnection(#[from] persistence::connection::Error),

    #[error("database migration error")]
    Migrations(#[from] refinery::Error),

    #[error("outbox dequeuer error")]
    Dequeuer(#[from] dequeuer::OutboxDequeuerError),

    #[error("pubsub error")]
    PubSub(#[from] pubsub::PubSubError),

    #[error(
        "environment variable PUBSUB_EMULATOR_HOST is required for dequeuer Pub/Sub publishing"
    )]
    MissingPubSubEmulatorHost,
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

    let mut client = persistence::conn(config.database.url.expose_secret()).await?;

    debug!("running migrations");
    persistence::migrate(&mut client).await?;
    debug!("done running migrations");
    drop(client);

    let pool = persistence::conn_pool(
        config.database.url.expose_secret(),
        persistence::PoolBounds::from_config(
            &config.database.pool,
            persistence::PoolRuntime::Dequeuer,
        ),
    )
    .await?;
    let postgres = Postgres::new(pool);
    if env::var_os(pubsub::PUBSUB_EMULATOR_HOST).is_none() {
        // TODO: when we support GCP pubsub, we should log a warning when the emulator variable is set
        return Err(Error::MissingPubSubEmulatorHost);
    }

    ensure_worker_subscription(&config.pubsub.project_id, &config.pubsub.subscriptions).await?;

    let publisher = GoogleCloudPublisher::new(config.pubsub.project_id).await?;
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
