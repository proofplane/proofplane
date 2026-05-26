use std::sync::Arc;

use crate::{
    domain::{
        Control, ControlId, CreateControlPayload, CreateEvidenceRequestControlMappingPayload,
        EvidenceRequestControlMapping, EvidenceRequestId, Framework, FrameworkId,
        FrameworkRequirement, UpdateControlPayload, WorkspaceId,
    },
    repository::Postgres,
    services::Error,
};

pub struct ControlService {
    repository: Arc<Postgres>,
}

impl Clone for ControlService {
    fn clone(&self) -> Self {
        Self {
            repository: self.repository.clone(),
        }
    }
}

impl ControlService {
    pub fn new(repository: Arc<Postgres>) -> Self {
        Self { repository }
    }

    pub async fn list_frameworks(
        &self,
        workspace_id: WorkspaceId,
    ) -> Result<Vec<Framework>, Error> {
        Ok(self
            .repository
            .in_workspace(workspace_id, async move |context| {
                context.list_frameworks().await
            })
            .await?)
    }

    pub async fn list_framework_requirements(
        &self,
        workspace_id: WorkspaceId,
        framework_id: FrameworkId,
    ) -> Result<Vec<FrameworkRequirement>, Error> {
        Ok(self
            .repository
            .in_workspace(workspace_id, async move |context| {
                context.list_framework_requirements(framework_id).await
            })
            .await?)
    }

    pub async fn create_control(
        &self,
        workspace_id: WorkspaceId,
        payload: CreateControlPayload,
    ) -> Result<Option<Control>, Error> {
        Ok(self
            .repository
            .in_workspace(workspace_id, async move |context| {
                context.create_control(&payload).await
            })
            .await?)
    }

    pub async fn list_controls(&self, workspace_id: WorkspaceId) -> Result<Vec<Control>, Error> {
        Ok(self
            .repository
            .in_workspace(workspace_id, async move |context| {
                context.list_controls().await
            })
            .await?)
    }

    pub async fn get_control(
        &self,
        workspace_id: WorkspaceId,
        control_id: ControlId,
    ) -> Result<Option<Control>, Error> {
        Ok(self
            .repository
            .in_workspace(workspace_id, async move |context| {
                context.get_control(control_id).await
            })
            .await?)
    }

    pub async fn replace_control(
        &self,
        workspace_id: WorkspaceId,
        control_id: ControlId,
        payload: UpdateControlPayload,
    ) -> Result<Option<Control>, Error> {
        Ok(self
            .repository
            .in_workspace(workspace_id, async move |context| {
                context.replace_control(control_id, &payload).await
            })
            .await?)
    }

    pub async fn create_evidence_request_control_mapping(
        &self,
        workspace_id: WorkspaceId,
        payload: CreateEvidenceRequestControlMappingPayload,
    ) -> Result<Option<EvidenceRequestControlMapping>, Error> {
        Ok(self
            .repository
            .in_workspace(workspace_id, async move |context| {
                context
                    .create_evidence_request_control_mapping(&payload)
                    .await
            })
            .await?)
    }

    pub async fn list_evidence_request_control_mappings(
        &self,
        workspace_id: WorkspaceId,
        evidence_request_id: EvidenceRequestId,
    ) -> Result<Option<Vec<EvidenceRequestControlMapping>>, Error> {
        Ok(self
            .repository
            .in_workspace(workspace_id, async move |context| {
                context
                    .list_evidence_request_control_mappings(evidence_request_id)
                    .await
            })
            .await?)
    }

    pub async fn delete_evidence_request_control_mapping(
        &self,
        workspace_id: WorkspaceId,
        evidence_request_id: EvidenceRequestId,
        control_id: ControlId,
    ) -> Result<bool, Error> {
        Ok(self
            .repository
            .in_workspace(workspace_id, async move |context| {
                context
                    .delete_evidence_request_control_mapping(evidence_request_id, control_id)
                    .await
            })
            .await?)
    }
}
