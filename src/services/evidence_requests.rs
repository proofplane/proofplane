use std::sync::Arc;

use chrono::{DateTime, Utc};

use crate::{
    authentication::ActorContext,
    domain::{
        CreateEvidenceRequestPayload, EvidenceRequest, EvidenceRequestId,
        UpdateEvidenceRequestPayload,
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
        actor: ActorContext,
        request: CreateEvidenceRequestPayload,
    ) -> Result<EvidenceRequest, Error> {
        Ok(self
            .repository
            .in_actor_context(actor.workspace_id, actor.id, async move |context| {
                context.create_evidence_request(&request).await
            })
            .await?)
    }

    pub async fn get(
        &self,
        actor: ActorContext,
        id: EvidenceRequestId,
    ) -> Result<Option<EvidenceRequest>, Error> {
        Ok(self
            .repository
            .in_actor_context_read(actor.workspace_id, actor.id, async move |context| {
                context.get_evidence_request(id).await
            })
            .await?)
    }

    pub async fn list_by_workspace(
        &self,
        actor: ActorContext,
    ) -> Result<Vec<EvidenceRequest>, Error> {
        Ok(self
            .repository
            .in_actor_context_read(actor.workspace_id, actor.id, async |context| {
                context.list_evidence_requests().await
            })
            .await?)
    }

    pub async fn replace(
        &self,
        actor: ActorContext,
        id: EvidenceRequestId,
        update: UpdateEvidenceRequestPayload,
    ) -> Result<Option<EvidenceRequest>, Error> {
        Ok(self
            .repository
            .in_actor_context(actor.workspace_id, actor.id, async move |context| {
                context.replace_evidence_request(id, &update).await
            })
            .await?)
    }

    pub async fn list_due(
        &self,
        actor: ActorContext,
        now: DateTime<Utc>,
    ) -> Result<Vec<EvidenceRequest>, Error> {
        Ok(self
            .repository
            .in_actor_context_read(actor.workspace_id, actor.id, async move |context| {
                context.list_due_evidence_requests(now).await
            })
            .await?)
    }
}
