use thiserror::Error;

pub mod agent_connections;
pub mod attachment_downloads;
pub mod attachment_upload_grants;
pub mod auditor_access_grants;
pub mod auditor_access_sessions;
pub mod auditor_portal;
pub mod cimd;
pub mod client_registration;
pub mod controls;
pub mod evidence_requests;
pub mod evidence_submissions;
pub mod oauth;
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
