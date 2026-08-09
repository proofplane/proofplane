use chrono::{DateTime, Utc};

use crate::domain::{EvidenceId, EvidenceStatus, WorkspaceId};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvidenceDetail {
    pub id: EvidenceId,
    pub workspace_id: WorkspaceId,
    pub title: String,
    pub description: String,
    pub collection_instructions: String,
    pub status: EvidenceStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlEvidenceMapping {
    pub evidence: EvidenceDetail,
    pub rationale: String,
    pub created_at: DateTime<Utc>,
}
