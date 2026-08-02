use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::{validate, validation::Validation};

use super::{ids::uuid_id, required_text, BatchKey, DomainError, EvidenceId, WorkspaceId};

uuid_id!(FrameworkId);
uuid_id!(FrameworkRequirementId);
uuid_id!(ControlId);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlDefinition {
    code: String,
    title: String,
    description: String,
}

impl ControlDefinition {
    pub fn new(
        raw_code: String,
        raw_title: String,
        raw_description: String,
    ) -> Validation<Self, DomainError> {
        validate! {
            code <- required_text("code", raw_code),
            title <- required_text("title", raw_title),
            description <- required_text("description", raw_description),
            => Self { code, title, description },
        }
    }

    pub fn code(&self) -> &str {
        &self.code
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn description(&self) -> &str {
        &self.description
    }
}

/// Complete mutable snapshot for one workspace control.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlAggregate {
    id: ControlId,
    workspace_id: WorkspaceId,
    definition: ControlDefinition,
    framework_requirement_ids: Vec<FrameworkRequirementId>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl ControlAggregate {
    pub fn define(
        id: ControlId,
        workspace_id: WorkspaceId,
        definition: ControlDefinition,
        framework_requirement_ids: Vec<FrameworkRequirementId>,
        created_at: DateTime<Utc>,
    ) -> Result<Self, ControlAggregateError> {
        let framework_requirement_ids =
            normalize_framework_requirement_ids(framework_requirement_ids)?;
        Ok(Self {
            id,
            workspace_id,
            definition,
            framework_requirement_ids,
            created_at,
            updated_at: created_at,
        })
    }

    pub(crate) fn rehydrate(
        id: ControlId,
        workspace_id: WorkspaceId,
        definition: ControlDefinition,
        framework_requirement_ids: Vec<FrameworkRequirementId>,
        created_at: DateTime<Utc>,
        updated_at: DateTime<Utc>,
    ) -> Result<Self, ControlAggregateError> {
        if updated_at < created_at {
            return Err(ControlAggregateError::InvalidRehydration);
        }
        let mut control = Self::define(
            id,
            workspace_id,
            definition,
            framework_requirement_ids,
            created_at,
        )?;
        control.updated_at = updated_at;
        Ok(control)
    }

    pub fn replace(
        &mut self,
        definition: ControlDefinition,
        framework_requirement_ids: Vec<FrameworkRequirementId>,
        updated_at: DateTime<Utc>,
    ) -> Result<(), ControlAggregateError> {
        let framework_requirement_ids =
            normalize_framework_requirement_ids(framework_requirement_ids)?;
        if updated_at < self.created_at {
            return Err(ControlAggregateError::InvalidReplacementTime);
        }
        self.definition = definition;
        self.framework_requirement_ids = framework_requirement_ids;
        self.updated_at = updated_at;
        Ok(())
    }

    pub fn id(&self) -> ControlId {
        self.id
    }

    pub fn workspace_id(&self) -> WorkspaceId {
        self.workspace_id
    }

    pub fn code(&self) -> &str {
        self.definition.code()
    }

    pub fn title(&self) -> &str {
        self.definition.title()
    }

    pub fn description(&self) -> &str {
        self.definition.description()
    }

    pub fn framework_requirement_ids(&self) -> &[FrameworkRequirementId] {
        &self.framework_requirement_ids
    }

    pub fn created_at(&self) -> DateTime<Utc> {
        self.created_at
    }

    pub fn updated_at(&self) -> DateTime<Utc> {
        self.updated_at
    }
}

fn normalize_framework_requirement_ids(
    mut ids: Vec<FrameworkRequirementId>,
) -> Result<Vec<FrameworkRequirementId>, ControlAggregateError> {
    ids.sort_unstable_by_key(|id| Uuid::from(*id));
    let mut seen = std::collections::HashSet::with_capacity(ids.len());
    for id in &ids {
        if !seen.insert(*id) {
            return Err(ControlAggregateError::DuplicateFrameworkRequirementReference(*id));
        }
    }
    Ok(ids)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ControlAggregateError {
    #[error("framework requirement reference is duplicated")]
    DuplicateFrameworkRequirementReference(FrameworkRequirementId),
    #[error("persisted control snapshot is inconsistent")]
    InvalidRehydration,
    #[error("control replacement predates its creation")]
    InvalidReplacementTime,
}

/**
 * A Framework is a specific set of rules an organization wants to adhere to.
 */
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Framework {
    pub id: FrameworkId,
    pub code: String,
    pub name: String,
    pub description: String,
}

/**
 * A FrameworkRequirement is a specific rule inside a framework.
 */
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameworkRequirement {
    pub id: FrameworkRequirementId,
    pub framework_id: FrameworkId,
    pub framework_code: String,
    pub framework_name: String,
    pub code: String,
    pub title: String,
    pub description: String,
}

/**
 * A Control is an organization's way of ensuring they are adhering to
 * a requirement. Sometimes, different frameworks have similar requirements
 * so it can be useful to use the same control for multiple requirements.
 */
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

impl BatchKey for FrameworkRequirementId {
    fn key(&self) -> Uuid {
        (*self).into()
    }
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
pub struct EvidenceControlMapping {
    pub evidence_id: EvidenceId,
    pub control: ControlSummary,
    pub rationale: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateEvidenceControlMappingPayload {
    pub evidence_id: EvidenceId,
    pub control_id: ControlId,
    pub rationale: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvidenceControlMappingItem {
    pub control_id: ControlId,
    pub rationale: String,
}

impl BatchKey for EvidenceControlMappingItem {
    fn key(&self) -> Uuid {
        self.control_id.into()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateEvidenceControlMappingsPayload {
    pub evidence_id: EvidenceId,
    pub items: Vec<EvidenceControlMappingItem>,
}

impl BatchKey for ControlId {
    fn key(&self) -> Uuid {
        (*self).into()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeleteEvidenceControlMappingsPayload {
    pub evidence_id: EvidenceId,
    pub control_ids: Vec<ControlId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeleteControlEvidenceMappingsPayload {
    pub control_id: ControlId,
    pub evidence_ids: Vec<EvidenceId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlEvidenceMappingItem {
    pub evidence_id: EvidenceId,
    pub rationale: String,
}

impl BatchKey for ControlEvidenceMappingItem {
    fn key(&self) -> Uuid {
        self.evidence_id.into()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateControlEvidenceMappingsPayload {
    pub control_id: ControlId,
    pub items: Vec<ControlEvidenceMappingItem>,
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, TimeZone, Utc};
    use uuid::Uuid;

    use super::{
        ControlAggregate, ControlAggregateError, ControlDefinition, ControlId, FrameworkId,
        FrameworkRequirementId,
    };

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

    #[test]
    fn control_replacement_changes_the_complete_definition_and_reference_snapshot() {
        let created_at = Utc.with_ymd_and_hms(2026, 8, 2, 12, 0, 0).unwrap();
        let updated_at = created_at + Duration::minutes(5);
        let first_requirement = FrameworkRequirementId::from(Uuid::new_v4());
        let second_requirement = FrameworkRequirementId::from(Uuid::new_v4());
        let mut control = ControlAggregate::define(
            ControlId::from(Uuid::new_v4()),
            Uuid::new_v4().into(),
            ControlDefinition::new(
                "PP-AC-01".to_owned(),
                "Access review".to_owned(),
                "Review access quarterly.".to_owned(),
            )
            .into_result()
            .unwrap(),
            vec![first_requirement],
            created_at,
        )
        .unwrap();

        control
            .replace(
                ControlDefinition::new(
                    "PP-AC-02".to_owned(),
                    "Privileged access review".to_owned(),
                    "Review privileged access monthly.".to_owned(),
                )
                .into_result()
                .unwrap(),
                vec![second_requirement],
                updated_at,
            )
            .unwrap();

        assert_eq!(control.code(), "PP-AC-02");
        assert_eq!(control.title(), "Privileged access review");
        assert_eq!(control.description(), "Review privileged access monthly.");
        assert_eq!(control.framework_requirement_ids(), &[second_requirement]);
        assert_eq!(control.created_at(), created_at);
        assert_eq!(control.updated_at(), updated_at);
    }

    #[test]
    fn duplicate_framework_references_are_rejected_without_changing_the_snapshot() {
        let now = Utc.with_ymd_and_hms(2026, 8, 2, 12, 0, 0).unwrap();
        let requirement = FrameworkRequirementId::from(Uuid::new_v4());
        let mut control = ControlAggregate::define(
            ControlId::from(Uuid::new_v4()),
            Uuid::new_v4().into(),
            ControlDefinition::new(
                "PP-AC-01".to_owned(),
                "Access review".to_owned(),
                "Review access quarterly.".to_owned(),
            )
            .into_result()
            .unwrap(),
            vec![requirement],
            now,
        )
        .unwrap();
        let before = control.clone();

        assert_eq!(
            control.replace(
                ControlDefinition::new(
                    "PP-AC-02".to_owned(),
                    "Changed".to_owned(),
                    "Changed description.".to_owned(),
                )
                .into_result()
                .unwrap(),
                vec![requirement, requirement],
                now + Duration::minutes(1),
            ),
            Err(ControlAggregateError::DuplicateFrameworkRequirementReference(requirement))
        );
        assert_eq!(control, before);
    }
}
