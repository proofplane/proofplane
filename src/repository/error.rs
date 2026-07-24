use thiserror::Error;
use uuid::Uuid;

use crate::domain::DomainError;

use super::constraints::ConflictKind;

/// Every way a create/attach batch can name offending ids, surfaced together so
/// the caller learns everything that failed in one response. `archived` is only
/// populated when the counterparts are policies (`attach_control_to_policies`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BatchMapRejection {
    pub unknown: Vec<Uuid>,
    pub archived: Vec<Uuid>,
    pub already_mapped: Vec<Uuid>,
}

impl BatchMapRejection {
    pub fn is_empty(&self) -> bool {
        self.unknown.is_empty() && self.archived.is_empty() && self.already_mapped.is_empty()
    }
}

/// Every way a remove/detach batch can name offending ids, surfaced together so
/// the caller learns everything that failed in one response. `archived` is only
/// populated when the counterparts are policies (`detach_control_from_policies`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BatchUnmapRejection {
    pub unknown: Vec<Uuid>,
    pub archived: Vec<Uuid>,
    pub not_mapped: Vec<Uuid>,
}

impl BatchUnmapRejection {
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

    #[error("batch map rejected: {0:?}")]
    BatchMapRejected(BatchMapRejection),

    #[error("batch unmap rejected: {0:?}")]
    BatchUnmapRejected(BatchUnmapRejection),

    #[error("repository invariant violation: {0}")]
    InvariantViolation(&'static str),
}
