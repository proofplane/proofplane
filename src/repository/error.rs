use thiserror::Error;

use crate::domain::DomainError;

#[derive(Debug, Error)]
pub enum Error {
    #[error("database error")]
    Database(#[from] tokio_postgres::Error),

    #[error("conflict: {0}")]
    Conflict(&'static str),

    #[error("connection pool error")]
    Pool(#[from] deadpool_postgres::PoolError),

    #[error("invalid persisted data")]
    InvalidData(#[from] DomainError),

    #[error("repository invariant violation: {0}")]
    InvariantViolation(&'static str),
}
