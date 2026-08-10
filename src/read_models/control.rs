use chrono::{DateTime, Utc};

use crate::domain::{ControlId, EvidenceId, FrameworkId, FrameworkRequirementId, WorkspaceId};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameworkDetail {
    pub id: FrameworkId,
    pub code: String,
    pub name: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameworkRequirementDetail {
    pub id: FrameworkRequirementId,
    pub framework_id: FrameworkId,
    pub framework_code: String,
    pub framework_name: String,
    pub code: String,
    pub title: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlDetail {
    pub id: ControlId,
    pub workspace_id: WorkspaceId,
    pub code: String,
    pub title: String,
    pub description: String,
    pub framework_requirements: Vec<FrameworkRequirementDetail>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlSummary {
    pub id: ControlId,
    pub code: String,
    pub title: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvidenceControlMapping {
    pub evidence_id: EvidenceId,
    pub control: ControlSummary,
    pub rationale: String,
    pub created_at: DateTime<Utc>,
}
