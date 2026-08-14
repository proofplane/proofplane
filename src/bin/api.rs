use std::sync::Arc;
use tokio::net::TcpListener;

use metrics_exporter_prometheus::{BuildError, PrometheusBuilder};
use proofplane::{
    app::{create_app, AppDependencies, CreateAppError},
    authentication::{auth0::Auth0TokenVerifier, UserAuthenticator},
    config, object_storage, observability, persistence,
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
    persistence::apply_migrations(&mut client).await?;
    debug!("done running migrations");
    // Close the client connection so that it doesn't add an extra connection outside
    // the number of connections we want as specified in the configuration.
    drop(client);

    let pool = persistence::conn_pool(
        config.database.url.expose_secret(),
        persistence::PoolBounds::from_config(&config.database.pool, persistence::PoolRuntime::Api),
    )
    .await?;
    let postgres = Arc::new(persistence::Postgres::new(pool));
    let object_store = Arc::new(object_storage::from_config(&config.object_storage).await?);

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
        object_store,
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
