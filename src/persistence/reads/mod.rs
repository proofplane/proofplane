//! Synchronous Postgres read gateways.
//!
//! These modules load read models from the source-of-truth tables. They are
//! not asynchronous or materialized projection processors.

mod agent_connections;
mod auditor_access_grants;
mod auditor_auth_transactions;
mod auditor_portal;
mod auditor_sessions;
mod contexts;
mod controls;
mod documents;
mod evidence;
mod evidence_submissions;
mod executor;
mod frameworks;
mod oauth_authorization_flows;
mod policies;
mod users;
mod workspace_people;
mod workspaces;

pub(crate) use super::param;
pub(crate) use agent_connections::AgentConnectionReads;
pub(crate) use auditor_access_grants::AuditorAccessGrantReads;
pub(crate) use auditor_auth_transactions::AuditorAuthTransactionReads;
pub(crate) use auditor_portal::AuditorPortalReads;
pub(crate) use auditor_sessions::AuditorSessionReads;
pub(crate) use contexts::{Reads, WorkspaceScopedReads};
pub(crate) use controls::ControlReads;
pub(crate) use documents::DocumentReads;
pub(crate) use evidence::EvidenceReads;
pub(crate) use evidence_submissions::EvidenceSubmissionReads;
pub(crate) use executor::{PooledReadExecutor, ReadExecutor, TransactionalReadExecutor};
pub(crate) use frameworks::FrameworkReads;
pub(crate) use oauth_authorization_flows::OAuthAuthorizationFlowReads;
pub(crate) use policies::PolicyReads;
pub(crate) use users::UserReads;
pub(crate) use workspace_people::WorkspacePeopleReads;
pub(crate) use workspaces::WorkspaceReads;
