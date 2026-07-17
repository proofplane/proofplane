use std::{fmt, str::FromStr};

use chrono::{DateTime, Utc};

use super::{ids::uuid_id, DomainError, WorkspaceId};

uuid_id!(EvidenceId);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvidenceStatus {
    Active,
    Paused,
    Retired,
}

impl EvidenceStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Paused => "paused",
            Self::Retired => "retired",
        }
    }
}

impl fmt::Display for EvidenceStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for EvidenceStatus {
    type Err = DomainError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "active" => Ok(Self::Active),
            "paused" => Ok(Self::Paused),
            "retired" => Ok(Self::Retired),
            _ => Err(DomainError::InvalidEnumValue {
                field: "status",
                value: value.to_owned(),
            }),
        }
    }
}

/**
 * Evidence represents something an organization must prove it does,
 * according to its controls. Its collection_instructions say how to
 * gather the proof. The proof itself arrives as evidence submissions,
 * each covering a period the submitter states.
 */
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Evidence {
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
pub struct CreateEvidencePayload {
    pub title: String,
    pub description: String,
    pub collection_instructions: String,
    pub status: EvidenceStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateEvidencePayload {
    pub title: String,
    pub description: String,
    pub collection_instructions: String,
    pub status: EvidenceStatus,
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use uuid::Uuid;

    use super::{EvidenceId, EvidenceStatus};
    use crate::domain::DomainError;

    #[test]
    fn evidence_id_wraps_uuid() {
        let uuid = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap();
        let id = EvidenceId::from(uuid);

        assert_eq!(Uuid::from(id), uuid);
    }

    #[test]
    fn status_parses_allowed_values() {
        assert_eq!(
            EvidenceStatus::from_str("active").unwrap(),
            EvidenceStatus::Active
        );
        assert_eq!(
            EvidenceStatus::from_str("paused").unwrap(),
            EvidenceStatus::Paused
        );
        assert_eq!(
            EvidenceStatus::from_str("retired").unwrap(),
            EvidenceStatus::Retired
        );
    }

    #[test]
    fn status_rejects_invalid_values() {
        assert_eq!(
            EvidenceStatus::from_str("draft").unwrap_err(),
            DomainError::InvalidEnumValue {
                field: "status",
                value: "draft".to_owned()
            }
        );
    }
}
