use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("database error")]
    Database(#[from] tokio_postgres::Error),

    #[error("connection pool error")]
    Pool(#[from] deadpool_postgres::PoolError),
}
