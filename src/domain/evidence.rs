use std::{fmt, str::FromStr};

use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::{validate, validation::Validation};

use super::{ids::uuid_id, required_text, BatchKey, ControlId, DomainError, WorkspaceId};

uuid_id!(EvidenceId);

impl BatchKey for EvidenceId {
    fn key(&self) -> Uuid {
        (*self).into()
    }
}

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
pub struct EvidenceDefinition {
    title: String,
    description: String,
    collection_instructions: String,
}

impl EvidenceDefinition {
    pub fn new(
        raw_title: String,
        raw_description: String,
        raw_collection_instructions: String,
    ) -> Validation<Self, DomainError> {
        validate! {
            title <- required_text("title", raw_title),
            description <- required_text("description", raw_description),
            collection_instructions <- required_text("collection_instructions", raw_collection_instructions),
            => Self { title, description, collection_instructions },
        }
    }

    pub fn title(&self) -> &str {
        &self.title
    }
    pub fn description(&self) -> &str {
        &self.description
    }
    pub fn collection_instructions(&self) -> &str {
        &self.collection_instructions
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvidenceControlMappingState {
    control_id: ControlId,
    rationale: String,
    created_at: DateTime<Utc>,
}

impl EvidenceControlMappingState {
    pub fn new(
        control_id: ControlId,
        rationale: String,
        created_at: DateTime<Utc>,
    ) -> Validation<Self, DomainError> {
        required_text("rationale", rationale).map(|rationale| Self {
            control_id,
            rationale,
            created_at,
        })
    }

    pub fn control_id(&self) -> ControlId {
        self.control_id
    }
    pub fn rationale(&self) -> &str {
        &self.rationale
    }
    pub fn created_at(&self) -> DateTime<Utc> {
        self.created_at
    }
}

/// Complete mutable snapshot for one workspace evidence item and its mappings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvidenceAggregate {
    id: EvidenceId,
    workspace_id: WorkspaceId,
    definition: EvidenceDefinition,
    status: EvidenceStatus,
    mappings: Vec<EvidenceControlMappingState>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl EvidenceAggregate {
    pub fn define(
        id: EvidenceId,
        workspace_id: WorkspaceId,
        definition: EvidenceDefinition,
        status: EvidenceStatus,
        created_at: DateTime<Utc>,
    ) -> Self {
        Self {
            id,
            workspace_id,
            definition,
            status,
            mappings: Vec::new(),
            created_at,
            updated_at: created_at,
        }
    }

    pub(crate) fn rehydrate(
        id: EvidenceId,
        workspace_id: WorkspaceId,
        definition: EvidenceDefinition,
        status: EvidenceStatus,
        mappings: Vec<EvidenceControlMappingState>,
        created_at: DateTime<Utc>,
        updated_at: DateTime<Utc>,
    ) -> Result<Self, EvidenceAggregateError> {
        if updated_at < created_at {
            return Err(EvidenceAggregateError::InvalidRehydration);
        }
        let mut evidence = Self::define(id, workspace_id, definition, status, created_at);
        evidence.replace_mappings(mappings)?;
        evidence.updated_at = updated_at;
        Ok(evidence)
    }

    pub fn replace(
        &mut self,
        definition: EvidenceDefinition,
        status: EvidenceStatus,
        updated_at: DateTime<Utc>,
    ) -> Result<(), EvidenceAggregateError> {
        if updated_at < self.created_at {
            return Err(EvidenceAggregateError::InvalidReplacementTime);
        }
        self.definition = definition;
        self.status = status;
        self.updated_at = updated_at;
        Ok(())
    }

    pub fn replace_mappings(
        &mut self,
        mappings: Vec<EvidenceControlMappingState>,
    ) -> Result<(), EvidenceAggregateError> {
        let mappings = normalize_mappings(mappings)?;
        self.mappings = mappings;
        Ok(())
    }

    pub fn id(&self) -> EvidenceId {
        self.id
    }
    pub fn workspace_id(&self) -> WorkspaceId {
        self.workspace_id
    }
    pub fn title(&self) -> &str {
        self.definition.title()
    }
    pub fn description(&self) -> &str {
        self.definition.description()
    }
    pub fn collection_instructions(&self) -> &str {
        self.definition.collection_instructions()
    }
    pub fn status(&self) -> EvidenceStatus {
        self.status
    }
    pub fn mappings(&self) -> &[EvidenceControlMappingState] {
        &self.mappings
    }
    pub fn created_at(&self) -> DateTime<Utc> {
        self.created_at
    }
    pub fn updated_at(&self) -> DateTime<Utc> {
        self.updated_at
    }
}

fn normalize_mappings(
    mut mappings: Vec<EvidenceControlMappingState>,
) -> Result<Vec<EvidenceControlMappingState>, EvidenceAggregateError> {
    mappings.sort_unstable_by_key(|mapping| Uuid::from(mapping.control_id()));
    for pair in mappings.windows(2) {
        if pair[0].control_id() == pair[1].control_id() {
            return Err(EvidenceAggregateError::DuplicateControlMapping(
                pair[0].control_id(),
            ));
        }
    }
    Ok(mappings)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum EvidenceAggregateError {
    #[error("control mapping is duplicated")]
    DuplicateControlMapping(ControlId),
    #[error("persisted evidence snapshot is inconsistent")]
    InvalidRehydration,
    #[error("evidence replacement predates its creation")]
    InvalidReplacementTime,
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

    use chrono::{Duration, TimeZone, Utc};
    use uuid::Uuid;

    use super::{
        EvidenceAggregate, EvidenceAggregateError, EvidenceControlMappingState, EvidenceDefinition,
        EvidenceId, EvidenceStatus,
    };
    use crate::domain::{ControlId, DomainError, WorkspaceId};

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

    #[test]
    fn replacement_preserves_status_rules_and_rejects_time_before_creation() {
        let created_at = Utc.with_ymd_and_hms(2026, 8, 9, 12, 0, 0).unwrap();
        let mut evidence = EvidenceAggregate::define(
            EvidenceId::from(Uuid::new_v4()),
            WorkspaceId::from(Uuid::new_v4()),
            definition(),
            EvidenceStatus::Active,
            created_at,
        );

        evidence
            .replace(
                definition(),
                EvidenceStatus::Retired,
                created_at + Duration::seconds(1),
            )
            .unwrap();

        assert_eq!(evidence.status(), EvidenceStatus::Retired);
        assert!(matches!(
            evidence.replace(
                definition(),
                EvidenceStatus::Paused,
                created_at - Duration::seconds(1)
            ),
            Err(EvidenceAggregateError::InvalidReplacementTime)
        ));
    }

    #[test]
    fn mappings_are_sorted_and_duplicate_control_mappings_are_rejected() {
        let now = Utc.with_ymd_and_hms(2026, 8, 9, 12, 0, 0).unwrap();
        let first = ControlId::from(Uuid::from_u128(1));
        let second = ControlId::from(Uuid::from_u128(2));
        let mut evidence = EvidenceAggregate::define(
            EvidenceId::from(Uuid::new_v4()),
            WorkspaceId::from(Uuid::new_v4()),
            definition(),
            EvidenceStatus::Active,
            now,
        );

        evidence
            .replace_mappings(vec![mapping(second, now), mapping(first, now)])
            .unwrap();

        assert_eq!(
            evidence
                .mappings()
                .iter()
                .map(|mapping| mapping.control_id())
                .collect::<Vec<_>>(),
            vec![first, second]
        );
        assert!(matches!(
            evidence.replace_mappings(vec![mapping(first, now), mapping(first, now)]),
            Err(EvidenceAggregateError::DuplicateControlMapping(id)) if id == first
        ));
    }

    fn definition() -> EvidenceDefinition {
        EvidenceDefinition::new("Title".into(), "Description".into(), "Collect it".into())
            .into_result()
            .unwrap()
    }

    fn mapping(
        control_id: ControlId,
        created_at: chrono::DateTime<Utc>,
    ) -> EvidenceControlMappingState {
        EvidenceControlMappingState::new(control_id, "Supports the control".into(), created_at)
            .into_result()
            .unwrap()
    }
}
