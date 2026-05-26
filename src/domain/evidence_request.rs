use std::{fmt, str::FromStr};

use chrono::{DateTime, Utc};

use super::{DomainError, EvidenceRequestId, WorkspaceId};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvidenceRequestCadence {
    Once,
    Monthly,
    Quarterly,
    Annually,
}

impl EvidenceRequestCadence {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Once => "once",
            Self::Monthly => "monthly",
            Self::Quarterly => "quarterly",
            Self::Annually => "annually",
        }
    }
}

impl fmt::Display for EvidenceRequestCadence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for EvidenceRequestCadence {
    type Err = DomainError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "once" => Ok(Self::Once),
            "monthly" => Ok(Self::Monthly),
            "quarterly" => Ok(Self::Quarterly),
            "annually" => Ok(Self::Annually),
            _ => Err(DomainError::InvalidEnumValue {
                field: "cadence",
                value: value.to_owned(),
            }),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvidenceRequestStatus {
    Active,
    Paused,
    Retired,
}

impl EvidenceRequestStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Paused => "paused",
            Self::Retired => "retired",
        }
    }
}

impl fmt::Display for EvidenceRequestStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for EvidenceRequestStatus {
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvidenceRequest {
    pub id: EvidenceRequestId,
    pub workspace_id: WorkspaceId,
    pub title: String,
    pub description: String,
    pub collection_instructions: String,
    pub cadence: EvidenceRequestCadence,
    pub due_at: DateTime<Utc>,
    pub schedule_anchor_at: DateTime<Utc>,
    pub freshness_window_days: Option<i32>,
    pub status: EvidenceRequestStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateEvidenceRequestPayload {
    pub title: String,
    pub description: String,
    pub collection_instructions: String,
    pub cadence: EvidenceRequestCadence,
    pub due_at: DateTime<Utc>,
    pub schedule_anchor_at: DateTime<Utc>,
    pub freshness_window_days: Option<i32>,
    pub status: EvidenceRequestStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateEvidenceRequestPayload {
    pub title: String,
    pub description: String,
    pub collection_instructions: String,
    pub cadence: EvidenceRequestCadence,
    pub due_at: DateTime<Utc>,
    pub schedule_anchor_at: DateTime<Utc>,
    pub freshness_window_days: Option<i32>,
    pub status: EvidenceRequestStatus,
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::{EvidenceRequestCadence, EvidenceRequestStatus};
    use crate::domain::DomainError;

    #[test]
    fn cadence_parses_allowed_values() {
        assert_eq!(
            EvidenceRequestCadence::from_str("once").unwrap(),
            EvidenceRequestCadence::Once
        );
        assert_eq!(
            EvidenceRequestCadence::from_str("monthly").unwrap(),
            EvidenceRequestCadence::Monthly
        );
        assert_eq!(
            EvidenceRequestCadence::from_str("quarterly").unwrap(),
            EvidenceRequestCadence::Quarterly
        );
        assert_eq!(
            EvidenceRequestCadence::from_str("annually").unwrap(),
            EvidenceRequestCadence::Annually
        );
    }

    #[test]
    fn cadence_rejects_invalid_values() {
        assert_eq!(
            EvidenceRequestCadence::from_str("weekly").unwrap_err(),
            DomainError::InvalidEnumValue {
                field: "cadence",
                value: "weekly".to_owned()
            }
        );
    }

    #[test]
    fn status_parses_allowed_values() {
        assert_eq!(
            EvidenceRequestStatus::from_str("active").unwrap(),
            EvidenceRequestStatus::Active
        );
        assert_eq!(
            EvidenceRequestStatus::from_str("paused").unwrap(),
            EvidenceRequestStatus::Paused
        );
        assert_eq!(
            EvidenceRequestStatus::from_str("retired").unwrap(),
            EvidenceRequestStatus::Retired
        );
    }

    #[test]
    fn status_rejects_invalid_values() {
        assert_eq!(
            EvidenceRequestStatus::from_str("draft").unwrap_err(),
            DomainError::InvalidEnumValue {
                field: "status",
                value: "draft".to_owned()
            }
        );
    }
}
