use std::collections::HashSet;

use chrono::{DateTime, Utc};

use crate::validation::Validation;

use super::{ids::uuid_id, ControlId, ControlSummary, DomainError, WorkspaceId};

uuid_id!(PolicyId);
uuid_id!(PolicyAttachmentId);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Policy {
    pub id: PolicyId,
    pub workspace_id: WorkspaceId,
    pub name: String,
    pub description: Option<String>,
    pub control_mappings: Vec<PolicyControlMapping>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub archived_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyControlMapping {
    pub policy_id: PolicyId,
    pub control: ControlSummary,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreatePolicyPayload {
    pub name: String,
    pub description: Option<String>,
    pub control_ids: Vec<ControlId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdatePolicyPayload {
    pub name: String,
    pub description: Option<String>,
}

pub fn validate_policy_name(value: String) -> Validation<String, DomainError> {
    let value = value.trim().to_owned();
    if value.is_empty() {
        return Validation::invalid(DomainError::EmptyRequiredText { field: "name" });
    }
    if value.chars().count() > 200 {
        return Validation::invalid(DomainError::RequiredTextTooLong {
            field: "name",
            maximum: 200,
        });
    }

    Validation::valid(value)
}

pub fn validate_unique_policy_control_ids(
    ids: Vec<ControlId>,
) -> Validation<Vec<ControlId>, DomainError> {
    let mut seen = HashSet::with_capacity(ids.len());
    if ids.iter().copied().all(|id| seen.insert(id)) {
        return Validation::valid(ids);
    }

    Validation::invalid(DomainError::DuplicatePolicyControlId)
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::{validate_policy_name, validate_unique_policy_control_ids, PolicyId};
    use crate::domain::{ControlId, DomainError};

    #[test]
    fn policy_id_wraps_uuid() {
        let uuid = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440005").unwrap();
        assert_eq!(Uuid::from(PolicyId::from(uuid)), uuid);
    }

    #[test]
    fn policy_name_is_trimmed_and_limited_by_unicode_characters() {
        assert_eq!(
            validate_policy_name("  Security policy  ".to_owned()).into_result(),
            Ok("Security policy".to_owned())
        );
        assert_eq!(
            validate_policy_name("é".repeat(200)).into_result(),
            Ok("é".repeat(200))
        );
        assert_eq!(
            validate_policy_name(format!(" {} ", "é".repeat(201))).into_result(),
            Err(vec![DomainError::RequiredTextTooLong {
                field: "name",
                maximum: 200,
            }])
        );
    }

    #[test]
    fn policy_name_rejects_blank_text() {
        assert_eq!(
            validate_policy_name(" \t\n ".to_owned()).into_result(),
            Err(vec![DomainError::EmptyRequiredText { field: "name" }])
        );
    }

    #[test]
    fn policy_control_ids_must_be_unique() {
        let id = ControlId::from(Uuid::new_v4());
        assert_eq!(
            validate_unique_policy_control_ids(vec![id, id]).into_result(),
            Err(vec![DomainError::DuplicatePolicyControlId])
        );
    }
}
