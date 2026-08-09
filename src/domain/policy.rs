use std::collections::HashSet;

use chrono::{DateTime, Utc};

use crate::{validate, validation::Validation};

use uuid::Uuid;

use super::{
    ids::uuid_id, optional_text, BatchKey, ControlId, ControlSummary, DomainError, WorkspaceId,
};

uuid_id!(PolicyId);
uuid_id!(PolicyDocumentUploadGrantId);

impl BatchKey for PolicyId {
    fn key(&self) -> Uuid {
        (*self).into()
    }
}

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

/// Complete mutable snapshot for a policy and its control mappings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyAggregate {
    id: PolicyId,
    workspace_id: WorkspaceId,
    definition: PolicyDefinition,
    mappings: Vec<PolicyControlMappingState>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    archived_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyDefinition {
    name: String,
    description: Option<String>,
}

impl PolicyDefinition {
    pub fn new(raw_name: String, raw_description: Option<String>) -> Validation<Self, DomainError> {
        validate! {
            name <- validate_policy_name(raw_name),
            description <- optional_text("description", raw_description, 4_000),
            => Self { name, description },
        }
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyControlMappingState {
    control_id: ControlId,
    created_at: DateTime<Utc>,
}
impl PolicyControlMappingState {
    pub fn new(control_id: ControlId, created_at: DateTime<Utc>) -> Self {
        Self {
            control_id,
            created_at,
        }
    }
    pub fn control_id(&self) -> ControlId {
        self.control_id
    }
    pub fn created_at(&self) -> DateTime<Utc> {
        self.created_at
    }
}

impl PolicyAggregate {
    pub fn define(
        id: PolicyId,
        workspace_id: WorkspaceId,
        definition: PolicyDefinition,
        created_at: DateTime<Utc>,
    ) -> Self {
        Self {
            id,
            workspace_id,
            definition,
            mappings: Vec::new(),
            created_at,
            updated_at: created_at,
            archived_at: None,
        }
    }
    pub(crate) fn rehydrate(
        id: PolicyId,
        workspace_id: WorkspaceId,
        definition: PolicyDefinition,
        mappings: Vec<PolicyControlMappingState>,
        created_at: DateTime<Utc>,
        updated_at: DateTime<Utc>,
        archived_at: Option<DateTime<Utc>>,
    ) -> Result<Self, PolicyAggregateError> {
        if updated_at < created_at || archived_at.is_some_and(|at| at < created_at) {
            return Err(PolicyAggregateError::InvalidRehydration);
        }
        let mut policy = Self::define(id, workspace_id, definition, created_at);
        policy.replace_mappings(mappings)?;
        policy.updated_at = updated_at;
        policy.archived_at = archived_at;
        Ok(policy)
    }
    pub fn replace(
        &mut self,
        definition: PolicyDefinition,
        at: DateTime<Utc>,
    ) -> Result<(), PolicyAggregateError> {
        self.ensure_active()?;
        if at < self.created_at {
            return Err(PolicyAggregateError::InvalidReplacementTime);
        }
        self.definition = definition;
        self.updated_at = at;
        Ok(())
    }
    pub fn archive(&mut self, at: DateTime<Utc>) -> Result<(), PolicyAggregateError> {
        self.ensure_active()?;
        if at < self.created_at {
            return Err(PolicyAggregateError::InvalidReplacementTime);
        }
        self.archived_at = Some(at);
        self.updated_at = at;
        Ok(())
    }
    pub fn replace_mappings(
        &mut self,
        mut mappings: Vec<PolicyControlMappingState>,
    ) -> Result<(), PolicyAggregateError> {
        self.ensure_active()?;
        mappings.sort_unstable_by_key(|m| Uuid::from(m.control_id()));
        if mappings
            .windows(2)
            .any(|pair| pair[0].control_id() == pair[1].control_id())
        {
            return Err(PolicyAggregateError::DuplicateControlMapping);
        }
        self.mappings = mappings;
        Ok(())
    }
    fn ensure_active(&self) -> Result<(), PolicyAggregateError> {
        if self.archived_at.is_some() {
            Err(PolicyAggregateError::Archived)
        } else {
            Ok(())
        }
    }
    pub fn id(&self) -> PolicyId {
        self.id
    }
    pub fn workspace_id(&self) -> WorkspaceId {
        self.workspace_id
    }
    pub fn name(&self) -> &str {
        self.definition.name()
    }
    pub fn description(&self) -> Option<&str> {
        self.definition.description()
    }
    pub fn mappings(&self) -> &[PolicyControlMappingState] {
        &self.mappings
    }
    pub fn created_at(&self) -> DateTime<Utc> {
        self.created_at
    }
    pub fn updated_at(&self) -> DateTime<Utc> {
        self.updated_at
    }
    pub fn archived_at(&self) -> Option<DateTime<Utc>> {
        self.archived_at
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PolicyAggregateError {
    #[error("policy is archived")]
    Archived,
    #[error("control mapping is duplicated")]
    DuplicateControlMapping,
    #[error("policy snapshot is inconsistent")]
    InvalidRehydration,
    #[error("policy replacement predates its creation")]
    InvalidReplacementTime,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreatePolicyControlMappingsPayload {
    pub policy_id: PolicyId,
    pub control_ids: Vec<ControlId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateControlPolicyMappingsPayload {
    pub control_id: ControlId,
    pub policy_ids: Vec<PolicyId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeletePolicyControlMappingsPayload {
    pub policy_id: PolicyId,
    pub control_ids: Vec<ControlId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeleteControlPolicyMappingsPayload {
    pub control_id: ControlId,
    pub policy_ids: Vec<PolicyId>,
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
    use chrono::{TimeZone, Utc};
    use uuid::Uuid;

    use super::{
        validate_policy_name, validate_unique_policy_control_ids, PolicyAggregate,
        PolicyAggregateError, PolicyDefinition, PolicyId,
    };
    use crate::domain::{ControlId, DomainError, WorkspaceId};

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

    #[test]
    fn archive_rejects_later_definition_and_mapping_mutations() {
        let created_at = Utc.with_ymd_and_hms(2026, 8, 9, 12, 0, 0).unwrap();
        let mut policy = PolicyAggregate::define(
            PolicyId::from(Uuid::new_v4()),
            WorkspaceId::from(Uuid::new_v4()),
            PolicyDefinition::new("  Security  ".into(), Some("  Description  ".into()))
                .into_result()
                .unwrap(),
            created_at,
        );
        policy.archive(created_at).unwrap();
        assert_eq!(
            policy.replace_mappings(Vec::new()),
            Err(PolicyAggregateError::Archived)
        );
        assert_eq!(
            policy.replace(
                PolicyDefinition::new("Replacement".into(), None)
                    .into_result()
                    .unwrap(),
                created_at,
            ),
            Err(PolicyAggregateError::Archived)
        );
        assert_eq!(policy.name(), "Security");
        assert_eq!(policy.description(), Some("Description"));
    }
}
