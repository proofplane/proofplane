use std::{collections::HashSet, sync::Arc};

use chrono::Utc;
use uuid::Uuid;

use crate::{
    application::ExecutionMetadata,
    authentication::AgentConnectionContext,
    domain::{
        ControlId, PolicyAggregateError, PolicyControlMapping, PolicyControlMappingState,
        PolicyDefinition, PolicyId, WorkspacePermission,
    },
    projections::policy_projection::PolicyDetail,
    repository::{ConflictKind, Error as RepositoryError, Postgres},
};

#[derive(Debug, Clone)]
pub struct CreatePolicy {
    pub connection: AgentConnectionContext,
    pub name: String,
    pub description: Option<String>,
    pub control_ids: Vec<ControlId>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreatedPolicy {
    pub policy: PolicyDetail,
}
#[derive(Clone)]
pub struct CreatePolicyHandler {
    repository: Arc<Postgres>,
}
impl CreatePolicyHandler {
    pub fn new(repository: Arc<Postgres>) -> Self {
        Self { repository }
    }
    pub async fn handle(
        &self,
        command: CreatePolicy,
        _: ExecutionMetadata,
    ) -> Result<CreatedPolicy, PolicyCommandError> {
        authorize(command.connection)?;
        let definition = PolicyDefinition::new(command.name, command.description)
            .into_result()
            .map_err(PolicyCommandError::InvalidDefinition)?;
        let id = PolicyId::from(Uuid::new_v4());
        let requested = command.control_ids;
        let outcome = self
            .repository
            .in_agent_connection_workspace_context(
                command.connection.workspace_id,
                command.connection.user_id,
                command.connection.connection_id,
                async move |context| {
                    let present = context.existing_control_ids(&requested).await?;
                    let unknown = requested
                        .iter()
                        .copied()
                        .filter(|id| !present.contains(id))
                        .collect::<Vec<_>>();
                    if !unknown.is_empty() || has_duplicates(&requested) {
                        return Ok(CreateOutcome::Rejected { unknown });
                    }
                    let mut policy = crate::domain::PolicyAggregate::define(
                        id,
                        context.workspace_id,
                        definition,
                        Utc::now(),
                    );
                    policy
                        .replace_mappings(
                            requested
                                .iter()
                                .copied()
                                .map(|id| PolicyControlMappingState::new(id, Utc::now()))
                                .collect(),
                        )
                        .map_err(invariant)?;
                    context.policies().save(&policy).await?;
                    Ok(CreateOutcome::Created(Box::new(
                        context.get_policy_detail(id).await?.ok_or(
                            RepositoryError::InvariantViolation(
                                "created policy must be readable in its transaction",
                            ),
                        )?,
                    )))
                },
            )
            .await
            .map_err(PolicyCommandError::from)?;
        match outcome {
            CreateOutcome::Created(policy) => Ok(CreatedPolicy { policy: *policy }),
            CreateOutcome::Rejected { unknown } => Err(PolicyCommandError::Rejected {
                unknown,
                already_mapped: Vec::new(),
            }),
        }
    }
}
enum CreateOutcome {
    Created(Box<PolicyDetail>),
    Rejected { unknown: Vec<ControlId> },
}

#[derive(Debug, Clone)]
pub struct ReplacePolicy {
    pub connection: AgentConnectionContext,
    pub policy_id: PolicyId,
    pub name: String,
    pub description: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplacedPolicy {
    pub policy: PolicyDetail,
}
#[derive(Clone)]
pub struct ReplacePolicyHandler {
    repository: Arc<Postgres>,
}
impl ReplacePolicyHandler {
    pub fn new(repository: Arc<Postgres>) -> Self {
        Self { repository }
    }
    pub async fn handle(
        &self,
        command: ReplacePolicy,
        _: ExecutionMetadata,
    ) -> Result<ReplacedPolicy, PolicyCommandError> {
        authorize(command.connection)?;
        let definition = PolicyDefinition::new(command.name, command.description)
            .into_result()
            .map_err(PolicyCommandError::InvalidDefinition)?;
        let id = command.policy_id;
        let detail = self
            .repository
            .in_agent_connection_workspace_context(
                command.connection.workspace_id,
                command.connection.user_id,
                command.connection.connection_id,
                async move |context| {
                    let repository = context.policies();
                    let Some(mut policy) = repository.get(id).await? else {
                        return Ok(None);
                    };
                    policy.replace(definition, Utc::now()).map_err(invariant)?;
                    repository.save(&policy).await?;
                    context.get_policy_detail(id).await
                },
            )
            .await?;
        detail
            .map(|policy| ReplacedPolicy { policy })
            .ok_or(PolicyCommandError::Unavailable)
    }
}

#[derive(Debug, Clone)]
pub struct ArchivePolicy {
    pub connection: AgentConnectionContext,
    pub policy_id: PolicyId,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchivedPolicy {
    pub policy_id: PolicyId,
    pub archived_at: chrono::DateTime<Utc>,
}
#[derive(Clone)]
pub struct ArchivePolicyHandler {
    repository: Arc<Postgres>,
}
impl ArchivePolicyHandler {
    pub fn new(repository: Arc<Postgres>) -> Self {
        Self { repository }
    }
    pub async fn handle(
        &self,
        command: ArchivePolicy,
        _: ExecutionMetadata,
    ) -> Result<ArchivedPolicy, ArchivePolicyError> {
        authorize(command.connection).map_err(|_| ArchivePolicyError::Unavailable)?;
        let id = command.policy_id;
        let outcome = self
            .repository
            .in_agent_connection_workspace_context(
                command.connection.workspace_id,
                command.connection.user_id,
                command.connection.connection_id,
                async move |context| {
                    let repository = context.policies();
                    let Some(mut policy) = repository.get(id).await? else {
                        return Ok(None);
                    };
                    if context.policy_document_in_progress(id).await? {
                        return Ok(Some(Err(())));
                    }
                    policy.archive(Utc::now()).map_err(invariant)?;
                    let archived_at =
                        policy
                            .archived_at()
                            .ok_or(RepositoryError::InvariantViolation(
                                "archived policy needs timestamp",
                            ))?;
                    repository.save(&policy).await?;
                    Ok(Some(Ok(archived_at)))
                },
            )
            .await?;
        match outcome {
            Some(Ok(archived_at)) => Ok(ArchivedPolicy {
                policy_id: id,
                archived_at,
            }),
            Some(Err(())) => Err(ArchivePolicyError::DocumentInProgress),
            None => Err(ArchivePolicyError::Unavailable),
        }
    }
}

#[derive(Debug, Clone)]
pub struct AttachPolicyToControls {
    pub connection: AgentConnectionContext,
    pub policy_id: PolicyId,
    pub control_ids: Vec<ControlId>,
}
#[derive(Debug, Clone)]
pub struct DetachPolicyFromControls {
    pub connection: AgentConnectionContext,
    pub policy_id: PolicyId,
    pub control_ids: Vec<ControlId>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangedPolicyMappings {
    pub control_ids: Vec<ControlId>,
    pub mappings: Vec<PolicyControlMapping>,
}
#[derive(Clone)]
pub struct AttachPolicyToControlsHandler {
    repository: Arc<Postgres>,
}
#[derive(Clone)]
pub struct DetachPolicyFromControlsHandler {
    repository: Arc<Postgres>,
}
impl AttachPolicyToControlsHandler {
    pub fn new(repository: Arc<Postgres>) -> Self {
        Self { repository }
    }
    pub async fn handle(
        &self,
        command: AttachPolicyToControls,
        metadata: ExecutionMetadata,
    ) -> Result<ChangedPolicyMappings, PolicyCommandError> {
        change_mappings(
            &self.repository,
            command.connection,
            command.policy_id,
            command.control_ids,
            true,
            metadata,
        )
        .await
    }
}
impl DetachPolicyFromControlsHandler {
    pub fn new(repository: Arc<Postgres>) -> Self {
        Self { repository }
    }
    pub async fn handle(
        &self,
        command: DetachPolicyFromControls,
        metadata: ExecutionMetadata,
    ) -> Result<ChangedPolicyMappings, PolicyCommandError> {
        change_mappings(
            &self.repository,
            command.connection,
            command.policy_id,
            command.control_ids,
            false,
            metadata,
        )
        .await
    }
}
async fn change_mappings(
    repository: &Arc<Postgres>,
    connection: AgentConnectionContext,
    policy_id: PolicyId,
    requested: Vec<ControlId>,
    attach: bool,
    _: ExecutionMetadata,
) -> Result<ChangedPolicyMappings, PolicyCommandError> {
    authorize(connection)?;
    let response_control_ids = requested.clone();
    let outcome = repository
        .in_agent_connection_workspace_context(
            connection.workspace_id,
            connection.user_id,
            connection.connection_id,
            async move |context| {
                let repository = context.policies();
                let Some(mut policy) = repository.get(policy_id).await? else {
                    return Ok(None);
                };
                let existing = context.existing_control_ids(&requested).await?;
                let unknown = requested
                    .iter()
                    .copied()
                    .filter(|id| !existing.contains(id))
                    .collect::<Vec<_>>();
                let mapped = |id| policy.mappings().iter().any(|item| item.control_id() == id);
                let invalid = requested
                    .iter()
                    .copied()
                    .filter(|id| {
                        existing.contains(id) && (if attach { mapped(*id) } else { !mapped(*id) })
                    })
                    .collect::<Vec<_>>();
                if !unknown.is_empty() || !invalid.is_empty() || has_duplicates(&requested) {
                    return Ok(Some(Err((unknown, invalid))));
                };
                let next = if attach {
                    policy
                        .mappings()
                        .iter()
                        .cloned()
                        .chain(
                            requested
                                .iter()
                                .copied()
                                .map(|id| PolicyControlMappingState::new(id, Utc::now())),
                        )
                        .collect()
                } else {
                    policy
                        .mappings()
                        .iter()
                        .filter(|item| !requested.contains(&item.control_id()))
                        .cloned()
                        .collect()
                };
                policy.replace_mappings(next).map_err(invariant)?;
                repository.save(&policy).await?;
                let mut mappings = Vec::new();
                for id in &requested {
                    mappings.push(
                        context
                            .get_policy_control_mapping(policy_id, *id)
                            .await?
                            .ok_or(RepositoryError::InvariantViolation(
                                "saved policy mapping must be readable",
                            ))?,
                    );
                }
                Ok(Some(Ok(mappings)))
            },
        )
        .await?;
    match outcome {
        None => Err(PolicyCommandError::Unavailable),
        Some(Ok(mappings)) => Ok(ChangedPolicyMappings {
            control_ids: response_control_ids,
            mappings,
        }),
        Some(Err((unknown, already_mapped))) => Err(PolicyCommandError::Rejected {
            unknown,
            already_mapped,
        }),
    }
}
fn has_duplicates(ids: &[ControlId]) -> bool {
    let mut seen = HashSet::new();
    ids.iter().any(|id| !seen.insert(*id))
}
fn authorize(connection: AgentConnectionContext) -> Result<(), PolicyCommandError> {
    if connection
        .permissions
        .has(WorkspacePermission::WriteControls)
    {
        Ok(())
    } else {
        Err(PolicyCommandError::Unavailable)
    }
}
fn invariant(error: PolicyAggregateError) -> RepositoryError {
    match error {
        PolicyAggregateError::Archived => {
            RepositoryError::InvariantViolation("archived policy cannot be mutated")
        }
        _ => RepositoryError::InvariantViolation("policy snapshot is invalid"),
    }
}
#[derive(Debug, thiserror::Error)]
pub enum PolicyCommandError {
    #[error("policy is unavailable")]
    Unavailable,
    #[error("policy definition is invalid")]
    InvalidDefinition(Vec<crate::domain::DomainError>),
    #[error("control mappings are invalid")]
    Rejected {
        unknown: Vec<ControlId>,
        already_mapped: Vec<ControlId>,
    },
    #[error("policy name is taken")]
    NameTaken,
    #[error("repository error")]
    Repository(RepositoryError),
}
impl From<RepositoryError> for PolicyCommandError {
    fn from(error: RepositoryError) -> Self {
        match error {
            RepositoryError::Conflict(ConflictKind::PolicyNameTaken) => Self::NameTaken,
            other => Self::Repository(other),
        }
    }
}
#[derive(Debug, thiserror::Error)]
pub enum ArchivePolicyError {
    #[error("policy is unavailable")]
    Unavailable,
    #[error("policy document is in progress")]
    DocumentInProgress,
    #[error("repository error")]
    Repository(#[from] RepositoryError),
}
