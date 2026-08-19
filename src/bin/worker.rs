use std::{sync::Arc, time::Duration};

use metrics_exporter_prometheus::{BuildError, PrometheusBuilder};
use proofplane::{
    config,
    object_storage::{self, RuntimeObjectStore},
    observability,
    persistence::{self, PoolBounds, PoolRuntime, Postgres},
    scanner::ClamAvMalwareScanner,
    worker::{create_worker_app, WorkerAppDependencies},
    VERSION,
};
use secrecy::ExposeSecret;
use thiserror::Error;
use tokio::net::TcpListener;
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

    #[error("prometheus initialization error")]
    PrometheusInit(#[from] BuildError),

    #[error("object storage initialization error")]
    ObjectStorage(#[from] object_storage::StorageError),

    #[error("worker server I/O error")]
    Server(#[from] std::io::Error),
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
        PoolBounds::from_config(&config.database.pool, PoolRuntime::Worker),
    )
    .await?;
    let client = pool.get().await?;
    persistence::check_schema_revision(&client).await?;
    drop(client);
    let postgres = Arc::new(Postgres::new(pool));
    let object_store = Arc::new(RuntimeObjectStore::from_config(&config.object_storage).await?);
    let scanner = Arc::new(ClamAvMalwareScanner::new(
        config.scanner.clamd_address,
        Duration::from_millis(config.scanner.connection_timeout_ms),
        Duration::from_millis(config.scanner.scan_timeout_ms),
    ));
    let metrics = PrometheusBuilder::new().install_recorder()?;

    let listener = TcpListener::bind(config.server.worker_bind).await?;
    info!(
        binary = "worker",
        version = VERSION,
        "listening on {}",
        config.server.worker_bind
    );

    let app = create_worker_app(WorkerAppDependencies {
        postgres,
        object_store,
        scanner,
        worker_max_delivery_attempts: config.pubsub.subscriptions.worker_max_delivery_attempts,
        metrics,
        live_path: config.health.live_path,
        ready_path: config.health.ready_path,
        dependency_timeout_ms: config.health.dependency_timeout_ms,
    });

    axum::serve(listener, app.into_make_service())
        .with_graceful_shutdown(async move {
            if let Err(error) = tokio::signal::ctrl_c().await {
                error!(%error, "failed to listen for ctrl-c; shutting down");
            }

            info!("received shutdown signal")
        })
        .await?;

    info!(binary = "worker", "server shutdown complete");

    Ok(())
}
