use thiserror::Error;

use crate::domain::DomainError;

use super::constraints::ConflictKind;

#[derive(Debug, Error)]
pub enum Error {
    #[error("database error")]
    Database(#[from] tokio_postgres::Error),

    #[error("conflict: {0:?}")]
    Conflict(ConflictKind),

    #[error("connection pool error")]
    Pool(#[from] deadpool_postgres::PoolError),

    #[error("invalid persisted data")]
    InvalidData(#[from] DomainError),

    #[error("policy control references are invalid")]
    InvalidPolicyControlReferences,

    #[error("repository invariant violation: {0}")]
    InvariantViolation(&'static str),
}
