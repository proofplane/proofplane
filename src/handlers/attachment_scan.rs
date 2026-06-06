use std::sync::Arc;

use serde::Deserialize;
use thiserror::Error;
use uuid::Uuid;

use crate::{
    domain::{EvidenceAttachmentId, EvidenceSubmissionId},
    object_storage::{ObjectKey, ObjectStore, StorageError},
    repository::{AttachmentScanRepository, PendingAttachmentUploadWork},
    scanner::{
        MalwareScanError, MalwareScanOutcome, MalwareScanResult, MalwareScanner, ScanObjectRequest,
    },
    validate,
    validation::Validation,
    worker::{RetryableWorkerError, WorkerMessage},
};

const MISSING_OBJECT_FAILURE_REASON: &str = "quarantined object was not found";
const SAFE_REASON_MAX_CHARS: usize = 512;

pub struct AttachmentScanHandler<R, S, C> {
    repository: Arc<R>,
    object_store: Arc<S>,
    scanner: Arc<C>,
    max_delivery_attempts: u16,
}

impl<R, S, C> Clone for AttachmentScanHandler<R, S, C> {
    fn clone(&self) -> Self {
        Self {
            repository: self.repository.clone(),
            object_store: self.object_store.clone(),
            scanner: self.scanner.clone(),
            max_delivery_attempts: self.max_delivery_attempts,
        }
    }
}

impl<R, S, C> AttachmentScanHandler<R, S, C> {
    pub fn new(
        repository: Arc<R>,
        object_store: Arc<S>,
        scanner: Arc<C>,
        max_delivery_attempts: u16,
    ) -> Self {
        Self {
            repository,
            object_store,
            scanner,
            max_delivery_attempts,
        }
    }
}

