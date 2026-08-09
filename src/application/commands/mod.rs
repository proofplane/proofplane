//! Write-side application operations.
//!
//! Each child module owns its command, concrete handler, result, and error.

pub mod create_control;
pub mod create_owned_workspace;
pub mod issue_agent_evidence_upload_grant;
pub mod issue_auditor_access_grant;
pub mod issue_evidence_document_upload_grant;
pub mod issue_policy_document_upload_grant;
pub mod provision_user;
pub mod record_user_login;
pub mod redeem_evidence_document_upload_grant;
pub mod redeem_policy_document_upload_grant;
pub mod remove_workspace_member;
pub mod replace_control;
pub mod revoke_auditor_access_grant;
