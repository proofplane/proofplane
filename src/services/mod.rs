use thiserror::Error;

pub mod actors;
pub mod api_tokens;
pub mod attachment_downloads;
pub mod controls;
pub mod evidence_requests;
pub mod evidence_submissions;
pub mod user;
pub mod workspaces;

#[derive(Debug, Error)]
pub enum Error {
    #[error("repository error")]
    Repository(#[from] crate::repository::Error),

    #[error("object storage error")]
    Storage(#[from] crate::object_storage::StorageError),
}