impl<R, S, C> AttachmentScanHandler<R, S, C>
where
    R: AttachmentScanRepository,
    S: ObjectStore + Send + Sync,
    C: MalwareScanner + Send + Sync,
{
    pub async fn handle_scan_requested(
        &self,
        message: WorkerMessage,
    ) -> Result<(), RetryableWorkerError> {
        let final_delivery = message
            .delivery_attempt
            .is_some_and(|attempt| attempt >= u32::from(self.max_delivery_attempts));
        let payload = match ScanRequestedPayload::try_from_message(&message) {
            Ok(payload) => payload,
            Err(error) => {
                tracing::warn!(
                    message_id = %message.message_id,
                    error = %error,
                    "acknowledging invalid attachment scan message"
                );
                return Ok(());
            }
        };

        let Some(work) = self
            .repository
            .load_pending_attachment_upload_work(
                payload.evidence_attachment_id,
                payload.object_key.as_str(),
            )
            .await
            .map_err(retryable)?
        else {
            tracing::info!(
                evidence_attachment_id = %payload.evidence_attachment_id,
                object_key = %payload.object_key,
                "acknowledging duplicate or stale attachment scan message"
            );
            return Ok(());
        };

        if payload.evidence_submission_id != work.evidence_submission_id {
            tracing::warn!(
                evidence_attachment_id = %payload.evidence_attachment_id,
                payload_submission_id = %payload.evidence_submission_id,
                work_submission_id = %work.evidence_submission_id,
                "acknowledging attachment scan message with mismatched submission id"
            );
            return Ok(());
        }

        let quarantine_key = ObjectKey::parse(work.object_key.clone()).map_err(retryable)?;
        let scan_result = self
            .scanner
            .scan_object(ScanObjectRequest {
                object_key: quarantine_key.clone(),
                content_type: work.content_type.clone(),
                content_length: scan_content_length(&work)?,
                sha256: work.checksum_sha256.clone(),
            })
            .await;

        let scan_result = match scan_result {
            Ok(scan_result) => scan_result,
            Err(MalwareScanError::ObjectNotFound) => {
                self.mark_failed(&work, MISSING_OBJECT_FAILURE_REASON)
                    .await?;
                return Ok(());
            }
            Err(error) if final_delivery => {
                self.mark_failed(&work, error.to_string()).await?;
                return Ok(());
            }
            Err(error) => return Err(scan_error(error)),
        };

        self.apply_scan_result(work, quarantine_key, scan_result)
            .await
    }

    async fn apply_scan_result(
        &self,
        work: PendingAttachmentUploadWork,
        quarantine_key: ObjectKey,
        scan_result: MalwareScanResult,
    ) -> Result<(), RetryableWorkerError> {
        match scan_result.outcome {
            MalwareScanOutcome::Clean => {
                let final_key = final_attachment_object_key(&work).map_err(retryable)?;
                self.object_store
                    .copy_object(quarantine_key.clone(), final_key.clone())
                    .await
                    .map_err(retryable)?;

                let updated = self
                    .repository
                    .mark_attachment_uploaded(
                        work.evidence_attachment_id,
                        quarantine_key.as_str(),
                        final_key.as_str(),
                    )
                    .await
                    .map_err(retryable)?;

                if updated {
                    self.object_store
                        .delete_object(quarantine_key)
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
            MalwareScanOutcome::Malicious { reason } => self.mark_malicious(&work, reason).await,
            MalwareScanOutcome::Failed { reason } => self.mark_failed(&work, reason).await,
        }
    }

    async fn mark_malicious(
        &self,
        work: &PendingAttachmentUploadWork,
        reason: impl AsRef<str>,
    ) -> Result<(), RetryableWorkerError> {
        let updated = self
            .repository
            .mark_attachment_contains_virus(
                work.evidence_attachment_id,
                &work.object_key,
                safe_failure_reason(reason.as_ref()),
            )
            .await
            .map_err(retryable)?;

        if updated {
            self.delete_quarantine_object(&work.object_key).await;
        }

        Ok(())
    }

    async fn mark_failed(
        &self,
        work: &PendingAttachmentUploadWork,
        reason: impl AsRef<str>,
    ) -> Result<(), RetryableWorkerError> {
        let updated = self
            .repository
            .mark_attachment_upload_failed(
                work.evidence_attachment_id,
                &work.object_key,
                safe_failure_reason(reason.as_ref()),
            )
            .await
            .map_err(retryable)?;

        if updated {
            self.delete_quarantine_object(&work.object_key).await;
        }

        Ok(())
    }

    async fn delete_quarantine_object(&self, object_key: &str) {
        let Ok(key) = ObjectKey::parse(object_key.to_owned()) else {
            tracing::warn!(
                object_key = %object_key,
                "failed to parse object_key when attempting to delete quarantined object"
            );
            return;
        };

        self.object_store
            .delete_object(key)
            .await
            .inspect_err(|error| {
                tracing::warn!(
                    error = %error,
                    "failed to delete quarantined attachment object after terminal scan result"
                );
            })
            .ok();
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ScanRequestedPayload {
    evidence_attachment_id: EvidenceAttachmentId,
    evidence_submission_id: EvidenceSubmissionId,
    object_key: ObjectKey,
}

impl ScanRequestedPayload {
    fn try_from_message(message: &WorkerMessage) -> Result<Self, PermanentScanMessageErrors> {
        let dto = serde_json::from_value::<ScanRequestedPayloadDTO>(message.payload.clone())
            .map_err(|_| {
                PermanentScanMessageErrors(vec![PermanentScanMessageError::InvalidPayload])
            })?;

        if message.aggregate_type != "evidence_attachment" {
            return Err(PermanentScanMessageErrors(vec![
                PermanentScanMessageError::InvalidAggregateType,
            ]));
        }

        let payload = validate! {
            evidence_attachment_id <- validate_aggregate_id(&message.aggregate_id),
            evidence_submission_id <- validate_submission_id(&dto.evidence_submission_id),
            object_key <- validate_object_key(dto.object_key),
            => (evidence_attachment_id, evidence_submission_id, object_key),
        }
        .into_result()
        .map_err(PermanentScanMessageErrors)?;

        let (evidence_attachment_id, evidence_submission_id, object_key) = payload;

        Ok(Self {
            evidence_attachment_id,
            evidence_submission_id,
            object_key,
        })
    }
}

fn validate_aggregate_id(
    value: &str,
) -> Validation<EvidenceAttachmentId, PermanentScanMessageError> {
    Uuid::parse_str(value)
        .map(EvidenceAttachmentId::from)
        .map(Validation::valid)
        .unwrap_or_else(|_| Validation::invalid(PermanentScanMessageError::InvalidAggregateId))
}

fn validate_submission_id(
    value: &str,
) -> Validation<EvidenceSubmissionId, PermanentScanMessageError> {
    Uuid::parse_str(value)
        .map(EvidenceSubmissionId::from)
        .map(Validation::valid)
        .unwrap_or_else(|_| Validation::invalid(PermanentScanMessageError::InvalidSubmissionId))
}

fn validate_object_key(value: String) -> Validation<ObjectKey, PermanentScanMessageError> {
    ObjectKey::parse(value)
        .map(Validation::valid)
        .unwrap_or_else(|_| Validation::invalid(PermanentScanMessageError::InvalidKey))
}

#[derive(Debug, Deserialize)]
struct ScanRequestedPayloadDTO {
    evidence_submission_id: String,
    object_key: String,
}

#[derive(Debug, Error, PartialEq, Eq)]
enum PermanentScanMessageError {
    #[error("invalid aggregate type")]
    InvalidAggregateType,
    #[error("invalid scan-request payload")]
    InvalidPayload,
    #[error("invalid evidence submission id")]
    InvalidSubmissionId,
    #[error("invalid aggregate id")]
    InvalidAggregateId,
    #[error("invalid quarantine object key")]
    InvalidKey,
}

#[derive(Debug, Error, PartialEq, Eq)]
#[error("invalid attachment scan message: {0:?}")]
struct PermanentScanMessageErrors(Vec<PermanentScanMessageError>);

fn final_attachment_object_key(
    work: &PendingAttachmentUploadWork,
) -> Result<ObjectKey, StorageError> {
    ObjectKey::new(
        work.workspace_id,
        format!(
            "evidence-submissions/{}/attachments/{}",
            work.evidence_submission_id, work.evidence_attachment_id
        ),
        &work.filename,
    )
}

fn scan_content_length(work: &PendingAttachmentUploadWork) -> Result<u64, RetryableWorkerError> {
    u64::try_from(work.content_length)
        .map_err(|_| RetryableWorkerError("pending attachment has negative length".to_owned()))
}

fn safe_failure_reason(reason: &str) -> String {
    reason.trim().chars().take(SAFE_REASON_MAX_CHARS).collect()
}

fn scan_error(error: MalwareScanError) -> RetryableWorkerError {
    RetryableWorkerError(format!("malware scanner adapter failed: {error}"))
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
        object_storage::{ObjectMetadata, ObjectStream, PutObjectRequest},
        repository::Error as RepositoryError,
        scanner::{MalwareScanOutcome, MalwareScanResult},
    };

    use super::*;

    #[test]
    fn scan_payload_parsing_accepts_valid_message() {
        let attachment_id = Uuid::new_v4();
        let submission_id = Uuid::new_v4();
        let workspace_id = Uuid::new_v4();
        let key = format!(
            "workspaces/{workspace_id}/quarantine/evidence-submissions/{submission_id}/attachments/upload/manual.txt"
        );

        let payload =
            ScanRequestedPayload::try_from_message(&message(attachment_id, submission_id, &key))
                .expect("payload parses");

        assert_eq!(Uuid::from(payload.evidence_attachment_id), attachment_id);
        assert_eq!(Uuid::from(payload.evidence_submission_id), submission_id);
        assert_eq!(payload.object_key.as_str(), key);
    }

    #[test]
    fn scan_payload_parsing_rejects_permanent_payload_errors() {
        let attachment_id = Uuid::new_v4();
        let submission_id = Uuid::new_v4();
        let key = format!(
            "workspaces/{}/quarantine/evidence-submissions/{submission_id}/attachments/upload/manual.txt",
            Uuid::new_v4()
        );

        let mut invalid = message(attachment_id, submission_id, &key);
        invalid.payload = serde_json::json!({});
        assert_eq!(
            ScanRequestedPayload::try_from_message(&invalid).unwrap_err(),
            PermanentScanMessageErrors(vec![PermanentScanMessageError::InvalidPayload])
        );

        let mut invalid_aggregate_type = message(attachment_id, submission_id, &key);
        invalid_aggregate_type.aggregate_type = "evidence_submission".to_owned();
        assert_eq!(
            ScanRequestedPayload::try_from_message(&invalid_aggregate_type).unwrap_err(),
            PermanentScanMessageErrors(vec![PermanentScanMessageError::InvalidAggregateType])
        );

        let mut invalid_submission_id = message(attachment_id, submission_id, &key);
        invalid_submission_id.payload["evidence_submission_id"] =
            serde_json::Value::String("not-a-uuid".to_owned());
        assert_eq!(
            ScanRequestedPayload::try_from_message(&invalid_submission_id).unwrap_err(),
            PermanentScanMessageErrors(vec![PermanentScanMessageError::InvalidSubmissionId])
        );

        let mut invalid_fields = message(attachment_id, submission_id, "not/workspace/key");
        invalid_fields.aggregate_id = "not-a-uuid".to_owned();
        assert_eq!(
            ScanRequestedPayload::try_from_message(&invalid_fields).unwrap_err(),
            PermanentScanMessageErrors(vec![
                PermanentScanMessageError::InvalidAggregateId,
                PermanentScanMessageError::InvalidKey,
            ])
        );
    }

    #[tokio::test]
    async fn clean_scan_copies_marks_clean_and_deletes_quarantine() {
        let fixture = Fixture::new(MalwareScanOutcome::Clean);

        fixture
            .handler()
            .handle_scan_requested(fixture.message())
            .await
            .expect("handler succeeds");

        let repo = fixture.repository.state.lock().unwrap();
        assert_eq!(repo.cleaned.len(), 1);
        assert_eq!(
            repo.cleaned[0].2,
            format!(
                "workspaces/{}/evidence-submissions/{}/attachments/{}/manual.txt",
                fixture.workspace_id, fixture.submission_id, fixture.attachment_id
            )
        );
        drop(repo);

        let store = fixture.object_store.state.lock().unwrap();
        assert_eq!(store.copied.len(), 1);
        assert_eq!(store.deleted, vec![fixture.object_key.clone()]);
    }

    #[tokio::test]
    async fn malicious_and_scanner_failed_scans_persist_terminal_states_without_finalizing() {
        for outcome in [
            MalwareScanOutcome::Malicious {
                reason: "EICAR".to_owned(),
            },
            MalwareScanOutcome::Failed {
                reason: "scanner refused".to_owned(),
            },
        ] {
            let fixture = Fixture::new(outcome.clone());

            fixture
                .handler()
                .handle_scan_requested(fixture.message())
                .await
                .expect("handler succeeds");

            let repo = fixture.repository.state.lock().unwrap();
            match outcome {
                MalwareScanOutcome::Malicious { .. } => assert_eq!(repo.malicious.len(), 1),
                MalwareScanOutcome::Failed { .. } => assert_eq!(repo.failed.len(), 1),
                MalwareScanOutcome::Clean => unreachable!(),
            }
            drop(repo);

            let store = fixture.object_store.state.lock().unwrap();
            assert!(store.copied.is_empty());
            assert_eq!(store.deleted, vec![fixture.object_key.clone()]);
        }
    }

    #[tokio::test]
    async fn scanner_adapter_error_without_delivery_attempt_is_retryable() {
        let fixture = Fixture::scanner_error();
        let mut message = fixture.message();
        message.delivery_attempt = None;

        let result = fixture.handler().handle_scan_requested(message).await;

        assert!(result.is_err());
        assert!(fixture.repository.state.lock().unwrap().failed.is_empty());
    }

    #[tokio::test]
    async fn scanner_adapter_error_below_maximum_is_retryable() {
        let fixture = Fixture::scanner_error();
        let mut message = fixture.message();
        message.delivery_attempt = Some(u32::from(MAX_DELIVERY_ATTEMPTS - 1));

        let result = fixture.handler().handle_scan_requested(message).await;

        assert!(result.is_err());
        assert!(fixture.repository.state.lock().unwrap().failed.is_empty());
    }

    #[tokio::test]
    async fn scanner_adapter_error_at_maximum_marks_failed() {
        let fixture = Fixture::scanner_error();
        let mut message = fixture.message();
        message.delivery_attempt = Some(u32::from(MAX_DELIVERY_ATTEMPTS));

        fixture
            .handler()
            .handle_scan_requested(message)
            .await
            .expect("handler succeeds");

        let repo = fixture.repository.state.lock().unwrap();
        assert_eq!(repo.failed.len(), 1);
        assert!(repo.failed[0].2.contains("malware scanner is unavailable"));
        drop(repo);

        let store = fixture.object_store.state.lock().unwrap();
        assert_eq!(store.deleted, vec![fixture.object_key.clone()]);
    }

    #[tokio::test]
    async fn scanner_adapter_error_above_maximum_marks_failed() {
        let fixture = Fixture::scanner_error();
        let mut message = fixture.message();
        message.delivery_attempt = Some(u32::from(MAX_DELIVERY_ATTEMPTS) + 1);

        fixture
            .handler()
            .handle_scan_requested(message)
            .await
            .expect("handler succeeds");

        let repo = fixture.repository.state.lock().unwrap();
        assert_eq!(repo.failed.len(), 1);
        assert!(repo.failed[0].2.contains("malware scanner is unavailable"));
        drop(repo);

        let store = fixture.object_store.state.lock().unwrap();
        assert_eq!(store.deleted, vec![fixture.object_key.clone()]);
    }

    #[tokio::test]
    async fn missing_quarantine_object_marks_failed_and_acknowledges() {
        let fixture = Fixture::missing_object();

        fixture
            .handler()
            .handle_scan_requested(fixture.message())
            .await
            .expect("handler succeeds");

        let repo = fixture.repository.state.lock().unwrap();
        assert_eq!(repo.failed.len(), 1);
        assert_eq!(repo.failed[0].2, MISSING_OBJECT_FAILURE_REASON);
    }

    #[tokio::test]
    async fn duplicate_or_stale_work_is_successful_noop() {
        let fixture = Fixture::no_pending_work();

        fixture
            .handler()
            .handle_scan_requested(fixture.message())
            .await
            .expect("handler succeeds");

        let repo = fixture.repository.state.lock().unwrap();
        assert!(repo.cleaned.is_empty());
        assert!(repo.failed.is_empty());
        assert!(repo.malicious.is_empty());
    }

    #[tokio::test]
    async fn mismatched_payload_submission_id_is_successful_noop() {
        let fixture = Fixture::new(MalwareScanOutcome::Clean);
        let mut message = fixture.message();
        message.payload["evidence_submission_id"] = serde_json::json!(Uuid::new_v4().to_string());

        fixture
            .handler()
            .handle_scan_requested(message)
            .await
            .expect("handler succeeds");

        let repo = fixture.repository.state.lock().unwrap();
        assert!(repo.cleaned.is_empty());
        assert!(repo.failed.is_empty());
        assert!(repo.malicious.is_empty());
    }

    struct Fixture {
        attachment_id: Uuid,
        submission_id: Uuid,
        workspace_id: Uuid,
        object_key: String,
        repository: Arc<FakeRepository>,
        object_store: Arc<FakeObjectStore>,
        scanner: Arc<FakeScanner>,
    }

    const MAX_DELIVERY_ATTEMPTS: u16 = 5;

    impl Fixture {
        fn new(outcome: MalwareScanOutcome) -> Self {
            let attachment_id = Uuid::new_v4();
            let submission_id = Uuid::new_v4();
            let workspace_id = Uuid::new_v4();
            let object_key = quarantine_key(workspace_id, submission_id);
            let work = pending_work(attachment_id, submission_id, workspace_id, &object_key);

            Self {
                attachment_id,
                submission_id,
                workspace_id,
                object_key: object_key.clone(),
                repository: Arc::new(FakeRepository::with_work(work)),
                object_store: Arc::new(FakeObjectStore::found()),
                scanner: Arc::new(FakeScanner::outcome(outcome)),
            }
        }

        fn scanner_error() -> Self {
            let mut fixture = Self::new(MalwareScanOutcome::Clean);
            fixture.scanner = Arc::new(FakeScanner::error());
            fixture
        }

        fn missing_object() -> Self {
            let mut fixture = Self::new(MalwareScanOutcome::Clean);
            fixture.scanner = Arc::new(FakeScanner::missing_object());
            fixture
        }

        fn no_pending_work() -> Self {
            let mut fixture = Self::new(MalwareScanOutcome::Clean);
            fixture.repository = Arc::new(FakeRepository::empty());
            fixture
        }

        fn handler(&self) -> AttachmentScanHandler<FakeRepository, FakeObjectStore, FakeScanner> {
            AttachmentScanHandler::new(
                self.repository.clone(),
                self.object_store.clone(),
                self.scanner.clone(),
                MAX_DELIVERY_ATTEMPTS,
            )
        }

        fn message(&self) -> WorkerMessage {
            message(self.attachment_id, self.submission_id, &self.object_key)
        }
    }

    #[derive(Default)]
    struct FakeRepository {
        state: Mutex<FakeRepositoryState>,
    }

    #[derive(Default)]
    struct FakeRepositoryState {
        work: Option<PendingAttachmentUploadWork>,
        cleaned: Vec<(EvidenceAttachmentId, String, String)>,
        malicious: Vec<(EvidenceAttachmentId, String, String)>,
        failed: Vec<(EvidenceAttachmentId, String, String)>,
    }

    impl FakeRepository {
        fn with_work(work: PendingAttachmentUploadWork) -> Self {
            Self {
                state: Mutex::new(FakeRepositoryState {
                    work: Some(work),
                    ..FakeRepositoryState::default()
                }),
            }
        }

        fn empty() -> Self {
            Self::default()
        }
    }

    #[async_trait]
    impl AttachmentScanRepository for FakeRepository {
        async fn load_pending_attachment_upload_work(
            &self,
            _evidence_attachment_id: EvidenceAttachmentId,
            _quarantine_object_key: &str,
        ) -> Result<Option<PendingAttachmentUploadWork>, RepositoryError> {
            Ok(self.state.lock().unwrap().work.clone())
        }

        async fn mark_attachment_uploaded(
            &self,
            evidence_attachment_id: EvidenceAttachmentId,
            quarantine_object_key: &str,
            final_object_key: &str,
        ) -> Result<bool, RepositoryError> {
            self.state.lock().unwrap().cleaned.push((
                evidence_attachment_id,
                quarantine_object_key.to_owned(),
                final_object_key.to_owned(),
            ));
            Ok(true)
        }

        async fn mark_attachment_contains_virus(
            &self,
            evidence_attachment_id: EvidenceAttachmentId,
            quarantine_object_key: &str,
            reason: String,
        ) -> Result<bool, RepositoryError> {
            self.state.lock().unwrap().malicious.push((
                evidence_attachment_id,
                quarantine_object_key.to_owned(),
                reason,
            ));
            Ok(true)
        }

        async fn mark_attachment_upload_failed(
            &self,
            evidence_attachment_id: EvidenceAttachmentId,
            quarantine_object_key: &str,
            reason: String,
        ) -> Result<bool, RepositoryError> {
            self.state.lock().unwrap().failed.push((
                evidence_attachment_id,
                quarantine_object_key.to_owned(),
                reason,
            ));
            Ok(true)
        }
    }

    struct FakeObjectStore {
        state: Mutex<FakeObjectStoreState>,
    }

    #[derive(Default)]
    struct FakeObjectStoreState {
        copied: Vec<(String, String)>,
        deleted: Vec<String>,
    }

    impl FakeObjectStore {
        fn found() -> Self {
            Self {
                state: Mutex::new(FakeObjectStoreState::default()),
            }
        }
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
            unreachable!("attachment scan handler does not upload objects")
        }

        async fn get_object(&self, _key: ObjectKey) -> Result<ObjectStream, StorageError> {
            unreachable!("attachment scan handler does not read objects directly")
        }

        async fn head_object(&self, _key: ObjectKey) -> Result<ObjectMetadata, StorageError> {
            unreachable!("attachment scan handler does not inspect objects directly")
        }

        async fn copy_object(
            &self,
            source: ObjectKey,
            destination: ObjectKey,
        ) -> Result<ObjectMetadata, StorageError> {
            self.state
                .lock()
                .unwrap()
                .copied
                .push((source.to_string(), destination.to_string()));
            Ok(ObjectMetadata {
                key: destination,
                content_type: "text/plain".to_owned(),
                content_length: 5,
                sha256: "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
                    .to_owned(),
            })
        }

        async fn delete_object(&self, key: ObjectKey) -> Result<(), StorageError> {
            self.state.lock().unwrap().deleted.push(key.to_string());
            Ok(())
        }
    }

    struct FakeScanner {
        result: Result<MalwareScanResult, MalwareScanError>,
    }

    impl FakeScanner {
        fn outcome(outcome: MalwareScanOutcome) -> Self {
            Self {
                result: Ok(MalwareScanResult {
                    scanner_name: "fake".to_owned(),
                    scanner_version: Some("1".to_owned()),
                    outcome,
                }),
            }
        }

        fn error() -> Self {
            Self {
                result: Err(MalwareScanError::Unavailable {
                    reason: "offline".to_owned(),
                }),
            }
        }

        fn missing_object() -> Self {
            Self {
                result: Err(MalwareScanError::ObjectNotFound),
            }
        }
    }

    #[async_trait]
    impl MalwareScanner for FakeScanner {
        async fn scan_object(
            &self,
            _request: ScanObjectRequest,
        ) -> Result<MalwareScanResult, MalwareScanError> {
            self.result.clone()
        }
    }

    fn pending_work(
        attachment_id: Uuid,
        submission_id: Uuid,
        workspace_id: Uuid,
        object_key: &str,
    ) -> PendingAttachmentUploadWork {
        PendingAttachmentUploadWork {
            workspace_id: WorkspaceId::from(workspace_id),
            evidence_submission_id: EvidenceSubmissionId::from(submission_id),
            evidence_attachment_id: EvidenceAttachmentId::from(attachment_id),
            filename: "manual.txt".to_owned(),
            content_type: "text/plain".to_owned(),
            content_length: 5,
            object_key: object_key.to_owned(),
            checksum_sha256: "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
                .to_owned(),
            upload_status: AttachmentUploadStatus::PendingUpload,
        }
    }

    fn message(attachment_id: Uuid, submission_id: Uuid, object_key: &str) -> WorkerMessage {
        WorkerMessage {
            message_id: "message-1".to_owned(),
            event_type: "attachment.scan_requested".to_owned(),
            aggregate_type: "evidence_attachment".to_owned(),
            aggregate_id: attachment_id.to_string(),
            request_id: Some(Uuid::from_u128(1)),
            payload: serde_json::json!({
                "evidence_submission_id": submission_id.to_string(),
                "object_key": object_key,
            }),
            delivery_attempt: Some(1),
        }
    }

    fn quarantine_key(workspace_id: Uuid, submission_id: Uuid) -> String {
        format!(
            "workspaces/{workspace_id}/quarantine/evidence-submissions/{submission_id}/attachments/{}/manual.txt",
            Uuid::new_v4()
        )
    }
}
