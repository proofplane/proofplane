use std::sync::Arc;
use tokio::net::TcpListener;

use metrics_exporter_prometheus::{BuildError, PrometheusBuilder};
use proofplane::{
    app::{create_app, AppDependencies, CreateAppError},
    authentication::{auth0::Auth0TokenVerifier, UserAuthenticator},
    config,
    object_storage::{self, DocumentObjectStores},
    observability, persistence,
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

    #[error("prometheus initialization error")]
    PrometheusInit(#[from] BuildError),

    #[error("application initialization error")]
    App(#[from] CreateAppError),

    #[error("object storage initialization error")]
    ObjectStorage(#[from] object_storage::StorageError),

    #[error("I/O error")]
    Io(#[from] std::io::Error),
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
        config.database.tls,
        persistence::PoolBounds::from_config(&config.database.pool, persistence::PoolRuntime::Api),
    )
    .await?;
    let client = pool.get().await?;
    persistence::check_schema_revision(&client).await?;
    drop(client);
    let postgres = Arc::new(persistence::Postgres::new(pool));
    let object_stores = DocumentObjectStores::from_config(&config.object_storage).await?;

    let metrics = PrometheusBuilder::new().install_recorder()?;

    let listener = TcpListener::bind(config.server.api_bind).await?;
    info!("listening on {}", config.server.api_bind);

    let user_authenticator = UserAuthenticator::new(
        Arc::new(Auth0TokenVerifier::new(&config.auth0)),
        postgres.clone(),
    );

    let deps = AppDependencies {
        config,
        postgres,
        quarantine_store: object_stores.quarantine,
        evidence_store: object_stores.evidence,
        metrics,
        user_authenticator,
        auditor_identity_provider: None,
    };

    let app = create_app(deps)?;

    axum::serve(listener, app.into_make_service())
        .with_graceful_shutdown(async move {
            match tokio::signal::ctrl_c().await {
                Ok(()) => info!("received shutdown signal"),
                Err(error) => error!(%error, "failed to listen for ctrl-c"),
            }
        })
        .await?;

    info!("server shutdown complete");

    Ok(())
}
