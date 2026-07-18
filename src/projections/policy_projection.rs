use chrono::{DateTime, Utc};

use crate::domain::{DocumentId, DocumentUploadStatus, Policy, PolicyId, UserId};

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
    pub policy: Policy,
    pub document: Option<PolicyDocumentDetail>,
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
