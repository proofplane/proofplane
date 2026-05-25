use std::fmt;

use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WorkspaceId(Uuid);

impl fmt::Display for WorkspaceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl From<Uuid> for WorkspaceId {
    fn from(value: Uuid) -> Self {
        Self(value)
    }
}

impl From<WorkspaceId> for Uuid {
    fn from(value: WorkspaceId) -> Self {
        value.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EvidenceRequestId(Uuid);

impl fmt::Display for EvidenceRequestId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl From<Uuid> for EvidenceRequestId {
    fn from(value: Uuid) -> Self {
        Self(value)
    }
}

impl From<EvidenceRequestId> for Uuid {
    fn from(value: EvidenceRequestId) -> Self {
        value.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FrameworkId(Uuid);

impl fmt::Display for FrameworkId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl From<Uuid> for FrameworkId {
    fn from(value: Uuid) -> Self {
        Self(value)
    }
}

impl From<FrameworkId> for Uuid {
    fn from(value: FrameworkId) -> Self {
        value.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FrameworkRequirementId(Uuid);

impl fmt::Display for FrameworkRequirementId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl From<Uuid> for FrameworkRequirementId {
    fn from(value: Uuid) -> Self {
        Self(value)
    }
}

impl From<FrameworkRequirementId> for Uuid {
    fn from(value: FrameworkRequirementId) -> Self {
        value.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ControlId(Uuid);

impl fmt::Display for ControlId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl From<Uuid> for ControlId {
    fn from(value: Uuid) -> Self {
        Self(value)
    }
}

impl From<ControlId> for Uuid {
    fn from(value: ControlId) -> Self {
        value.0
    }
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::{ControlId, EvidenceRequestId, FrameworkId, FrameworkRequirementId, WorkspaceId};

    #[test]
    fn workspace_id_is_uuid_value_type() {
        let uuid = Uuid::parse_str("00000000-0000-4000-8000-000000000001").unwrap();
        let id = WorkspaceId::from(uuid);

        assert_eq!(Uuid::from(id), uuid);
        assert_eq!(id, WorkspaceId::from(uuid));
    }

    #[test]
    fn evidence_request_id_wraps_uuid() {
        let uuid = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap();
        let id = EvidenceRequestId::from(uuid);

        assert_eq!(Uuid::from(id), uuid);
    }

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
