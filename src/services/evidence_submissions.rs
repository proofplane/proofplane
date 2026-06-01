use std::sync::Arc;

use crate::{
    domain::{
        CreateEvidenceAttachmentPayload, CreateEvidenceSubmissionPayload,
        EvidenceAttachmentWithScan, EvidenceRequestId, EvidenceSubmission,
        EvidenceSubmissionDetail, EvidenceSubmissionId,
    },
    object_storage::{ObjectKey, ObjectStore, PutObjectRequest},
    repository::Postgres,
    routes::authentication::ActorContext,
    services::Error,
};
use uuid::Uuid;

pub struct EvidenceSubmissionService {
    repository: Arc<Postgres>,
    object_store: Arc<dyn ObjectStore>,
}

impl Clone for EvidenceSubmissionService {
    fn clone(&self) -> Self {
        Self {
            repository: self.repository.clone(),
            object_store: self.object_store.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UploadEvidenceAttachmentPayload {
    pub evidence_submission_id: EvidenceSubmissionId,
    pub filename: String,
    pub content_type: String,
    pub content_length: i64,
    pub bytes: Vec<u8>,
    pub checksum_sha256: String,
    pub checksum_crc32c: String,
}

impl EvidenceSubmissionService {
    pub fn new(repository: Arc<Postgres>, object_store: Arc<dyn ObjectStore>) -> Self {
        Self {
            repository,
            object_store,
        }
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

    pub async fn upload_attachment(
        &self,
        actor: ActorContext,
        submission_id: EvidenceSubmissionId,
        mut payload: UploadEvidenceAttachmentPayload,
    ) -> Result<Option<EvidenceAttachmentWithScan>, Error> {
        payload.evidence_submission_id = submission_id;

        let upload_id = Uuid::new_v4();
        let stable_prefix =
            format!("quarantine/evidence-submissions/{submission_id}/attachments/{upload_id}");
        let key = ObjectKey::new(actor.workspace_id, stable_prefix, &payload.filename)?;

        self.object_store
            .put_object(PutObjectRequest {
                key: key.clone(),
                content_type: payload.content_type.clone(),
                bytes: payload.bytes,
            })
            .await?;

        let create_payload = CreateEvidenceAttachmentPayload {
            evidence_submission_id: submission_id,
            filename: payload.filename,
            content_type: payload.content_type,
            content_length: payload.content_length,
            object_key: key.to_string(),
            checksum_sha256: payload.checksum_sha256,
            checksum_crc32c: payload.checksum_crc32c,
        };

        let result = self
            .repository
            .in_actor_context(actor, async move |context| {
                context.create_evidence_attachment(&create_payload).await
            })
            .await;

        match result {
            Ok(Some(attachment)) => Ok(Some(attachment)),
            Ok(None) => {
                let _ = self.object_store.delete_object(key).await;
                Ok(None)
            }
            Err(error) => {
                let _ = self.object_store.delete_object(key).await;
                Err(error.into())
            }
        }
    }
}
