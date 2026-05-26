use chrono::{DateTime, Utc};

use super::{ids::uuid_id, EvidenceRequestId, WorkspaceId};

uuid_id!(FrameworkId);
uuid_id!(FrameworkRequirementId);
uuid_id!(ControlId);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Framework {
    pub id: FrameworkId,
    pub code: String,
    pub name: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameworkRequirement {
    pub id: FrameworkRequirementId,
    pub framework_id: FrameworkId,
    pub code: String,
    pub title: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Control {
    pub id: ControlId,
    pub workspace_id: WorkspaceId,
    pub code: String,
    pub title: String,
    pub description: String,
    pub framework_requirements: Vec<FrameworkRequirement>,
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
pub struct CreateControlPayload {
    pub code: String,
    pub title: String,
    pub description: String,
    pub framework_requirement_ids: Vec<FrameworkRequirementId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateControlPayload {
    pub code: String,
    pub title: String,
    pub description: String,
    pub framework_requirement_ids: Vec<FrameworkRequirementId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvidenceRequestControlMapping {
    pub evidence_request_id: EvidenceRequestId,
    pub control: ControlSummary,
    pub rationale: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateEvidenceRequestControlMappingPayload {
    pub evidence_request_id: EvidenceRequestId,
    pub control_id: ControlId,
    pub rationale: String,
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::{ControlId, FrameworkId, FrameworkRequirementId};

    #[test]
    fn framework_ids_wrap_uuid() {
        let uuid = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440001").unwrap();

        assert_eq!(Uuid::from(FrameworkId::from(uuid)), uuid);
        assert_eq!(Uuid::from(FrameworkRequirementId::from(uuid)), uuid);
    }

    #[test]
    fn control_id_wraps_uuid() {
        let uuid = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440002").unwrap();
        let id = ControlId::from(uuid);

        assert_eq!(Uuid::from(id), uuid);
    }
}
