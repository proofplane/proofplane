use thiserror::Error;

pub mod agent_connections;
pub mod agent_evidence_upload_grants;
pub mod agent_evidence_uploads;
pub mod agent_policy_document_upload_grants;
pub mod agent_policy_document_uploads;
pub mod cimd;
pub mod client_resolver;
pub mod document_downloads;
mod documents;
pub mod evidence_submissions;
pub mod oauth;
pub mod policy_document_upload_grants;
pub mod policy_documents;
pub mod policy_upload_sessions;
pub mod upload_sessions;

#[derive(Debug, Error)]
pub enum Error {
    #[error("repository error")]
    Repository(#[from] crate::repository::Error),

    #[error("object storage error")]
    Storage(#[from] crate::object_storage::StorageError),
}
