use std::sync::Arc;

use serde::Deserialize;
use uuid::Uuid;

use crate::{
    domain::{EvidenceAttachmentId, EvidenceSubmissionId},
    object_storage::{ObjectKey, ObjectStore},
    observability::audit::{AuditActor, AuditClientType, AuditEvent, AuditObject, AuditOutcome},
    repository::{FinalizingAttachmentUploadWork, Postgres},
    worker::{RetryableWorkerError, WorkerMessage},
};

pub struct AttachmentFinalizationHandler<S> {
    repository: Arc<Postgres>,
    object_store: Arc<S>,
}

impl<S> Clone for AttachmentFinalizationHandler<S> {
    fn clone(&self) -> Self {
        Self {
            repository: self.repository.clone(),
            object_store: self.object_store.clone(),
        }
    }
}

impl<S> AttachmentFinalizationHandler<S> {
    pub fn new(repository: Arc<Postgres>, object_store: Arc<S>) -> Self {
        Self {
            repository,
            object_store,
        }
    }
}

impl<S> AttachmentFinalizationHandler<S>
where
    S: ObjectStore + Send + Sync,
{
    pub async fn handle_finalization_requested(
        &self,
        message: WorkerMessage,
    ) -> Result<(), RetryableWorkerError> {
        let payload = match FinalizationRequestedPayload::try_from_message(&message) {
            Some(payload) => payload,
            None => {
                tracing::warn!(
                    message_id = %message.message_id,
                    "skipping invalid attachment finalization message"
                );
                return Ok(());
            }
        };

        let Some(work) = self
            .repository
            .load_finalizing_attachment_upload_work(
                payload.evidence_attachment_id,
                payload.evidence_submission_id,
                payload.object_key.as_str(),
            )
            .await
            .map_err(retryable)?
        else {
            tracing::info!(
                evidence_attachment_id = %payload.evidence_attachment_id,
                "skipping duplicate or stale attachment finalization message"
            );
            return Ok(());
        };

        tracing::debug!("finalizing attachment");

        let final_key = final_attachment_object_key(&work).map_err(retryable)?;
        self.object_store
            .copy_object(&payload.object_key, &final_key)
            .await
            .map_err(retryable)?;

        tracing::debug!("object copied");

        let updated = self
            .repository
            .mark_attachment_uploaded(
                work.evidence_attachment_id,
                payload.object_key.as_str(),
                final_key.as_str(),
            )
            .await
            .map_err(retryable)?;

        tracing::debug!("attaachment marked as uploaded in repository");

        if updated {
            emit_worker_finalization_audit(&work, message.request_id);
            self.object_store
                .delete_object(&payload.object_key)
                .await
                .inspect_err(|error| {
                    tracing::warn!(
                        error = %error,
                        "failed to delete quarantined attachment object after finalization"
                    );
                })
                .ok();
        }

        Ok(())
    }
}

fn emit_worker_finalization_audit(work: &FinalizingAttachmentUploadWork, request_id: Option<Uuid>) {
    let mut event = AuditEvent::new(
        "evidence_attachment_finalization.completed",
        AuditOutcome::Success,
        AuditActor::System { name: "worker" },
        AuditClientType::Worker,
        "handle_attachment_finalization",
    )
    .workspace_id(work.workspace_id.into())
    .evidence_submission_id(work.evidence_submission_id.into())
    .evidence_attachment_id(work.evidence_attachment_id.into())
    .lifecycle_status("uploaded")
    .object(AuditObject::new(
        "evidence_attachment",
        work.evidence_attachment_id.into(),
    ));
    if let Some(request_id) = request_id {
        event = event.request_id(request_id);
    }
    event.emit();
}

struct FinalizationRequestedPayload {
    evidence_attachment_id: EvidenceAttachmentId,
    evidence_submission_id: EvidenceSubmissionId,
    object_key: ObjectKey,
}

impl FinalizationRequestedPayload {
    fn try_from_message(message: &WorkerMessage) -> Option<Self> {
        if message.aggregate_type != "evidence_attachment" {
            return None;
        }

        let dto = FinalizationRequestedPayloadDto::deserialize(&message.payload).ok()?;

        Some(Self {
            evidence_attachment_id: Uuid::parse_str(&message.aggregate_id).ok()?.into(),
            evidence_submission_id: Uuid::parse_str(&dto.evidence_submission_id).ok()?.into(),
            object_key: ObjectKey::parse(dto.object_key).ok()?,
        })
    }
}

#[derive(Deserialize)]
struct FinalizationRequestedPayloadDto {
    evidence_submission_id: String,
    object_key: String,
}

fn final_attachment_object_key(
    work: &FinalizingAttachmentUploadWork,
) -> Result<ObjectKey, crate::object_storage::StorageError> {
    ObjectKey::new(
        work.workspace_id,
        format!(
            "evidence-submissions/{}/attachments/{}",
            work.evidence_submission_id, work.evidence_attachment_id
        ),
        &work.filename,
    )
}

fn retryable(error: impl ToString) -> RetryableWorkerError {
    RetryableWorkerError(error.to_string())
}

#[cfg(test)]
mod tests {
    use crate::domain::{AttachmentUploadStatus, WorkspaceId};

    use super::*;

    #[test]
    fn finalization_payload_parses_valid_message_and_rejects_invalid_message() {
        let attachment_id = Uuid::new_v4();
        let submission_id = Uuid::new_v4();
        let object_key = format!(
            "workspaces/{}/quarantine/evidence-submissions/{submission_id}/attachments/upload/manual.txt",
            Uuid::new_v4()
        );
        let mut message = WorkerMessage {
            message_id: "message-1".to_owned(),
            event_type: "attachment.finalization_requested".to_owned(),
            aggregate_type: "evidence_attachment".to_owned(),
            aggregate_id: attachment_id.to_string(),
            request_id: None,
            payload: serde_json::json!({
                "evidence_submission_id": submission_id.to_string(),
                "object_key": object_key,
            }),
            delivery_attempt: Some(1),
        };

        let payload =
            FinalizationRequestedPayload::try_from_message(&message).expect("payload parses");
        assert_eq!(Uuid::from(payload.evidence_attachment_id), attachment_id);
        assert_eq!(Uuid::from(payload.evidence_submission_id), submission_id);

        message.aggregate_type = "evidence_submission".to_owned();
        assert!(FinalizationRequestedPayload::try_from_message(&message).is_none());
    }

    #[test]
    fn final_attachment_key_uses_stable_attachment_path() {
        let workspace_id = Uuid::new_v4();
        let submission_id = Uuid::new_v4();
        let attachment_id = Uuid::new_v4();
        let work = FinalizingAttachmentUploadWork {
            workspace_id: WorkspaceId::from(workspace_id),
            evidence_submission_id: submission_id.into(),
            evidence_attachment_id: attachment_id.into(),
            filename: "manual.txt".to_owned(),
            content_type: "text/plain".to_owned(),
            content_length: 5,
            object_key: "unused".to_owned(),
            checksum_sha256: "checksum".to_owned(),
            upload_status: AttachmentUploadStatus::Finalizing,
        };

        assert_eq!(
            final_attachment_object_key(&work)
                .expect("key is valid")
                .as_str(),
            format!(
                "workspaces/{workspace_id}/evidence-submissions/{submission_id}/attachments/{attachment_id}/manual.txt"
            )
        );
    }
}
