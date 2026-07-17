use thiserror::Error;

pub mod agent_connections;
pub mod auditor_access_grants;
pub mod auditor_access_sessions;
pub mod auditor_portal;
pub mod cimd;
pub mod client_resolver;
pub mod controls;
pub mod evidence;
pub mod evidence_submissions;
pub mod evidence_upload_grants;
pub mod oauth;
pub mod submission_downloads;
pub mod upload_sessions;
pub mod user;
pub mod workspaces;

#[derive(Debug, Error)]
pub enum Error {
    #[error("repository error")]
    Repository(#[from] crate::repository::Error),

    #[error("object storage error")]
    Storage(#[from] crate::object_storage::StorageError),
}
