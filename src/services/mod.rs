use thiserror::Error;

pub mod agent_connections;
pub mod auditor_access_grants;
pub mod auditor_access_sessions;
pub mod auditor_portal;
pub mod cimd;
pub mod client_resolver;
pub mod controls;
pub mod document_downloads;
pub mod document_upload_grants;
mod documents;
pub mod evidence;
pub mod evidence_submissions;
pub mod oauth;
pub mod policies;
pub mod policy_document_upload_grants;
pub mod policy_documents;
pub mod policy_upload_sessions;
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
