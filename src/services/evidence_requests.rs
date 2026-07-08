use std::sync::Arc;

use chrono::{DateTime, Utc};

use crate::{
    domain::{
        CreateEvidenceRequestPayload, EvidenceRequest, EvidenceRequestId,
        UpdateEvidenceRequestPayload,
    },
    repository::Postgres,
    services::Error,
};

use super::agent_connections::AgentConnectionContext;

#[derive(Clone)]
pub struct EvidenceRequestService {
    repository: Arc<Postgres>,
}

impl EvidenceRequestService {
    pub fn new(repository: Arc<Postgres>) -> Self {
        Self { repository }
    }

    pub async fn create(
        &self,
        connection: AgentConnectionContext,
        request: CreateEvidenceRequestPayload,
    ) -> Result<EvidenceRequest, Error> {
        Ok(self
            .repository
            .in_agent_connection_workspace_context(
                connection.workspace_id,
                connection.user_id,
                connection.connection_id,
                async move |context| context.create_evidence_request(&request).await,
            )
            .await?)
    }

    pub async fn get(
        &self,
        connection: AgentConnectionContext,
        id: EvidenceRequestId,
    ) -> Result<Option<EvidenceRequest>, Error> {
        Ok(self
            .repository
            .in_workspace_context_read(connection.workspace_id, async move |context| {
                context.get_evidence_request(id).await
            })
            .await?)
    }

    pub async fn list_by_workspace(
        &self,
        connection: AgentConnectionContext,
    ) -> Result<Vec<EvidenceRequest>, Error> {
        Ok(self
            .repository
            .in_workspace_context_read(connection.workspace_id, async |context| {
                context.list_evidence_requests().await
            })
            .await?)
    }

    pub async fn replace(
        &self,
        connection: AgentConnectionContext,
        id: EvidenceRequestId,
        update: UpdateEvidenceRequestPayload,
    ) -> Result<Option<EvidenceRequest>, Error> {
        Ok(self
            .repository
            .in_agent_connection_workspace_context(
                connection.workspace_id,
                connection.user_id,
                connection.connection_id,
                async move |context| context.replace_evidence_request(id, &update).await,
            )
            .await?)
    }

    pub async fn list_due(
        &self,
        connection: AgentConnectionContext,
        now: DateTime<Utc>,
    ) -> Result<Vec<EvidenceRequest>, Error> {
        Ok(self
            .repository
            .in_workspace_context_read(connection.workspace_id, async move |context| {
                context.list_due_evidence_requests(now).await
            })
            .await?)
    }
}
