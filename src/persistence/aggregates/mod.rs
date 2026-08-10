//! Complete-snapshot repositories for mutable domain aggregates.

mod agent_connections;
mod agent_evidence_upload_grants;
mod agent_policy_document_upload_grants;
mod auditor_access_grants;
mod auditor_access_sessions;
mod auditor_auth_transactions;
mod controls;
mod document_upload_grants;
mod documents;
mod evidence;
mod evidence_submissions;
mod oauth;
mod policies;
mod policy_document_upload_grants;
mod users;
mod workspace_memberships;
mod workspaces;

use super::{constraints, snapshot, Error, Postgres, UnitOfWork, WorkspaceUnitOfWork};

pub use agent_connections::AgentConnectionRepository;
pub use agent_evidence_upload_grants::AgentEvidenceUploadGrantRepository;
pub use agent_policy_document_upload_grants::AgentPolicyDocumentUploadGrantRepository;
pub use auditor_access_grants::AuditorAccessGrantRepository;
pub use auditor_access_sessions::AuditorSessionRepository;
pub use auditor_auth_transactions::AuditorAuthTransactionRepository;
pub use document_upload_grants::EvidenceDocumentUploadGrantRepository;
pub use documents::{DocumentRepository, WorkspaceDocumentRepository};
pub use evidence_submissions::{ArchiveDocumentResult, EvidenceSubmissionRepository};
pub use oauth::OAuthAuthorizationFlowRepository;
pub use policies::{ArchivePolicyResult, CreatePolicyDocumentResult};
pub use policy_document_upload_grants::PolicyDocumentUploadGrantRepository;
pub use users::UserRepository;
pub use workspace_memberships::NewWorkspaceMembership;
pub use workspaces::WorkspaceRepository;
