use chrono::{DateTime, Utc};

use crate::domain::{AttachmentUploadStatus, Policy, PolicyAttachmentId, PolicyId};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyCatalogEntry {
    pub id: PolicyId,
    pub name: String,
    pub description: Option<String>,
    pub mapped_control_count: i64,
    pub attachment: Option<PolicyAttachmentStatus>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PolicyAttachmentStatus {
    pub upload_status: AttachmentUploadStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyDetail {
    pub policy: Policy,
    pub attachment: Option<PolicyAttachmentDetail>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyAttachmentDetail {
    pub id: PolicyAttachmentId,
    pub filename: String,
    pub content_type: String,
    pub content_length: i64,
    pub checksum_sha256: String,
    pub checksum_crc32c: String,
    pub upload_status: AttachmentUploadStatus,
    pub created_at: DateTime<Utc>,
}
