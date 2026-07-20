use std::sync::Arc;

use crate::{
    domain::{CreateEvidencePayload, Evidence, EvidenceId, UpdateEvidencePayload},
    repository::Postgres,
    services::Error,
};

use super::agent_connections::AgentConnectionContext;

#[derive(Clone)]
pub struct EvidenceService {
    repository: Arc<Postgres>,
}

impl EvidenceService {
    pub fn new(repository: Arc<Postgres>) -> Self {
        Self { repository }
    }

    pub async fn create(
        &self,
        connection: AgentConnectionContext,
        payload: CreateEvidencePayload,
    ) -> Result<Evidence, Error> {
        Ok(self
            .repository
            .in_agent_connection_workspace_context(
                connection.workspace_id,
                connection.user_id,
                connection.connection_id,
                async move |context| context.create_evidence(&payload).await,
            )
            .await?)
    }

    pub async fn get(
        &self,
        connection: AgentConnectionContext,
        id: EvidenceId,
    ) -> Result<Option<Evidence>, Error> {
        Ok(self
            .repository
            .in_workspace_context_read(connection.workspace_id, async move |context| {
                context.get_evidence(id).await
            })
            .await?)
    }

    pub async fn list_by_workspace(
        &self,
        connection: AgentConnectionContext,
    ) -> Result<Vec<Evidence>, Error> {
        Ok(self
            .repository
            .in_workspace_context_read(connection.workspace_id, async |context| {
                context.list_evidence().await
            })
            .await?)
    }

    pub async fn replace(
        &self,
        connection: AgentConnectionContext,
        id: EvidenceId,
        update: UpdateEvidencePayload,
    ) -> Result<Option<Evidence>, Error> {
        Ok(self
            .repository
            .in_agent_connection_workspace_context(
                connection.workspace_id,
                connection.user_id,
                connection.connection_id,
                async move |context| context.replace_evidence(id, &update).await,
            )
            .await?)
    }
}
