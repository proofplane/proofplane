use std::sync::Arc;
use tokio::net::TcpListener;

use metrics_exporter_prometheus::{BuildError, PrometheusBuilder};
use proofplane::{
    app::{create_app, AppDependencies},
    authentication::{
        auth0::Auth0TokenVerifier, paseto::ApiTokenVerifier, ApiTokenAuthenticator,
        UserAuthenticator,
    },
    config, object_storage, observability, repository, store,
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

    #[error("object storage initialization error")]
    ObjectStorage(#[from] object_storage::StorageError),
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
    let object_store = Arc::new(object_storage::from_config(&config.object_storage).await?);

    let metrics = PrometheusBuilder::new().install_recorder()?;

    let listener = TcpListener::bind(config.server.api_bind).await.unwrap();
    info!("listening on {}", config.server.api_bind);

    let api_token_verifier = ApiTokenVerifier::from_config(
        config.server.public_api_base_url.clone(),
        "proofplane-api",
        &config.paseto.api,
    )
    .map_err(proofplane::authentication::Error::Paseto)?;
    let api_token_authenticator = ApiTokenAuthenticator::new(api_token_verifier, postgres.clone());
    let user_authenticator = UserAuthenticator::new(
        Arc::new(Auth0TokenVerifier::new(&config.auth0)),
        postgres.clone(),
    );

    let deps = AppDependencies {
        config,
        postgres,
        object_store,
        metrics,
        api_token_authenticator,
        user_authenticator,
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
