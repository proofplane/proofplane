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
        let mut definition_errors = Vec::new();
        let definition =
            match PolicyDefinition::new(command.name, command.description).into_result() {
                Ok(definition) => Some(definition),
                Err(mut errors) => {
                    definition_errors.append(&mut errors);
                    None
                }
            };
        if has_duplicates(&command.control_ids) {
            definition_errors.push(crate::domain::DomainError::DuplicatePolicyControlId);
        }
        if !definition_errors.is_empty() {
            return Err(PolicyCommandError::InvalidDefinition(definition_errors));
        }
        let definition = definition.ok_or(PolicyCommandError::Repository(
            RepositoryError::InvariantViolation("validated policy definition must be present"),
        ))?;
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
                    if !unknown.is_empty() {
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
                    if policy.archived_at().is_some() {
                        return Ok(None);
                    }
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
                    if policy.archived_at().is_some() {
                        return Ok(None);
                    }
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

#[derive(Debug, Clone)]
pub struct AttachControlToPolicies {
    pub connection: AgentConnectionContext,
    pub control_id: ControlId,
    pub policy_ids: Vec<PolicyId>,
}

#[derive(Debug, Clone)]
pub struct DetachControlFromPolicies {
    pub connection: AgentConnectionContext,
    pub control_id: ControlId,
    pub policy_ids: Vec<PolicyId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangedControlPolicies {
    pub policy_ids: Vec<PolicyId>,
}

#[derive(Clone)]
pub struct AttachControlToPoliciesHandler {
    repository: Arc<Postgres>,
}

#[derive(Clone)]
pub struct DetachControlFromPoliciesHandler {
    repository: Arc<Postgres>,
}

impl AttachControlToPoliciesHandler {
    pub fn new(repository: Arc<Postgres>) -> Self {
        Self { repository }
    }

    pub async fn handle(
        &self,
        command: AttachControlToPolicies,
        metadata: ExecutionMetadata,
    ) -> Result<ChangedControlPolicies, ControlPolicyCommandError> {
        change_control_policies(
            &self.repository,
            command.connection,
            command.control_id,
            command.policy_ids,
            true,
            metadata,
        )
        .await
    }
}

impl DetachControlFromPoliciesHandler {
    pub fn new(repository: Arc<Postgres>) -> Self {
        Self { repository }
    }

    pub async fn handle(
        &self,
        command: DetachControlFromPolicies,
        metadata: ExecutionMetadata,
    ) -> Result<ChangedControlPolicies, ControlPolicyCommandError> {
        change_control_policies(
            &self.repository,
            command.connection,
            command.control_id,
            command.policy_ids,
            false,
            metadata,
        )
        .await
    }
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
                if policy.archived_at().is_some() {
                    return Ok(None);
                }
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
                if attach {
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

async fn change_control_policies(
    repository: &Arc<Postgres>,
    connection: AgentConnectionContext,
    control_id: ControlId,
    requested: Vec<PolicyId>,
    attach: bool,
    _: ExecutionMetadata,
) -> Result<ChangedControlPolicies, ControlPolicyCommandError> {
    authorize(connection).map_err(|_| ControlPolicyCommandError::Unavailable)?;
    let response_policy_ids = requested.clone();
    let outcome = repository
        .in_agent_connection_workspace_context(
            connection.workspace_id,
            connection.user_id,
            connection.connection_id,
            async move |context| {
                if !context
                    .existing_control_ids(&[control_id])
                    .await?
                    .contains(&control_id)
                {
                    return Ok(ControlPolicyOutcome::ControlUnavailable);
                }

                let policy_repository = context.policies();
                let mut lock_order = requested.clone();
                lock_order.sort_unstable_by_key(|id| Uuid::from(*id));
                let mut policies = Vec::with_capacity(lock_order.len());
                let mut unknown = Vec::new();
                let mut archived = Vec::new();
                let mut invalid = Vec::new();

                for policy_id in lock_order {
                    let Some(policy) = policy_repository.get(policy_id).await? else {
                        unknown.push(policy_id);
                        continue;
                    };
                    if policy.archived_at().is_some() {
                        archived.push(policy_id);
                        continue;
                    }
                    let mapped = policy
                        .mappings()
                        .iter()
                        .any(|mapping| mapping.control_id() == control_id);
                    if (attach && mapped) || (!attach && !mapped) {
                        invalid.push(policy_id);
                    }
                    policies.push(policy);
                }

                if !unknown.is_empty()
                    || !archived.is_empty()
                    || !invalid.is_empty()
                    || has_policy_duplicates(&requested)
                {
                    return Ok(ControlPolicyOutcome::Rejected {
                        unknown,
                        archived,
                        invalid,
                    });
                }

                for mut policy in policies {
                    let next = if attach {
                        policy
                            .mappings()
                            .iter()
                            .cloned()
                            .chain(std::iter::once(PolicyControlMappingState::new(
                                control_id,
                                Utc::now(),
                            )))
                            .collect()
                    } else {
                        policy
                            .mappings()
                            .iter()
                            .filter(|mapping| mapping.control_id() != control_id)
                            .cloned()
                            .collect()
                    };
                    policy.replace_mappings(next).map_err(invariant)?;
                    policy_repository.save(&policy).await?;
                }

                Ok(ControlPolicyOutcome::Changed)
            },
        )
        .await?;

    match outcome {
        ControlPolicyOutcome::Changed => Ok(ChangedControlPolicies {
            policy_ids: response_policy_ids,
        }),
        ControlPolicyOutcome::ControlUnavailable => Err(ControlPolicyCommandError::Unavailable),
        ControlPolicyOutcome::Rejected {
            unknown,
            archived,
            invalid,
        } => Err(ControlPolicyCommandError::Rejected {
            unknown,
            archived,
            invalid,
        }),
    }
}

enum ControlPolicyOutcome {
    Changed,
    ControlUnavailable,
    Rejected {
        unknown: Vec<PolicyId>,
        archived: Vec<PolicyId>,
        invalid: Vec<PolicyId>,
    },
}

fn has_duplicates(ids: &[ControlId]) -> bool {
    let mut seen = HashSet::new();
    ids.iter().any(|id| !seen.insert(*id))
}

fn has_policy_duplicates(ids: &[PolicyId]) -> bool {
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

#[derive(Debug, thiserror::Error)]
pub enum ControlPolicyCommandError {
    #[error("control is unavailable")]
    Unavailable,
    #[error("policy mappings are invalid")]
    Rejected {
        unknown: Vec<PolicyId>,
        archived: Vec<PolicyId>,
        invalid: Vec<PolicyId>,
    },
    #[error("repository error")]
    Repository(#[from] RepositoryError),
}
