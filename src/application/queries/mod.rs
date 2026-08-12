//! Read-side application operations.
//!
//! Query modules follow the same one-operation/one-concrete-handler convention
//! as commands, but read DTOs directly and never rehydrate mutable aggregates.

pub mod agent_connections;
pub mod control_catalog;
pub mod evidence_catalog;
pub mod framework_catalog;
pub mod get_user;
pub mod get_workspace_for_user;
pub mod list_auditor_access_grants;
pub mod oauth_authorization_flows;
pub mod policy_catalog;
pub mod read_auditor_portal;
pub mod resolve_active_auditor_grant;
pub mod resolve_auditor_grant_by_secret;
pub mod resolve_auditor_session_by_digest;
pub mod resolve_evidence_document_upload_grant_authority;
pub mod resolve_policy_document_upload_grant_authority;
pub mod workspace_invitations;
