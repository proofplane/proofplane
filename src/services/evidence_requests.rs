use std::sync::Arc;

use chrono::{DateTime, Utc};

use crate::{
    authentication::ApiTokenContext,
    domain::{
        CreateEvidenceRequestPayload, EvidenceRequest, EvidenceRequestId,
        UpdateEvidenceRequestPayload,
    },
    repository::Postgres,
    services::Error,
};

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
        token: ApiTokenContext,
        request: CreateEvidenceRequestPayload,
    ) -> Result<EvidenceRequest, Error> {
        Ok(self
            .repository
            .in_workspace_context(
                token.workspace_id,
                token.user_id,
                token.api_token_id,
                async move |context| context.create_evidence_request(&request).await,
            )
            .await?)
    }

    pub async fn get(
        &self,
        token: ApiTokenContext,
        id: EvidenceRequestId,
    ) -> Result<Option<EvidenceRequest>, Error> {
        Ok(self
            .repository
            .in_workspace_context_read(token.workspace_id, async move |context| {
                context.get_evidence_request(id).await
            })
            .await?)
    }

    pub async fn list_by_workspace(
        &self,
        token: ApiTokenContext,
    ) -> Result<Vec<EvidenceRequest>, Error> {
        Ok(self
            .repository
            .in_workspace_context_read(token.workspace_id, async |context| {
                context.list_evidence_requests().await
            })
            .await?)
    }

    pub async fn replace(
        &self,
        token: ApiTokenContext,
        id: EvidenceRequestId,
        update: UpdateEvidenceRequestPayload,
    ) -> Result<Option<EvidenceRequest>, Error> {
        Ok(self
            .repository
            .in_workspace_context(
                token.workspace_id,
                token.user_id,
                token.api_token_id,
                async move |context| context.replace_evidence_request(id, &update).await,
            )
            .await?)
    }

    pub async fn list_due(
        &self,
        token: ApiTokenContext,
        now: DateTime<Utc>,
    ) -> Result<Vec<EvidenceRequest>, Error> {
        Ok(self
            .repository
            .in_workspace_context_read(token.workspace_id, async move |context| {
                context.list_due_evidence_requests(now).await
            })
            .await?)
    }
}
