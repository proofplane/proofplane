use thiserror::Error;

pub mod controls;
pub mod evidence_requests;
pub mod evidence_submissions;

#[derive(Debug, Error)]
pub enum Error {
    #[error("repository error")]
    Repository(#[from] crate::repository::Error),

    #[error("object storage error")]
    Storage(#[from] crate::object_storage::StorageError),

    #[error("invalid framework requirement references")]
    InvalidFrameworkRequirementReferences,
}
