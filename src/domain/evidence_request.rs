use std::{fmt, str::FromStr};

use chrono::{DateTime, Utc};

use crate::{validate, validation::Validation};

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
pub struct NewEvidenceRequest {
    pub workspace_id: WorkspaceId,
    pub title: String,
    pub description: String,
    pub collection_instructions: String,
    pub cadence: EvidenceRequestCadence,
    pub due_at: DateTime<Utc>,
    pub schedule_anchor_at: DateTime<Utc>,
    pub freshness_window_days: Option<i32>,
    pub status: EvidenceRequestStatus,
}

impl NewEvidenceRequest {
    pub fn validate(self) -> Validation<Self, DomainError> {
        validate! {
            title <- required_text("title", self.title),
            description <- required_text("description", self.description),
            collection_instructions <- required_text(
                "collection_instructions",
                self.collection_instructions
            ),
            freshness_window_days <- validate_freshness_window_days(self.freshness_window_days),
            => Self {
                workspace_id: self.workspace_id,
                title,
                description,
                collection_instructions,
                cadence: self.cadence,
                due_at: self.due_at,
                schedule_anchor_at: self.schedule_anchor_at,
                freshness_window_days,
                status: self.status,
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvidenceRequestUpdate {
    pub title: String,
    pub description: String,
    pub collection_instructions: String,
    pub cadence: EvidenceRequestCadence,
    pub due_at: DateTime<Utc>,
    pub schedule_anchor_at: DateTime<Utc>,
    pub freshness_window_days: Option<i32>,
    pub status: EvidenceRequestStatus,
}

impl EvidenceRequestUpdate {
    pub fn validate(self) -> Validation<Self, DomainError> {
        validate! {
            title <- required_text("title", self.title),
            description <- required_text("description", self.description),
            collection_instructions <- required_text(
                "collection_instructions",
                self.collection_instructions
            ),
            freshness_window_days <- validate_freshness_window_days(self.freshness_window_days),
            => Self {
                title,
                description,
                collection_instructions,
                cadence: self.cadence,
                due_at: self.due_at,
                schedule_anchor_at: self.schedule_anchor_at,
                freshness_window_days,
                status: self.status,
            },
        }
    }
}

fn required_text(field: &'static str, value: String) -> Validation<String, DomainError> {
    if value.trim().is_empty() {
        return Validation::invalid(DomainError::EmptyRequiredText { field });
    }

    Validation::valid(value)
}

fn validate_freshness_window_days(value: Option<i32>) -> Validation<Option<i32>, DomainError> {
    match value {
        Some(days) if days <= 0 => Validation::invalid(DomainError::InvalidFreshnessWindowDays),
        _ => Validation::valid(value),
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use chrono::{DateTime, Utc};
    use uuid::Uuid;

    use super::{
        EvidenceRequestCadence, EvidenceRequestStatus, EvidenceRequestUpdate, NewEvidenceRequest,
    };
    use crate::domain::{DomainError, WorkspaceId};

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

    #[test]
    fn new_evidence_request_rejects_blank_required_fields() {
        let errors = new_request_with_title(" ").unwrap_err();

        assert_eq!(
            errors,
            vec![DomainError::EmptyRequiredText { field: "title" }]
        );
    }

    #[test]
    fn new_evidence_request_accumulates_field_errors() {
        let errors = new_request_with_fields("", " ", "\t", Some(0)).unwrap_err();

        assert_eq!(
            errors,
            vec![
                DomainError::EmptyRequiredText { field: "title" },
                DomainError::EmptyRequiredText {
                    field: "description"
                },
                DomainError::EmptyRequiredText {
                    field: "collection_instructions"
                },
                DomainError::InvalidFreshnessWindowDays
            ]
        );
    }

    #[test]
    fn evidence_request_update_rejects_blank_required_fields() {
        let errors = EvidenceRequestUpdate {
            title: "Quarterly access review".to_owned(),
            description: String::new(),
            collection_instructions: "Export review results.".to_owned(),
            cadence: EvidenceRequestCadence::Quarterly,
            due_at: unix_epoch(),
            schedule_anchor_at: unix_epoch(),
            freshness_window_days: Some(90),
            status: EvidenceRequestStatus::Active,
        }
        .validate()
        .into_result()
        .unwrap_err();

        assert_eq!(
            errors,
            vec![DomainError::EmptyRequiredText {
                field: "description"
            }]
        );
    }

    #[test]
    fn freshness_window_accepts_positive_values() {
        let request = new_request_with_freshness_window(Some(30)).expect("valid request");

        assert_eq!(request.freshness_window_days, Some(30));
    }

    #[test]
    fn freshness_window_accepts_absent_value() {
        let request = new_request_with_freshness_window(None).expect("valid request");

        assert_eq!(request.freshness_window_days, None);
    }

    #[test]
    fn freshness_window_rejects_zero() {
        assert_eq!(
            new_request_with_freshness_window(Some(0)).unwrap_err(),
            vec![DomainError::InvalidFreshnessWindowDays]
        );
    }

    fn new_request_with_title(title: &str) -> Result<NewEvidenceRequest, Vec<DomainError>> {
        new_request_with_fields(
            title,
            "Confirm quarterly access reviews are completed.",
            "Export the completed review report from the IdP.",
            Some(90),
        )
    }

    fn new_request_with_freshness_window(
        freshness_window_days: Option<i32>,
    ) -> Result<NewEvidenceRequest, Vec<DomainError>> {
        new_request_with_fields(
            "Quarterly access review",
            "Confirm quarterly access reviews are completed.",
            "Export the completed review report from the IdP.",
            freshness_window_days,
        )
    }

    fn new_request_with_fields(
        title: &str,
        description: &str,
        collection_instructions: &str,
        freshness_window_days: Option<i32>,
    ) -> Result<NewEvidenceRequest, Vec<DomainError>> {
        NewEvidenceRequest {
            workspace_id: workspace_id(),
            title: title.to_owned(),
            description: description.to_owned(),
            collection_instructions: collection_instructions.to_owned(),
            cadence: EvidenceRequestCadence::Quarterly,
            due_at: unix_epoch(),
            schedule_anchor_at: unix_epoch(),
            freshness_window_days,
            status: EvidenceRequestStatus::Active,
        }
        .validate()
        .into_result()
    }

    fn workspace_id() -> WorkspaceId {
        WorkspaceId::from(Uuid::parse_str("00000000-0000-4000-8000-000000000001").unwrap())
    }

    fn unix_epoch() -> DateTime<Utc> {
        DateTime::<Utc>::UNIX_EPOCH
    }
}
