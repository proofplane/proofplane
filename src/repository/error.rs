use thiserror::Error;
use uuid::Uuid;

use crate::domain::DomainError;

use super::constraints::ConflictKind;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BatchRejection {
    pub unknown: Vec<Uuid>,
    pub archived: Vec<Uuid>,
    pub not_mapped: Vec<Uuid>,
}

impl BatchRejection {
    pub fn is_empty(&self) -> bool {
        self.unknown.is_empty() && self.archived.is_empty() && self.not_mapped.is_empty()
    }
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
