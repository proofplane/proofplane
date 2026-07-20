use std::sync::Arc;

use thiserror::Error as ThisError;

use crate::{
    domain::{
        Control, ControlId, CreateControlPayload, CreateEvidenceControlMappingPayload,
        EvidenceControlMapping, EvidenceId, Framework, FrameworkId, FrameworkRequirement,
        FrameworkRequirementId, UpdateControlPayload,
    },
    repository::{ConflictKind, Error as RepositoryError, Postgres},
    services::Error,
};

use super::agent_connections::AgentConnectionContext;

#[derive(Debug, ThisError)]
pub enum ControlMutationError {
    #[error("a control with this code already exists in the workspace")]
    CodeTaken,

    #[error("framework_requirement_ids contains unknown ids")]
    InvalidFrameworkRequirementReferences,

    #[error("repository error")]
    Repository(RepositoryError),
}

impl From<RepositoryError> for ControlMutationError {
    fn from(error: RepositoryError) -> Self {
        match error {
            RepositoryError::Conflict(ConflictKind::ControlCodeTaken) => Self::CodeTaken,
            other => Self::Repository(other),
        }
    }
}

#[derive(Clone)]
pub struct ControlService {
    repository: Arc<Postgres>,
}

impl ControlService {
    pub fn new(repository: Arc<Postgres>) -> Self {
        Self { repository }
    }

    pub async fn list_frameworks(&self) -> Result<Vec<Framework>, Error> {
        Ok(self.repository.list_frameworks().await?)
    }

    pub async fn list_framework_requirements(
        &self,
        framework_id: FrameworkId,
    ) -> Result<Vec<FrameworkRequirement>, Error> {
        Ok(self
            .repository
            .list_framework_requirements(framework_id)
            .await?)
    }

    pub async fn create_control(
        &self,
        connection: AgentConnectionContext,
        payload: CreateControlPayload,
    ) -> Result<Control, ControlMutationError> {
        self.validate_framework_requirement_references(&payload.framework_requirement_ids)
            .await?;

        Ok(self
            .repository
            .in_agent_connection_workspace_context(
                connection.workspace_id,
                connection.user_id,
                connection.connection_id,
                async move |context| context.create_control(&payload).await,
            )
            .await?)
    }

    pub async fn list_controls(
        &self,
        connection: AgentConnectionContext,
    ) -> Result<Vec<Control>, Error> {
        Ok(self
            .repository
            .in_workspace_context_read(connection.workspace_id, async |context| {
                context.list_controls().await
            })
            .await?)
    }

    pub async fn get_control(
        &self,
        connection: AgentConnectionContext,
        control_id: ControlId,
    ) -> Result<Option<Control>, Error> {
        Ok(self
            .repository
            .in_workspace_context_read(connection.workspace_id, async move |context| {
                context.get_control(control_id).await
            })
            .await?)
    }

    pub async fn replace_control(
        &self,
        connection: AgentConnectionContext,
        control_id: ControlId,
        payload: UpdateControlPayload,
    ) -> Result<Option<Control>, ControlMutationError> {
        self.validate_framework_requirement_references(&payload.framework_requirement_ids)
            .await?;

        Ok(self
            .repository
            .in_agent_connection_workspace_context(
                connection.workspace_id,
                connection.user_id,
                connection.connection_id,
                async move |context| context.replace_control(control_id, &payload).await,
            )
            .await?)
    }

    pub async fn create_evidence_control_mapping(
        &self,
        connection: AgentConnectionContext,
        payload: CreateEvidenceControlMappingPayload,
    ) -> Result<Option<EvidenceControlMapping>, Error> {
        Ok(self
            .repository
            .in_agent_connection_workspace_context(
                connection.workspace_id,
                connection.user_id,
                connection.connection_id,
                async move |context| context.create_evidence_control_mapping(&payload).await,
            )
            .await?)
    }

    pub async fn list_evidence_control_mappings(
        &self,
        connection: AgentConnectionContext,
        evidence_id: EvidenceId,
    ) -> Result<Option<Vec<EvidenceControlMapping>>, Error> {
        Ok(self
            .repository
            .in_workspace_context_read(connection.workspace_id, async move |context| {
                context.list_evidence_control_mappings(evidence_id).await
            })
            .await?)
    }

    pub async fn delete_evidence_control_mapping(
        &self,
        connection: AgentConnectionContext,
        evidence_id: EvidenceId,
        control_id: ControlId,
    ) -> Result<bool, Error> {
        Ok(self
            .repository
            .in_agent_connection_workspace_context(
                connection.workspace_id,
                connection.user_id,
                connection.connection_id,
                async move |context| {
                    context
                        .delete_evidence_control_mapping(evidence_id, control_id)
                        .await
                },
            )
            .await?)
    }

    async fn validate_framework_requirement_references(
        &self,
        ids: &[FrameworkRequirementId],
    ) -> Result<(), ControlMutationError> {
        if self.repository.framework_requirements_exist(ids).await? {
            return Ok(());
        }

        Err(ControlMutationError::InvalidFrameworkRequirementReferences)
    }
}
