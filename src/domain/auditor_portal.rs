use chrono::{DateTime, Utc};

use super::{ControlId, Evidence, EvidenceSubmission, FrameworkRequirement, WorkspaceId};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditorPortalReadModel {
    pub workspace_id: WorkspaceId,
    pub workspace_name: String,
    pub auditor_email: String,
    pub framework_requirements: Vec<FrameworkRequirement>,
    pub controls: Vec<AuditorPortalControl>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditorPortalControl {
    pub id: ControlId,
    pub code: String,
    pub title: String,
    pub description: String,
    pub framework_requirements: Vec<FrameworkRequirement>,
    pub evidence: Vec<AuditorPortalEvidence>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditorPortalEvidence {
    pub mapping_rationale: String,
    pub mapping_created_at: DateTime<Utc>,
    pub evidence: Evidence,
    pub submissions: Vec<AuditorPortalSubmission>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditorPortalSubmission {
    pub submission: EvidenceSubmission,
    pub download_eligible: bool,
}
