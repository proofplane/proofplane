use std::sync::Arc;

use serde::Deserialize;
use uuid::Uuid;

use crate::{
    domain::EvidenceSubmissionId,
    object_storage::{ObjectKey, ObjectStore},
    observability::audit::{AuditActor, AuditClientType, AuditEvent, AuditObject, AuditOutcome},
    repository::{FinalizingSubmissionUploadWork, Postgres},
    worker::{RetryableWorkerError, WorkerMessage},
};

pub struct SubmissionFinalizationHandler<S> {
    repository: Arc<Postgres>,
    object_store: Arc<S>,
}

impl<S> Clone for SubmissionFinalizationHandler<S> {
    fn clone(&self) -> Self {
        Self {
            repository: self.repository.clone(),
            object_store: self.object_store.clone(),
        }
    }
}

impl<S> SubmissionFinalizationHandler<S> {
    pub fn new(repository: Arc<Postgres>, object_store: Arc<S>) -> Self {
        Self {
            repository,
            object_store,
        }
    }
}

impl<S> SubmissionFinalizationHandler<S>
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
                    "skipping invalid submission finalization message"
                );
                return Ok(());
            }
        };

        let Some(work) = self
            .repository
            .load_finalizing_submission_upload_work(
                payload.evidence_submission_id,
                payload.object_key.as_str(),
            )
            .await
            .map_err(retryable)?
        else {
            tracing::info!(
                evidence_submission_id = %payload.evidence_submission_id,
                "skipping duplicate or stale submission finalization message"
            );
            return Ok(());
        };

        tracing::debug!("finalizing submission");

        let final_key = final_submission_object_key(&work).map_err(retryable)?;
        self.object_store
            .copy_object(&payload.object_key, &final_key)
            .await
            .map_err(retryable)?;

        tracing::debug!("object copied");

        let updated = self
            .repository
            .mark_submission_uploaded(
                work.evidence_submission_id,
                payload.object_key.as_str(),
                final_key.as_str(),
            )
            .await
            .map_err(retryable)?;

        tracing::debug!("submission marked as uploaded in repository");

        if updated {
            emit_worker_finalization_audit(&work, message.request_id);
            self.object_store
                .delete_object(&payload.object_key)
                .await
                .inspect_err(|error| {
                    tracing::warn!(
                        error = %error,
                        "failed to delete quarantined object after finalization"
                    );
                })
                .ok();
        }

        Ok(())
    }
}

fn emit_worker_finalization_audit(work: &FinalizingSubmissionUploadWork, request_id: Option<Uuid>) {
    let mut event = AuditEvent::new(
        "evidence_submission_finalization.completed",
        AuditOutcome::Success,
        AuditActor::System { name: "worker" },
        AuditClientType::Worker,
        "handle_submission_finalization",
    )
    .workspace_id(work.workspace_id.into())
    .metadata("evidence_id", Uuid::from(work.evidence_id))
    .metadata(
        "evidence_submission_id",
        Uuid::from(work.evidence_submission_id),
    )
    .metadata("lifecycle_status", "uploaded")
    .object(AuditObject::new(
        "evidence_submission",
        work.evidence_submission_id.into(),
    ));
    if let Some(request_id) = request_id {
        event = event.request_id(request_id);
    }
    event.emit();
}

struct FinalizationRequestedPayload {
    evidence_submission_id: EvidenceSubmissionId,
    object_key: ObjectKey,
}

impl FinalizationRequestedPayload {
    fn try_from_message(message: &WorkerMessage) -> Option<Self> {
        if message.aggregate_type != "evidence_submission" {
            return None;
        }

        let dto = FinalizationRequestedPayloadDto::deserialize(&message.payload).ok()?;

        Some(Self {
            evidence_submission_id: Uuid::parse_str(&message.aggregate_id).ok()?.into(),
            object_key: ObjectKey::parse(dto.object_key).ok()?,
        })
    }
}

#[derive(Deserialize)]
struct FinalizationRequestedPayloadDto {
    object_key: String,
}

fn final_submission_object_key(
    work: &FinalizingSubmissionUploadWork,
) -> Result<ObjectKey, crate::object_storage::StorageError> {
    ObjectKey::new(
        work.workspace_id,
        format!(
            "evidence/{}/submissions/{}",
            work.evidence_id, work.evidence_submission_id
        ),
        &work.filename,
    )
}

fn retryable(error: impl ToString) -> RetryableWorkerError {
    RetryableWorkerError(error.to_string())
}

#[cfg(test)]
mod tests {
    use crate::domain::{SubmissionUploadStatus, WorkspaceId};

    use super::*;

    #[test]
    fn finalization_payload_takes_its_submission_from_the_aggregate_id() {
        let submission_id = Uuid::new_v4();
        let object_key = format!(
            "workspaces/{}/quarantine/evidence/{}/submissions/{}/manual.txt",
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4()
        );
        let mut message = WorkerMessage {
            message_id: "message-1".to_owned(),
            event_type: "submission.finalization_requested".to_owned(),
            aggregate_type: "evidence_submission".to_owned(),
            aggregate_id: submission_id.to_string(),
            request_id: None,
            payload: serde_json::json!({ "object_key": object_key }),
            delivery_attempt: Some(1),
        };

        let payload =
            FinalizationRequestedPayload::try_from_message(&message).expect("payload parses");
        assert_eq!(Uuid::from(payload.evidence_submission_id), submission_id);
        assert_eq!(payload.object_key.as_str(), object_key);

        message.aggregate_type = "unsupported_aggregate".to_owned();
        assert!(FinalizationRequestedPayload::try_from_message(&message).is_none());

        let mut malformed_aggregate = message.clone();
        malformed_aggregate.aggregate_type = "evidence_submission".to_owned();
        malformed_aggregate.aggregate_id = "not-a-uuid".to_owned();
        assert!(FinalizationRequestedPayload::try_from_message(&malformed_aggregate).is_none());
    }

    #[test]
    fn final_object_key_is_namespaced_by_evidence_and_submission() {
        let workspace_id = Uuid::new_v4();
        let evidence_id = Uuid::new_v4();
        let submission_id = Uuid::new_v4();
        let work = FinalizingSubmissionUploadWork {
            workspace_id: WorkspaceId::from(workspace_id),
            evidence_id: evidence_id.into(),
            evidence_submission_id: submission_id.into(),
            filename: "manual.txt".to_owned(),
            content_type: "text/plain".to_owned(),
            content_length: 5,
            object_key: "unused".to_owned(),
            checksum_sha256: "checksum".to_owned(),
            upload_status: SubmissionUploadStatus::Finalizing,
        };

        assert_eq!(
            final_submission_object_key(&work)
                .expect("key is valid")
                .as_str(),
            format!(
                "workspaces/{workspace_id}/evidence/{evidence_id}/submissions/{submission_id}/manual.txt"
            )
        );
    }
}
