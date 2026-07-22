use thiserror::Error;
use uuid::Uuid;

use crate::domain::DomainError;

use super::constraints::ConflictKind;

/// Why a batch cannot be applied, carried through [`Error`] so returning it
/// rolls the transaction back — the operation's `Ok` value is what commits, so a
/// rejection discovered after a write must travel as an error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BatchRejection {
    UnknownIds(Vec<Uuid>),
    NotMapped(Vec<Uuid>),
}

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

    #[error("batch rejected: {0:?}")]
    BatchRejected(BatchRejection),

    #[error("repository invariant violation: {0}")]
    InvariantViolation(&'static str),
}
