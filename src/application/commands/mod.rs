//! Write-side application operations.
//!
//! Each child module owns its command, concrete handler, result, and error.

pub mod agent_connections;
pub mod claim_auditor_auth_transaction;
pub mod complete_auditor_authentication;
pub mod create_authenticated_auditor_session;
pub mod create_control;
pub mod create_evidence;
pub mod create_owned_workspace;
pub mod documents;
pub mod issue_agent_evidence_upload_grant;
pub mod issue_auditor_access_grant;
pub mod issue_evidence_document_upload_grant;
pub mod issue_policy_document_upload_grant;
pub mod map_control_to_evidence;
pub mod map_evidence_to_controls;
pub mod oauth_authorization_flows;
pub mod policies;
pub mod provision_user;
pub mod record_user_login;
pub mod redeem_evidence_document_upload_grant;
pub mod redeem_policy_document_upload_grant;
pub mod remove_workspace_member;
pub mod replace_control;
pub mod replace_evidence;
pub mod revoke_auditor_access_grant;
pub mod revoke_auditor_session;
pub mod start_auditor_auth_transaction;
pub mod unmap_control_from_evidence;
pub mod unmap_evidence_from_controls;
