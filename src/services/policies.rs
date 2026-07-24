use std::sync::Arc;

use thiserror::Error as ThisError;

use crate::{
    domain::{
        optional_text, validate_policy_name, validate_unique_policy_control_ids, ControlId,
        CreateControlPolicyMappingsPayload, CreatePolicyControlMappingsPayload,
        CreatePolicyPayload, DeleteControlPolicyMappingsPayload,
        DeletePolicyControlMappingsPayload, DomainError, PolicyControlMapping, PolicyId,
        UpdatePolicyPayload,
    },
    projections::policy_projection::{PolicyCatalogEntry, PolicyDetail},
    repository::{ArchivePolicyResult, ConflictKind, Error as RepositoryError, Postgres},
    validation::Validation,
};

use super::{agent_connections::AgentConnectionContext, Error};

#[derive(Debug, ThisError)]
pub enum PolicyMutationError {
    #[error("policy validation failed")]
    Validation(Vec<DomainError>),

    #[error("an active policy with this name already exists in the workspace")]
    NameTaken,

    #[error("this control is already mapped to the policy")]
    MappingExists,

    #[error("control_ids contains unknown or cross-workspace ids")]
    InvalidControlReferences,

    #[error("repository error")]
    Repository(RepositoryError),
}

impl From<RepositoryError> for PolicyMutationError {
    fn from(error: RepositoryError) -> Self {
        match error {
            RepositoryError::Conflict(ConflictKind::PolicyNameTaken) => Self::NameTaken,
            RepositoryError::Conflict(ConflictKind::PolicyControlMappingExists) => {
                Self::MappingExists
            }
            RepositoryError::InvalidPolicyControlReferences => Self::InvalidControlReferences,
            other => Self::Repository(other),
        }
    }
}

#[derive(Debug, ThisError)]
pub enum AttachPolicyToControlsError {
    #[error("resource not found")]
    PolicyNotFound,

    #[error("control_ids contains unknown or already-attached ids")]
    Rejected {
        unknown: Vec<ControlId>,
        already_mapped: Vec<ControlId>,
    },

    #[error("repository error")]
    Repository(RepositoryError),
}

impl From<RepositoryError> for AttachPolicyToControlsError {
    fn from(error: RepositoryError) -> Self {
        match error {
            RepositoryError::BatchMapRejected(rejection) => Self::Rejected {
                unknown: rejection.unknown.into_iter().map(ControlId::from).collect(),
                already_mapped: rejection
                    .already_mapped
                    .into_iter()
                    .map(ControlId::from)
                    .collect(),
            },
            other => Self::Repository(other),
        }
    }
}

#[derive(Debug, ThisError)]
pub enum AttachControlToPoliciesError {
    #[error("resource not found")]
    ControlNotFound,

    #[error("policy_ids contains unknown, archived, or already-attached ids")]
    Rejected {
        unknown: Vec<PolicyId>,
        archived: Vec<PolicyId>,
        already_mapped: Vec<PolicyId>,
    },

    #[error("repository error")]
    Repository(RepositoryError),
}

impl From<RepositoryError> for AttachControlToPoliciesError {
    fn from(error: RepositoryError) -> Self {
        match error {
            RepositoryError::BatchMapRejected(rejection) => Self::Rejected {
                unknown: rejection.unknown.into_iter().map(PolicyId::from).collect(),
                archived: rejection.archived.into_iter().map(PolicyId::from).collect(),
                already_mapped: rejection
                    .already_mapped
                    .into_iter()
                    .map(PolicyId::from)
                    .collect(),
            },
            other => Self::Repository(other),
        }
    }
}

#[derive(Debug, ThisError)]
pub enum DetachPolicyFromControlsError {
    #[error("resource not found")]
    PolicyNotFound,

    #[error("control_ids contains unknown or not-mapped ids")]
    Rejected {
        unknown: Vec<ControlId>,
        not_mapped: Vec<ControlId>,
    },

    #[error("repository error")]
    Repository(RepositoryError),
}

impl From<RepositoryError> for DetachPolicyFromControlsError {
    fn from(error: RepositoryError) -> Self {
        match error {
            RepositoryError::BatchUnmapRejected(rejection) => Self::Rejected {
                unknown: rejection.unknown.into_iter().map(ControlId::from).collect(),
                not_mapped: rejection
                    .not_mapped
                    .into_iter()
                    .map(ControlId::from)
                    .collect(),
            },
            other => Self::Repository(other),
        }
    }
}

#[derive(Debug, ThisError)]
pub enum DetachControlFromPoliciesError {
    #[error("resource not found")]
    ControlNotFound,

    #[error("policy_ids contains unknown, archived, or not-mapped ids")]
    Rejected {
        unknown: Vec<PolicyId>,
        archived: Vec<PolicyId>,
        not_mapped: Vec<PolicyId>,
    },

    #[error("repository error")]
    Repository(RepositoryError),
}

