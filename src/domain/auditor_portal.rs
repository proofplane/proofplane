use chrono::{DateTime, Utc};

use super::{
    ControlId, ControlSummary, DocumentId, DocumentUploadStatus, EvidenceRequest,
    EvidenceSubmission, EvidenceSubmissionId, FrameworkRequirement, PolicyId, UserId, WorkspaceId,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditorPortalReadModel {
    pub workspace_id: WorkspaceId,
    pub workspace_name: String,
    pub auditor_email: String,
    pub framework_requirements: Vec<FrameworkRequirement>,
    pub controls: Vec<AuditorPortalControl>,
    pub policies: Vec<AuditorPortalPolicy>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditorPortalControl {
    pub id: ControlId,
    pub code: String,
    pub title: String,
    pub description: String,
    pub framework_requirements: Vec<FrameworkRequirement>,
    pub evidence_requests: Vec<AuditorPortalEvidenceRequest>,
    pub policies: Vec<AuditorPortalPolicySummary>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditorPortalPolicy {
    pub id: PolicyId,
    pub name: String,
    pub description: Option<String>,
    pub controls: Vec<ControlSummary>,
    pub document: Option<AuditorPortalPolicyDocument>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditorPortalPolicySummary {
    pub id: PolicyId,
    pub name: String,
    pub description: Option<String>,
    pub document: Option<AuditorPortalPolicyDocumentStatus>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuditorPortalPolicyDocumentStatus {
    pub upload_status: DocumentUploadStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditorPortalPolicyDocument {
    pub id: DocumentId,
    pub policy_id: PolicyId,
    pub created_by_user_id: UserId,
    pub filename: String,
    pub content_type: String,
    pub content_length: i64,
    pub checksum_sha256: String,
    pub checksum_crc32c: String,
    pub upload_status: DocumentUploadStatus,
    pub created_at: DateTime<Utc>,
    pub download_eligible: bool,
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
    pub documents: Vec<AuditorPortalDocument>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditorPortalDocument {
    pub id: DocumentId,
    pub evidence_submission_id: EvidenceSubmissionId,
    pub created_by_user_id: UserId,
    pub filename: String,
    pub content_type: String,
    pub content_length: i64,
    pub checksum_sha256: String,
    pub checksum_crc32c: String,
    pub upload_status: DocumentUploadStatus,
    pub download_eligible: bool,
}
