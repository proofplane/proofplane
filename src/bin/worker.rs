use std::{sync::Arc, time::Duration};

use metrics_exporter_prometheus::{BuildError, PrometheusBuilder};
use proofplane::{
    config::{self, MailBackendConfig},
    mail::{LocalMailAdapter, MailAdapter, ResendMailAdapter},
    object_storage, observability, persistence,
    scanner::ClamAvMalwareScanner,
    services::workspace_invitation_authority::{
        WorkspaceInvitationAuthority, WorkspaceInvitationAuthorityError,
    },
    worker::{create_worker_app, WorkerAppDependencies},
    VERSION,
};
use secrecy::ExposeSecret;
use thiserror::Error;
use tokio::net::TcpListener;
use tracing::{debug, error, info};

const POSTGRES_POOL_SIZE: usize = 20;

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

    #[error("object storage initialization error")]
    ObjectStorage(#[from] object_storage::StorageError),

    #[error("workspace invitation authority initialization error")]
    WorkspaceInvitationAuthority(#[from] WorkspaceInvitationAuthorityError),

    #[error("worker server I/O error")]
    Server(#[from] std::io::Error),
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

    let mut client = persistence::conn(config.postgres.expose_secret()).await?;

    debug!("running migrations");
    persistence::migrate(&mut client).await?;
    debug!("done running migrations");
    drop(client);

    let pool = persistence::conn_pool(config.postgres.expose_secret(), POSTGRES_POOL_SIZE).await?;
    let postgres = Arc::new(persistence::Postgres::new(pool));
    let object_store = Arc::new(object_storage::from_config(&config.object_storage).await?);
    let scanner = Arc::new(ClamAvMalwareScanner::new(
        config.scanner.clamd_address,
        Duration::from_millis(config.scanner.connection_timeout_ms),
        Duration::from_millis(config.scanner.scan_timeout_ms),
    ));
    let metrics = PrometheusBuilder::new().install_recorder()?;
    let workspace_invitation_authority =
        WorkspaceInvitationAuthority::from_config(&config.workspace_invitations)?;
    let mail: Arc<dyn MailAdapter> = match config.mail.backend.clone() {
        MailBackendConfig::Local => Arc::new(LocalMailAdapter),
        MailBackendConfig::Resend { endpoint, api_key } => Arc::new(ResendMailAdapter::new(
            endpoint,
            api_key,
            config.mail.sender.clone(),
        )),
    };

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
        mail,
        workspace_invitation_authority,
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
