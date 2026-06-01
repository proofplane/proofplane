use std::sync::Arc;

use crate::{
    domain::{
        CreateEvidenceSubmissionPayload, EvidenceRequestId, EvidenceSubmission,
        EvidenceSubmissionDetail, EvidenceSubmissionId,
    },
    repository::Postgres,
    routes::authentication::ActorContext,
    services::Error,
};

pub struct EvidenceSubmissionService {
    repository: Arc<Postgres>,
}

impl Clone for EvidenceSubmissionService {
    fn clone(&self) -> Self {
        Self {
            repository: self.repository.clone(),
        }
    }
}

impl EvidenceSubmissionService {
    pub fn new(repository: Arc<Postgres>) -> Self {
        Self { repository }
    }

    pub async fn create(
        &self,
        actor: ActorContext,
        evidence_request_id: EvidenceRequestId,
        mut payload: CreateEvidenceSubmissionPayload,
    ) -> Result<Option<EvidenceSubmission>, Error> {
        payload.evidence_request_id = evidence_request_id;

        Ok(self
            .repository
            .in_actor_context(actor, async move |context| {
                context.create_evidence_submission(&payload).await
            })
            .await?)
    }

    pub async fn get(
        &self,
        actor: ActorContext,
        id: EvidenceSubmissionId,
    ) -> Result<Option<EvidenceSubmissionDetail>, Error> {
        Ok(self
            .repository
            .in_actor_context_read(actor, async move |context| {
                context.get_evidence_submission(id).await
            })
            .await?)
    }
}
