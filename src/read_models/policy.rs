use chrono::{DateTime, Utc};

use crate::domain::{DocumentId, DocumentUploadStatus, PolicyId, UserId, WorkspaceId};

use super::ControlSummary;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyDocumentUploadEligibility {
    Eligible,
    CurrentDocument,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyCatalogEntry {
    pub id: PolicyId,
    pub name: String,
    pub description: Option<String>,
    pub mapped_control_count: i64,
    pub document: Option<PolicyDocumentStatus>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PolicyDocumentStatus {
    pub upload_status: DocumentUploadStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyDetail {
    pub id: PolicyId,
    pub workspace_id: WorkspaceId,
    pub name: String,
    pub description: Option<String>,
    pub control_mappings: Vec<PolicyControlMapping>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub archived_at: Option<DateTime<Utc>>,
    pub document: Option<PolicyDocumentDetail>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicySummary {
    pub id: PolicyId,
    pub workspace_id: WorkspaceId,
    pub name: String,
    pub description: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyControlMapping {
    pub policy_id: PolicyId,
    pub control: ControlSummary,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlPolicyMapping {
    pub policy: PolicySummary,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyDocumentDetail {
    pub id: DocumentId,
    pub created_by_user_id: UserId,
    pub filename: String,
    pub content_type: String,
    pub content_length: i64,
    pub checksum_sha256: String,
    pub checksum_crc32c: String,
    pub upload_status: DocumentUploadStatus,
    pub created_at: DateTime<Utc>,
}
