use std::sync::Arc;

use serde::Deserialize;
use uuid::Uuid;

use crate::{
    domain::{EvidenceAttachmentId, EvidenceSubmissionId},
    object_storage::{ObjectKey, ObjectStore},
    repository::{AttachmentFinalizationRepository, FinalizingAttachmentUploadWork},
    worker::{RetryableWorkerError, WorkerMessage},
};

pub struct AttachmentFinalizationHandler<R, S> {
    repository: Arc<R>,
    object_store: Arc<S>,
}

impl<R, S> Clone for AttachmentFinalizationHandler<R, S> {
    fn clone(&self) -> Self {
        Self {
            repository: self.repository.clone(),
            object_store: self.object_store.clone(),
        }
    }
}

impl<R, S> AttachmentFinalizationHandler<R, S> {
    pub fn new(repository: Arc<R>, object_store: Arc<S>) -> Self {
        Self {
            repository,
            object_store,
        }
    }
}

impl<R, S> AttachmentFinalizationHandler<R, S>
where
    R: AttachmentFinalizationRepository,
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
                    "acknowledging invalid attachment finalization message"
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
                "acknowledging duplicate or stale attachment finalization message"
            );
            return Ok(());
        };

        let final_key = final_attachment_object_key(&work).map_err(retryable)?;
        self.object_store
            .copy_object(payload.object_key.clone(), final_key.clone())
            .await
            .map_err(retryable)?;

        let updated = self
            .repository
            .mark_attachment_uploaded(
                work.evidence_attachment_id,
                payload.object_key.as_str(),
                final_key.as_str(),
            )
            .await
            .map_err(retryable)?;

        if updated {
            self.object_store
                .delete_object(payload.object_key)
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

        let dto =
            serde_json::from_value::<FinalizationRequestedPayloadDto>(message.payload.clone())
                .ok()?;

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
    use std::sync::Mutex;

    use async_trait::async_trait;
    use bytes::Bytes;
    use futures_core::Stream;

    use crate::{
        domain::{AttachmentUploadStatus, WorkspaceId},
        object_storage::{ObjectMetadata, ObjectStream, PutObjectRequest, StorageError},
        repository::Error as RepositoryError,
    };

    use super::*;

    #[tokio::test]
    async fn finalization_copies_marks_uploaded_and_deletes_quarantine() {
        let fixture = Fixture::new();

        fixture
            .handler()
            .handle_finalization_requested(fixture.message())
            .await
            .expect("finalization succeeds");

        let state = fixture.store.state.lock().unwrap();
        assert_eq!(state.copied.len(), 1);
        assert_eq!(state.deleted, vec![fixture.object_key.clone()]);
        drop(state);
        assert_eq!(fixture.repository.state.lock().unwrap().uploaded.len(), 1);
    }

    #[tokio::test]
    async fn invalid_and_stale_messages_are_acknowledged_noops() {
        let fixture = Fixture::new();
        let mut invalid = fixture.message();
        invalid.aggregate_id = "not-a-uuid".to_owned();
        fixture
            .handler()
            .handle_finalization_requested(invalid)
            .await
            .expect("invalid message is acknowledged");

        fixture.repository.state.lock().unwrap().work = None;
        fixture
            .handler()
            .handle_finalization_requested(fixture.message())
            .await
            .expect("stale message is acknowledged");

        assert!(fixture.store.state.lock().unwrap().copied.is_empty());
    }

    #[tokio::test]
    async fn copy_and_database_failures_are_retryable() {
        let fixture = Fixture::new();
        fixture.store.state.lock().unwrap().copy_fails = true;
        assert!(fixture
            .handler()
            .handle_finalization_requested(fixture.message())
            .await
            .is_err());
        assert!(fixture.repository.state.lock().unwrap().uploaded.is_empty());

        fixture.store.state.lock().unwrap().copy_fails = false;
        fixture.repository.state.lock().unwrap().update_fails = true;
        assert!(fixture
            .handler()
            .handle_finalization_requested(fixture.message())
            .await
            .is_err());
        assert_eq!(fixture.store.state.lock().unwrap().copied.len(), 2);
    }

    #[tokio::test]
    async fn deletion_failure_is_best_effort_after_database_success() {
        let fixture = Fixture::new();
        fixture.store.state.lock().unwrap().delete_fails = true;

        fixture
            .handler()
            .handle_finalization_requested(fixture.message())
            .await
            .expect("delete failure is acknowledged");

        assert_eq!(fixture.repository.state.lock().unwrap().uploaded.len(), 1);
    }

    struct Fixture {
        attachment_id: Uuid,
        submission_id: Uuid,
        object_key: String,
        repository: Arc<FakeRepository>,
        store: Arc<FakeObjectStore>,
    }

    impl Fixture {
        fn new() -> Self {
            let attachment_id = Uuid::new_v4();
            let submission_id = Uuid::new_v4();
            let workspace_id = Uuid::new_v4();
            let object_key = format!(
                "workspaces/{workspace_id}/quarantine/evidence-submissions/{submission_id}/attachments/{attachment_id}/manual.txt"
            );
            let work = FinalizingAttachmentUploadWork {
                workspace_id: WorkspaceId::from(workspace_id),
                evidence_submission_id: submission_id.into(),
                evidence_attachment_id: attachment_id.into(),
                filename: "manual.txt".to_owned(),
                content_type: "text/plain".to_owned(),
                content_length: 5,
                object_key: object_key.clone(),
                checksum_sha256: "checksum".to_owned(),
                upload_status: AttachmentUploadStatus::Finalizing,
            };

            Self {
                attachment_id,
                submission_id,
                object_key,
                repository: Arc::new(FakeRepository {
                    state: Mutex::new(FakeRepositoryState {
                        work: Some(work),
                        ..Default::default()
                    }),
                }),
                store: Arc::new(FakeObjectStore::default()),
            }
        }

        fn handler(&self) -> AttachmentFinalizationHandler<FakeRepository, FakeObjectStore> {
            AttachmentFinalizationHandler::new(self.repository.clone(), self.store.clone())
        }

        fn message(&self) -> WorkerMessage {
            WorkerMessage {
                message_id: "message-1".to_owned(),
                event_type: "attachment.finalization_requested".to_owned(),
                aggregate_type: "evidence_attachment".to_owned(),
                aggregate_id: self.attachment_id.to_string(),
                request_id: Some(Uuid::new_v4()),
                payload: serde_json::json!({
                    "evidence_submission_id": self.submission_id.to_string(),
                    "object_key": self.object_key,
                }),
                delivery_attempt: Some(1),
            }
        }
    }

    #[derive(Default)]
    struct FakeRepositoryState {
        work: Option<FinalizingAttachmentUploadWork>,
        uploaded: Vec<(EvidenceAttachmentId, String, String)>,
        update_fails: bool,
    }

    struct FakeRepository {
        state: Mutex<FakeRepositoryState>,
    }

    #[async_trait]
    impl AttachmentFinalizationRepository for FakeRepository {
        async fn load_finalizing_attachment_upload_work(
            &self,
            _evidence_attachment_id: EvidenceAttachmentId,
            _evidence_submission_id: EvidenceSubmissionId,
            _quarantine_object_key: &str,
        ) -> Result<Option<FinalizingAttachmentUploadWork>, RepositoryError> {
            Ok(self.state.lock().unwrap().work.clone())
        }

        async fn mark_attachment_uploaded(
            &self,
            evidence_attachment_id: EvidenceAttachmentId,
            quarantine_object_key: &str,
            final_object_key: &str,
        ) -> Result<bool, RepositoryError> {
            let mut state = self.state.lock().unwrap();
            if state.update_fails {
                return Err(RepositoryError::InvariantViolation("injected failure"));
            }
            state.uploaded.push((
                evidence_attachment_id,
                quarantine_object_key.to_owned(),
                final_object_key.to_owned(),
            ));
            Ok(true)
        }
    }

    #[derive(Default)]
    struct FakeObjectStore {
        state: Mutex<FakeObjectStoreState>,
    }

    #[derive(Default)]
    struct FakeObjectStoreState {
        copied: Vec<(String, String)>,
        deleted: Vec<String>,
        copy_fails: bool,
        delete_fails: bool,
    }

    #[async_trait]
    impl ObjectStore for FakeObjectStore {
        async fn put_object<S>(
            &self,
            _request: PutObjectRequest<S>,
        ) -> Result<ObjectMetadata, StorageError>
        where
            S: Stream<Item = Result<Bytes, StorageError>> + Send,
        {
            unreachable!()
        }

        async fn get_object(&self, _key: ObjectKey) -> Result<ObjectStream, StorageError> {
            unreachable!()
        }

        async fn head_object(&self, _key: ObjectKey) -> Result<ObjectMetadata, StorageError> {
            unreachable!()
        }

        async fn copy_object(
            &self,
            source: ObjectKey,
            destination: ObjectKey,
        ) -> Result<ObjectMetadata, StorageError> {
            let mut state = self.state.lock().unwrap();
            state
                .copied
                .push((source.to_string(), destination.to_string()));
            if state.copy_fails {
                return Err(StorageError::UnsupportedBackend);
            }
            Ok(ObjectMetadata {
                key: destination,
                content_type: "text/plain".to_owned(),
                content_length: 5,
                sha256: "checksum".to_owned(),
            })
        }

        async fn delete_object(&self, key: ObjectKey) -> Result<(), StorageError> {
            let mut state = self.state.lock().unwrap();
            state.deleted.push(key.to_string());
            if state.delete_fails {
                return Err(StorageError::UnsupportedBackend);
            }
            Ok(())
        }
    }
}
