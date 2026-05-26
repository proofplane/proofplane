use std::sync::Arc;

use chrono::{DateTime, Utc};

use crate::{
    domain::{
        CreateEvidenceRequestPayload, EvidenceRequest, EvidenceRequestId,
        UpdateEvidenceRequestPayload, WorkspaceId,
    },
    repository::Postgres,
    services::Error,
};

pub struct EvidenceRequestService {
    repository: Arc<Postgres>,
}

impl Clone for EvidenceRequestService {
    fn clone(&self) -> Self {
        Self {
            repository: self.repository.clone(),
        }
    }
}

impl EvidenceRequestService {
    pub fn new(repository: Arc<Postgres>) -> Self {
        Self { repository }
    }

    pub async fn create(
        &self,
        workspace_id: WorkspaceId,
        request: CreateEvidenceRequestPayload,
    ) -> Result<EvidenceRequest, Error> {
        Ok(self
            .repository
            .in_workspace(workspace_id, async move |context| {
                context.create_evidence_request(&request).await
            })
            .await?)
    }

    pub async fn get(
        &self,
        workspace_id: WorkspaceId,
        id: EvidenceRequestId,
    ) -> Result<Option<EvidenceRequest>, Error> {
        Ok(self
            .repository
            .in_workspace(workspace_id, async move |context| {
                context.get_evidence_request(id).await
            })
            .await?)
    }

    pub async fn list_by_workspace(
        &self,
        workspace_id: WorkspaceId,
    ) -> Result<Vec<EvidenceRequest>, Error> {
        Ok(self
            .repository
            .in_workspace(workspace_id, async move |context| {
                context.list_evidence_requests().await
            })
            .await?)
    }

    pub async fn replace(
        &self,
        workspace_id: WorkspaceId,
        id: EvidenceRequestId,
        update: UpdateEvidenceRequestPayload,
    ) -> Result<Option<EvidenceRequest>, Error> {
        Ok(self
            .repository
            .in_workspace(workspace_id, async move |context| {
                context.replace_evidence_request(id, &update).await
            })
            .await?)
    }

    pub async fn list_due(
        &self,
        workspace_id: WorkspaceId,
        now: DateTime<Utc>,
    ) -> Result<Vec<EvidenceRequest>, Error> {
        Ok(self
            .repository
            .in_workspace(workspace_id, async move |context| {
                context.list_due_evidence_requests(now).await
            })
            .await?)
    }
}
