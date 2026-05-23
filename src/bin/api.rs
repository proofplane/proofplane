use std::sync::Arc;
use tokio::net::TcpListener;

use metrics_exporter_prometheus::{BuildError, PrometheusBuilder};
use proofplane::{
    app::{create_app, AppDependencies},
    authentication::{ApiKeyAuthenticator, ApiKeyManager},
    authorization::{
        evidence_requests::EvidenceRequestAuthorizer,
        spicedb::{ClientError as SpiceDbClientError, SpiceDbClient},
    },
    config, observability, repository, store,
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
    StoreConnection(#[from] store::conn::Error),

    #[error("database migration error")]
    Migrations(#[from] refinery::Error),

    #[error("prometheus initialization error")]
    PrometheusInit(#[from] BuildError),

    #[error("authentication initialization error")]
    Authentication(#[from] proofplane::authentication::Error),

    #[error("SpiceDB client initialization error")]
    SpiceDb(#[from] SpiceDbClientError),
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

    // TODO: move the Postgres pool size into configuration.
    let pool = store::conn_pool(config.postgres.expose_secret(), 200).await?;
    let postgres = Arc::new(repository::Postgres::new(pool));

    let metrics = PrometheusBuilder::new().install_recorder()?;

    let listener = TcpListener::bind(config.server.api_bind).await.unwrap();
    info!("listening on {}", config.server.api_bind);

    let authenticator = ApiKeyAuthenticator::new(ApiKeyManager::new()?, postgres.clone());
    let evidence_request_authorizer =
        EvidenceRequestAuthorizer::new(SpiceDbClient::from_config(&config.spicedb).await?);

    let deps = AppDependencies {
        config,
        postgres,
        metrics,
        authenticator,
        evidence_request_authorizer,
    };

    let app = create_app(deps)?;

    axum::serve(listener, app.into_make_service())
        .with_graceful_shutdown(async move {
            tokio::signal::ctrl_c()
                .await
                .expect("failed to listen for ctrl-c");

            info!("received shutdown signal")
        })
        .await
        .unwrap();

    info!("server shutdown complete");

    Ok(())
}
