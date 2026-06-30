use thiserror::Error;
use tokio_postgres::error::SqlState;

use crate::{domain::DomainError, errors::Retryable};

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

    #[error("repository invariant violation: {0}")]
    InvariantViolation(&'static str),
}

impl Retryable for deadpool_postgres::PoolError {
    fn is_retryable(&self) -> bool {
        matches!(self, Self::Timeout(_) | Self::Backend(_))
    }
}

impl Retryable for Error {
    fn is_retryable(&self) -> bool {
        match self {
            Self::Database(error) => {
                error.is_closed() || error.code().is_some_and(is_read_sqlstate_retryable)
            }
            _ => false,
        }
    }
}

pub(super) fn is_transaction_sqlstate_retryable(code: &SqlState) -> bool {
    code == &SqlState::T_R_SERIALIZATION_FAILURE || code == &SqlState::T_R_DEADLOCK_DETECTED
}

fn is_read_sqlstate_retryable(code: &SqlState) -> bool {
    code.code().starts_with("08")
        || is_transaction_sqlstate_retryable(code)
        || matches!(
            code,
            &SqlState::TOO_MANY_CONNECTIONS
                | &SqlState::ADMIN_SHUTDOWN
                | &SqlState::CRASH_SHUTDOWN
                | &SqlState::CANNOT_CONNECT_NOW
        )
}

#[cfg(test)]
mod tests {
    use deadpool_postgres::{PoolError, TimeoutType};
    use tokio_postgres::error::SqlState;

    use crate::errors::Retryable;

    use super::{is_read_sqlstate_retryable, is_transaction_sqlstate_retryable};

    #[test]
    fn pool_classification_retries_only_transient_failures() {
        let timeout: PoolError = PoolError::Timeout(TimeoutType::Wait);
        let closed: PoolError = PoolError::Closed;
        let no_runtime: PoolError = PoolError::NoRuntimeSpecified;

        assert!(timeout.is_retryable());
        assert!(!closed.is_retryable());
        assert!(!no_runtime.is_retryable());
    }

    #[test]
    fn read_sqlstate_classification_matches_transient_failures() {
        for code in [
            SqlState::from_code("08006"),
            SqlState::T_R_SERIALIZATION_FAILURE,
            SqlState::T_R_DEADLOCK_DETECTED,
            SqlState::TOO_MANY_CONNECTIONS,
            SqlState::ADMIN_SHUTDOWN,
            SqlState::CRASH_SHUTDOWN,
            SqlState::CANNOT_CONNECT_NOW,
        ] {
            assert!(is_read_sqlstate_retryable(&code), "{}", code.code());
        }

        assert!(!is_read_sqlstate_retryable(&SqlState::UNIQUE_VIOLATION));
    }

    #[test]
    fn transaction_sqlstate_classification_is_narrow() {
        assert!(is_transaction_sqlstate_retryable(
            &SqlState::T_R_SERIALIZATION_FAILURE
        ));
        assert!(is_transaction_sqlstate_retryable(
            &SqlState::T_R_DEADLOCK_DETECTED
        ));
        assert!(!is_transaction_sqlstate_retryable(
            &SqlState::ADMIN_SHUTDOWN
        ));
    }
}
