//! Read-side application operations.
//!
//! Query modules follow the same one-operation/one-concrete-handler convention
//! as commands, but read DTOs directly and never rehydrate mutable aggregates.

pub mod control_catalog;
pub mod framework_catalog;
pub mod get_user;
pub mod get_workspace_for_user;
pub mod resolve_evidence_document_upload_grant_authority;
pub mod resolve_policy_document_upload_grant_authority;