impl From<RepositoryError> for DetachControlFromPoliciesError {
    fn from(error: RepositoryError) -> Self {
        match error {
            RepositoryError::BatchUnmapRejected(rejection) => Self::Rejected {
                unknown: rejection.unknown.into_iter().map(PolicyId::from).collect(),
                archived: rejection.archived.into_iter().map(PolicyId::from).collect(),
                not_mapped: rejection
                    .not_mapped
                    .into_iter()
                    .map(PolicyId::from)
                    .collect(),
            },
            other => Self::Repository(other),
        }
    }
}

#[derive(Clone)]
pub struct PolicyService {
    repository: Arc<Postgres>,
}

impl PolicyService {
    pub fn new(repository: Arc<Postgres>) -> Self {
        Self { repository }
    }

    pub async fn list(
        &self,
        connection: AgentConnectionContext,
    ) -> Result<Vec<PolicyCatalogEntry>, Error> {
        Ok(self
            .repository
            .in_workspace_context_read(connection.workspace_id, async |context| {
                context.list_policy_catalog().await
            })
            .await?)
    }

    pub async fn get(
        &self,
        connection: AgentConnectionContext,
        policy_id: PolicyId,
    ) -> Result<Option<PolicyDetail>, Error> {
        Ok(self
            .repository
            .in_workspace_context_read(connection.workspace_id, async move |context| {
                context.get_policy_detail(policy_id).await
            })
            .await?)
    }

    pub async fn create(
        &self,
        connection: AgentConnectionContext,
        payload: CreatePolicyPayload,
    ) -> Result<PolicyDetail, PolicyMutationError> {
        let payload = validate_create_payload(payload)?;
        Ok(self
            .repository
            .in_agent_connection_workspace_context(
                connection.workspace_id,
                connection.user_id,
                connection.connection_id,
                async move |context| {
                    let policy = context.create_policy(&payload).await?;
                    context.get_policy_detail(policy.id).await?.ok_or(
                        RepositoryError::InvariantViolation(
                            "created policy detail must be readable in transaction",
                        ),
                    )
                },
            )
            .await?)
    }

    pub async fn update(
        &self,
        connection: AgentConnectionContext,
        policy_id: PolicyId,
        payload: UpdatePolicyPayload,
    ) -> Result<Option<PolicyDetail>, PolicyMutationError> {
        let payload = validate_update_payload(payload)?;
        Ok(self
            .repository
            .in_agent_connection_workspace_context(
                connection.workspace_id,
                connection.user_id,
                connection.connection_id,
                async move |context| {
                    if context.update_policy(policy_id, &payload).await?.is_none() {
                        return Ok(None);
                    }
                    context
                        .get_policy_detail(policy_id)
                        .await?
                        .ok_or(RepositoryError::InvariantViolation(
                            "updated policy detail must be readable in transaction",
                        ))
                        .map(Some)
                },
            )
            .await?)
    }

    pub async fn attach_to_control(
        &self,
        connection: AgentConnectionContext,
        policy_id: PolicyId,
        control_id: ControlId,
    ) -> Result<Option<PolicyControlMapping>, PolicyMutationError> {
        Ok(self
            .repository
            .in_agent_connection_workspace_context(
                connection.workspace_id,
                connection.user_id,
                connection.connection_id,
                async move |context| {
                    context
                        .attach_policy_to_control(policy_id, control_id)
                        .await
                },
            )
            .await?)
    }

    pub async fn attach_to_controls(
        &self,
        connection: AgentConnectionContext,
        payload: CreatePolicyControlMappingsPayload,
    ) -> Result<Vec<ControlId>, AttachPolicyToControlsError> {
        self.repository
            .in_agent_connection_workspace_context(
                connection.workspace_id,
                connection.user_id,
                connection.connection_id,
                async move |context| context.attach_policy_to_controls(&payload).await,
            )
            .await
            .map_err(AttachPolicyToControlsError::from)?
            .ok_or(AttachPolicyToControlsError::PolicyNotFound)
    }

    pub async fn attach_control_to_policies(
        &self,
        connection: AgentConnectionContext,
        payload: CreateControlPolicyMappingsPayload,
    ) -> Result<Vec<PolicyId>, AttachControlToPoliciesError> {
        self.repository
            .in_agent_connection_workspace_context(
                connection.workspace_id,
                connection.user_id,
                connection.connection_id,
                async move |context| context.attach_control_to_policies(&payload).await,
            )
            .await
            .map_err(AttachControlToPoliciesError::from)?
            .ok_or(AttachControlToPoliciesError::ControlNotFound)
    }

    pub async fn detach_policy_from_controls(
        &self,
        connection: AgentConnectionContext,
        payload: DeletePolicyControlMappingsPayload,
    ) -> Result<Vec<ControlId>, DetachPolicyFromControlsError> {
        self.repository
            .in_agent_connection_workspace_context(
                connection.workspace_id,
                connection.user_id,
                connection.connection_id,
                async move |context| context.detach_policy_from_controls(&payload).await,
            )
            .await
            .map_err(DetachPolicyFromControlsError::from)?
            .ok_or(DetachPolicyFromControlsError::PolicyNotFound)
    }

