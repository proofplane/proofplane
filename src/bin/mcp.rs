use std::{future::Future, sync::Arc, time::Duration};

use axum::Router;
use metrics_exporter_prometheus::{BuildError, PrometheusBuilder};
use proofplane::{
    authentication::paseto::{
        DownloadGrantDecryptor, DownloadGrantEncryptor, McpOAuthDecryptor, McpOAuthEncryptor,
        UploadGrantDecryptor, UploadGrantEncryptor,
    },
    config,
    mcp::{self, McpAppDependencies},
    object_storage, observability, repository,
    services::oauth::OAuthService,
    store,
};
use secrecy::ExposeSecret;
use thiserror::Error;
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info};

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        error!(%error, "MCP server failed");
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
    #[error("object storage initialization error")]
    ObjectStorage(#[from] object_storage::StorageError),
    #[error("download grant token initialization error")]
    DownloadGrantToken(#[from] proofplane::authentication::paseto::Error),
    #[error("MCP listener error")]
    Listener(#[from] std::io::Error),
    #[error("MCP application initialization error")]
    App(#[from] proofplane::mcp::McpAppError),
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

    let pool = store::conn_pool(config.postgres.expose_secret(), 200).await?;
    let postgres = Arc::new(repository::Postgres::new(pool));
    let object_store = Arc::new(object_storage::from_config(&config.object_storage).await?);
    let download_grant_encryptor = DownloadGrantEncryptor::from_config(
        config.server.public_api_base_url.clone(),
        "proofplane-attachment-download",
        &config.paseto.download,
    )?;
    let download_grant_decryptor = DownloadGrantDecryptor::from_config(
        config.server.public_api_base_url.clone(),
        "proofplane-attachment-download",
        &config.paseto.download,
    )?;
    let upload_grant_encryptor = UploadGrantEncryptor::from_config(
        config.server.public_api_base_url.clone(),
        "proofplane-attachment-upload-grant",
        &config.paseto.upload_grant,
    )?;
    let upload_grant_decryptor = UploadGrantDecryptor::from_config(
        config.server.public_api_base_url.clone(),
        "proofplane-attachment-upload-grant",
        &config.paseto.upload_grant,
    )?;
    let mcp_oauth_encryptor = McpOAuthEncryptor::from_config(
        config.server.public_api_base_url.clone(),
        config.mcp.resource.to_string(),
        &config.paseto.mcp_oauth,
    )?;
    let mcp_oauth_decryptor = McpOAuthDecryptor::from_config(
        config.server.public_api_base_url.clone(),
        config.mcp.resource.to_string(),
        &config.paseto.mcp_oauth,
    )?;
    let oauth_service = OAuthService::new(
        postgres.clone(),
        config.server.public_api_base_url.clone(),
        config.mcp.resource.clone(),
        proofplane::services::cimd::CimdResolver::new(),
        mcp_oauth_encryptor,
        mcp_oauth_decryptor,
    );
    let metrics = PrometheusBuilder::new().install_recorder()?;
    let sessions = CancellationToken::new();
    let app = mcp::create_app(McpAppDependencies {
        postgres,
        object_store,
        metrics,
        oauth_verifier: Arc::new(oauth_service),
        authorization_server: config.server.public_api_base_url.clone(),
        resource: config.mcp.resource.clone(),
        public_api_base_url: config.server.public_api_base_url.clone(),
        download_grant_encryptor,
        download_grant_decryptor,
        upload_grant_encryptor,
        upload_grant_decryptor,
        health: config.health.clone(),
        allowed_hosts: config.mcp.allowed_hosts.clone(),
        cancellation_token: sessions.clone(),
    })?;

    // All fallible dependency construction is complete before the socket accepts traffic.
    let listener = TcpListener::bind(config.server.mcp_bind).await?;
    info!(address = %config.server.mcp_bind, "MCP server listening");

    serve_until_shutdown(
        listener,
        app,
        async {
            match tokio::signal::ctrl_c().await {
                Ok(()) => info!("received shutdown signal"),
                Err(error) => error!(%error, "failed to listen for ctrl-c"),
            }
        },
        Duration::from_secs(config.mcp.shutdown_grace_seconds),
        sessions,
    )
    .await?;

    info!("MCP server shutdown complete");
    Ok(())
}

async fn serve_until_shutdown(
    listener: tokio::net::TcpListener,
    app: Router,
    shutdown: impl Future<Output = ()> + Send + 'static,
    grace: Duration,
    sessions: CancellationToken,
) -> std::io::Result<()> {
    let stop_accepting = CancellationToken::new();
    let mut server = tokio::spawn({
        let stop_accepting = stop_accepting.clone();
        async move {
            axum::serve(listener, app.into_make_service())
                .with_graceful_shutdown(stop_accepting.cancelled_owned())
                .await
        }
    });

    shutdown.await;
    stop_accepting.cancel();

    match tokio::time::timeout(grace, &mut server).await {
        Ok(result) => {
            result.map_err(std::io::Error::other)??;
        }
        Err(_) => {
            sessions.cancel();
            if tokio::time::timeout(Duration::from_millis(100), &mut server)
                .await
                .is_err()
            {
                server.abort();
                let _ = server.await;
            }
            return Ok(());
        }
    }

    sessions.cancel();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::routing::get;
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };
    use tokio::sync::{oneshot, Notify};

    #[tokio::test]
    async fn graceful_shutdown_drains_a_request_within_the_deadline() {
        let started = Arc::new(Notify::new());
        let app = Router::new().route(
            "/slow",
            get({
                let started = started.clone();
                move || {
                    let started = started.clone();
                    async move {
                        started.notify_one();
                        tokio::time::sleep(Duration::from_millis(25)).await;
                        "complete"
                    }
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("listener binds");
        let address = listener.local_addr().expect("listener has an address");
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let sessions = CancellationToken::new();
        let serving = tokio::spawn(serve_until_shutdown(
            listener,
            app,
            async move {
                let _ = shutdown_rx.await;
            },
            Duration::from_secs(1),
            sessions.clone(),
        ));
        let request = tokio::spawn(reqwest::get(format!("http://{address}/slow")));
        started.notified().await;
        shutdown_tx.send(()).expect("shutdown sends");

        let response = request
            .await
            .expect("request task joins")
            .expect("request completes");
        assert_eq!(response.text().await.expect("body reads"), "complete");
        serving
            .await
            .expect("server task joins")
            .expect("server stops");
        assert!(sessions.is_cancelled());
    }

    #[tokio::test]
    async fn graceful_shutdown_cancels_a_request_after_the_deadline() {
        struct CancellationGuard(Arc<AtomicBool>);
        impl Drop for CancellationGuard {
            fn drop(&mut self) {
                self.0.store(true, Ordering::SeqCst);
            }
        }

        let started = Arc::new(Notify::new());
        let cancelled = Arc::new(AtomicBool::new(false));
        let sessions = CancellationToken::new();
        let app = Router::new().route(
            "/blocked",
            get({
                let started = started.clone();
                let cancelled = cancelled.clone();
                let sessions = sessions.clone();
                move || {
                    let started = started.clone();
                    let cancelled = cancelled.clone();
                    let sessions = sessions.clone();
                    async move {
                        let _guard = CancellationGuard(cancelled);
                        started.notify_one();
                        sessions.cancelled().await;
                        "cancelled"
                    }
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("listener binds");
        let address = listener.local_addr().expect("listener has an address");
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let serving = tokio::spawn(serve_until_shutdown(
            listener,
            app,
            async move {
                let _ = shutdown_rx.await;
            },
            Duration::from_millis(20),
            sessions.clone(),
        ));
        let request = tokio::spawn(reqwest::get(format!("http://{address}/blocked")));
        started.notified().await;
        let shutdown_started = std::time::Instant::now();
        shutdown_tx.send(()).expect("shutdown sends");

        serving
            .await
            .expect("server task joins")
            .expect("server stops");
        assert!(shutdown_started.elapsed() < Duration::from_secs(1));
        assert!(sessions.is_cancelled());
        assert!(cancelled.load(Ordering::SeqCst));
        request.abort();
    }
}
