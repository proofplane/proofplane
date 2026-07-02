use chrono::{DateTime, Utc};

use super::{
    AttachmentUploadStatus, ControlId, EvidenceAttachmentId, EvidenceRequest, EvidenceSubmission,
    EvidenceSubmissionId, FrameworkRequirement, WorkspaceId,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditorPortalReadModel {
    pub workspace_id: WorkspaceId,
    pub auditor_email: String,
    pub controls: Vec<AuditorPortalControl>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditorPortalControl {
    pub id: ControlId,
    pub code: String,
    pub title: String,
    pub description: String,
    pub framework_requirements: Vec<FrameworkRequirement>,
    pub evidence_requests: Vec<AuditorPortalEvidenceRequest>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditorPortalEvidenceRequest {
    pub mapping_rationale: String,
    pub mapping_created_at: DateTime<Utc>,
    pub request: EvidenceRequest,
    pub submissions: Vec<AuditorPortalSubmission>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditorPortalSubmission {
    pub submission: EvidenceSubmission,
    pub attachments: Vec<AuditorPortalAttachment>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditorPortalAttachment {
    pub id: EvidenceAttachmentId,
    pub evidence_submission_id: EvidenceSubmissionId,
    pub filename: String,
    pub content_type: String,
    pub content_length: i64,
    pub checksum_sha256: String,
    pub checksum_crc32c: String,
    pub upload_status: AttachmentUploadStatus,
    pub download_eligible: bool,
}