    pub async fn detach_control_from_policies(
        &self,
        connection: AgentConnectionContext,
        payload: DeleteControlPolicyMappingsPayload,
    ) -> Result<Vec<PolicyId>, DetachControlFromPoliciesError> {
        self.repository
            .in_agent_connection_workspace_context(
                connection.workspace_id,
                connection.user_id,
                connection.connection_id,
                async move |context| context.detach_control_from_policies(&payload).await,
            )
            .await
            .map_err(DetachControlFromPoliciesError::from)?
            .ok_or(DetachControlFromPoliciesError::ControlNotFound)
    }

    pub async fn detach_from_control(
        &self,
        connection: AgentConnectionContext,
        policy_id: PolicyId,
        control_id: ControlId,
    ) -> Result<bool, PolicyMutationError> {
        Ok(self
            .repository
            .in_agent_connection_workspace_context(
                connection.workspace_id,
                connection.user_id,
                connection.connection_id,
                async move |context| {
                    context
                        .detach_policy_from_control(policy_id, control_id)
                        .await
                },
            )
            .await?)
    }

    pub async fn archive(
        &self,
        connection: AgentConnectionContext,
        policy_id: PolicyId,
    ) -> Result<ArchivePolicyResult, PolicyMutationError> {
        Ok(self
            .repository
            .in_agent_connection_workspace_context(
                connection.workspace_id,
                connection.user_id,
                connection.connection_id,
                async move |context| context.archive_policy(policy_id).await,
            )
            .await?)
    }
}

fn validate_create_payload(
    payload: CreatePolicyPayload,
) -> Result<CreatePolicyPayload, PolicyMutationError> {
    let name = validate_policy_name(payload.name);
    let description = optional_text("description", payload.description, 4_000);
    let control_ids = validate_unique_policy_control_ids(payload.control_ids);

    combine_create_validation(name, description, control_ids)
}

fn combine_create_validation(
    name: Validation<String, DomainError>,
    description: Validation<Option<String>, DomainError>,
    control_ids: Validation<Vec<ControlId>, DomainError>,
) -> Result<CreatePolicyPayload, PolicyMutationError> {
    let mut errors = Vec::new();
    let name = collect_validation(name, &mut errors);
    let description = collect_validation(description, &mut errors);
    let control_ids = collect_validation(control_ids, &mut errors);

    if !errors.is_empty() {
        return Err(PolicyMutationError::Validation(errors));
    }

    match (name, description, control_ids) {
        (Some(name), Some(description), Some(control_ids)) => Ok(CreatePolicyPayload {
            name,
            description,
            control_ids,
        }),
        _ => Err(PolicyMutationError::Repository(
            RepositoryError::InvariantViolation("valid policy payload fields must be present"),
        )),
    }
}

fn validate_update_payload(
    payload: UpdatePolicyPayload,
) -> Result<UpdatePolicyPayload, PolicyMutationError> {
    let mut errors = Vec::new();
    let name = collect_validation(validate_policy_name(payload.name), &mut errors);
    let description = collect_validation(
        optional_text("description", payload.description, 4_000),
        &mut errors,
    );

    if !errors.is_empty() {
        return Err(PolicyMutationError::Validation(errors));
    }

    match (name, description) {
        (Some(name), Some(description)) => Ok(UpdatePolicyPayload { name, description }),
        _ => Err(PolicyMutationError::Repository(
            RepositoryError::InvariantViolation("valid policy update fields must be present"),
        )),
    }
}

fn collect_validation<T>(
    validation: Validation<T, DomainError>,
    errors: &mut Vec<DomainError>,
) -> Option<T> {
    match validation {
        Validation::Valid(value) => Some(value),
        Validation::Invalid(mut validation_errors) => {
            errors.append(&mut validation_errors);
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::{validate_create_payload, PolicyMutationError};
    use crate::domain::{ControlId, CreatePolicyPayload, DomainError};

    #[test]
    fn create_validation_normalizes_text() {
        let payload = validate_create_payload(CreatePolicyPayload {
            name: "  Security policy  ".to_owned(),
            description: Some("  Describes safeguards.  ".to_owned()),
            control_ids: vec![],
        })
        .unwrap();

        assert_eq!(payload.name, "Security policy");
        assert_eq!(
            payload.description.as_deref(),
            Some("Describes safeguards.")
        );
    }

    #[test]
    fn create_validation_accumulates_field_errors() {
        let id = ControlId::from(Uuid::new_v4());
        let error = validate_create_payload(CreatePolicyPayload {
            name: " ".to_owned(),
            description: Some(" ".to_owned()),
            control_ids: vec![id, id],
        })
        .unwrap_err();

        assert!(matches!(
            error,
            PolicyMutationError::Validation(errors)
                if errors == vec![
                    DomainError::EmptyRequiredText { field: "name" },
                    DomainError::BlankOptionalText { field: "description" },
                    DomainError::DuplicatePolicyControlId,
                ]
        ));
    }
}
