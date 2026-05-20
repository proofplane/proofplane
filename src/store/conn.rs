use deadpool_postgres::{Pool, Runtime};
use thiserror::Error;
use tokio_postgres::{connect, Client, NoTls};
use tracing::{debug, error};

#[derive(Debug, Error)]
pub enum Error {
    #[error("postgres error: {0}")]
    TokioPostgres(#[from] tokio_postgres::Error),

    #[error("failed to build postgres pool")]
    Deadpool(#[from] deadpool_postgres::BuildError),
}

pub async fn conn(conn_str: &str) -> Result<Client, Error> {
    let (client, connection) = connect(conn_str, NoTls).await?;

    debug!("connected to postgres");

    tokio::spawn(async move {
        debug!("running connection");
        if let Err(e) = connection.await {
            error!("running connection returned error: {}", e);
        }
    });

    Ok(client)
}

pub async fn conn_pool(conn_str: &str, max_size: usize) -> Result<Pool, Error> {
    let db_url = conn_str;

    let config: tokio_postgres::Config = db_url.parse()?;
    let mgr = deadpool_postgres::Manager::new(config, NoTls);

    Pool::builder(mgr)
        .max_size(max_size)
        .runtime(Runtime::Tokio1)
        .build()
        .map_err(Error::Deadpool)
}
