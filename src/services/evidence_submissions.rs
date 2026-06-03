use std::sync::Arc;

use crate::{
    domain::{
        CreateEvidenceAttachmentPayload, CreateEvidenceSubmissionPayload,
        EvidenceAttachmentWithScan, EvidenceRequestId, EvidenceSubmission,
        EvidenceSubmissionDetail, EvidenceSubmissionId,
    },
    object_storage::{
        FilesystemObjectStore, ObjectKey, ObjectStore, PutObjectRequest, StorageError,
    },
    repository::Postgres,
    routes::authentication::ActorContext,
    services::Error,
};
use bytes::Bytes;
use futures_core::Stream;
use uuid::Uuid;

pub struct EvidenceSubmissionService {
    repository: Arc<Postgres>,
    object_store: Arc<FilesystemObjectStore>,
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
    pub object_key: String,
    pub checksum_sha256: String,
    pub checksum_crc32c: String,
}

impl EvidenceSubmissionService {
    pub fn new(repository: Arc<Postgres>, object_store: Arc<FilesystemObjectStore>) -> Self {
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

    pub async fn evidence_submission_exists(
        &self,
        actor: ActorContext,
        id: EvidenceSubmissionId,
    ) -> Result<bool, Error> {
        Ok(self
            .repository
            .in_actor_context_read(actor, async move |context| {
                context.evidence_submission_exists(id).await
            })
            .await?)
    }

    pub async fn upload_attachment<S>(
        &self,
        actor: ActorContext,
        submission_id: EvidenceSubmissionId,
        filename: String,
        content_type: String,
        chunks: S,
    ) -> Result<UploadEvidenceAttachmentPayload, Error>
    where
        S: Stream<Item = Result<Bytes, StorageError>> + Send,
    {
        let upload_id = Uuid::new_v4();
        let stable_prefix =
            format!("quarantine/evidence-submissions/{submission_id}/attachments/{upload_id}");
        let key = ObjectKey::new(actor.workspace_id, stable_prefix, &filename)?;

        let metadata = self
            .object_store
            .put_object(PutObjectRequest {
                key: key.clone(),
                content_type: content_type.clone(),
                chunks,
            })
            .await?;

        let content_length = i64::try_from(metadata.content_length).map_err(|_| {
            Error::Storage(crate::object_storage::StorageError::StreamRead {
                message: "file is too large".to_owned(),
                payload_too_large: true,
            })
        })?;

        Ok(UploadEvidenceAttachmentPayload {
            evidence_submission_id: submission_id,
            filename,
            content_type,
            content_length,
            object_key: key.to_string(),
            checksum_sha256: metadata.sha256,
            checksum_crc32c: String::new(),
        })
    }

    pub async fn delete_uploaded_attachment_object(&self, object_key: &str) -> Result<(), Error> {
        self.object_store
            .delete_object(ObjectKey::parse(object_key.to_owned())?)
            .await?;
        Ok(())
    }

    pub async fn create_attachment(
        &self,
        actor: ActorContext,
        submission_id: EvidenceSubmissionId,
        mut payload: UploadEvidenceAttachmentPayload,
    ) -> Result<EvidenceAttachmentWithScan, Error> {
        payload.evidence_submission_id = submission_id;

        let create_payload = CreateEvidenceAttachmentPayload {
            evidence_submission_id: submission_id,
            filename: payload.filename,
            content_type: payload.content_type,
            content_length: payload.content_length,
            object_key: payload.object_key.clone(),
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
            Ok(attachment) => Ok(attachment),
            Err(error) => {
                let _ = self
                    .delete_uploaded_attachment_object(&payload.object_key)
                    .await;
                Err(error.into())
            }
        }
    }
}
