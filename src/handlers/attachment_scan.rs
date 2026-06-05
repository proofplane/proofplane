use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use serde::Deserialize;
use thiserror::Error;
use uuid::Uuid;

use crate::{
    domain::{EvidenceAttachmentId, EvidenceSubmissionId},
    object_storage::{FilesystemObjectStore, ObjectKey, ObjectStore, StorageError},
    repository::{
        AttachmentScanCompletion, AttachmentScanFailure, PendingAttachmentScanWork, Postgres,
    },
    scanner::{
        MalwareScanError, MalwareScanOutcome, MalwareScanResult, MalwareScanner, ScanObjectRequest,
    },
    worker::{RetryableWorkerError, WorkerMessage},
};

const MISSING_OBJECT_FAILURE_REASON: &str = "quarantined object was not found";
const SAFE_REASON_MAX_CHARS: usize = 512;

#[async_trait]
pub trait AttachmentScanRepository: Send + Sync {
    async fn load_pending_attachment_scan_work(
        &self,
        evidence_attachment_id: EvidenceAttachmentId,
        quarantine_object_key: &str,
    ) -> Result<Option<PendingAttachmentScanWork>, AttachmentScanRepositoryError>;

    async fn mark_attachment_scan_clean(
        &self,
        evidence_attachment_id: EvidenceAttachmentId,
        quarantine_object_key: &str,
        final_object_key: &str,
        completion: AttachmentScanCompletion,
    ) -> Result<bool, AttachmentScanRepositoryError>;

    async fn mark_attachment_scan_malicious(
        &self,
        evidence_attachment_id: EvidenceAttachmentId,
        quarantine_object_key: &str,
        failure: AttachmentScanFailure,
    ) -> Result<bool, AttachmentScanRepositoryError>;

    async fn mark_attachment_scan_failed(
        &self,
        evidence_attachment_id: EvidenceAttachmentId,
        quarantine_object_key: &str,
        failure: AttachmentScanFailure,
    ) -> Result<bool, AttachmentScanRepositoryError>;
}

#[derive(Debug, Error)]
#[error("attachment scan repository error: {0}")]
pub struct AttachmentScanRepositoryError(pub String);

#[async_trait]
impl AttachmentScanRepository for Postgres {
    async fn load_pending_attachment_scan_work(
        &self,
        evidence_attachment_id: EvidenceAttachmentId,
        quarantine_object_key: &str,
    ) -> Result<Option<PendingAttachmentScanWork>, AttachmentScanRepositoryError> {
        Postgres::load_pending_attachment_scan_work(
            self,
            evidence_attachment_id,
            quarantine_object_key,
        )
        .await
        .map_err(|error| AttachmentScanRepositoryError(error.to_string()))
    }

    async fn mark_attachment_scan_clean(
        &self,
        evidence_attachment_id: EvidenceAttachmentId,
        quarantine_object_key: &str,
        final_object_key: &str,
        completion: AttachmentScanCompletion,
    ) -> Result<bool, AttachmentScanRepositoryError> {
        Postgres::mark_attachment_scan_clean(
            self,
            evidence_attachment_id,
            quarantine_object_key,
            final_object_key,
            &completion,
        )
        .await
        .map_err(|error| AttachmentScanRepositoryError(error.to_string()))
    }

    async fn mark_attachment_scan_malicious(
        &self,
        evidence_attachment_id: EvidenceAttachmentId,
        quarantine_object_key: &str,
        failure: AttachmentScanFailure,
    ) -> Result<bool, AttachmentScanRepositoryError> {
        Postgres::mark_attachment_scan_malicious(
            self,
            evidence_attachment_id,
            quarantine_object_key,
            &failure,
        )
        .await
        .map_err(|error| AttachmentScanRepositoryError(error.to_string()))
    }

    async fn mark_attachment_scan_failed(
        &self,
        evidence_attachment_id: EvidenceAttachmentId,
        quarantine_object_key: &str,
        failure: AttachmentScanFailure,
    ) -> Result<bool, AttachmentScanRepositoryError> {
        Postgres::mark_attachment_scan_failed(
            self,
            evidence_attachment_id,
            quarantine_object_key,
            &failure,
        )
        .await
        .map_err(|error| AttachmentScanRepositoryError(error.to_string()))
    }
}

#[async_trait]
pub trait AttachmentScanObjectStore: Send + Sync {
    async fn head_object(&self, key: ObjectKey) -> Result<(), StorageError>;

    async fn copy_object(
        &self,
        source: ObjectKey,
        destination: ObjectKey,
    ) -> Result<(), StorageError>;

    async fn delete_object(&self, key: ObjectKey) -> Result<(), StorageError>;
}

#[async_trait]
impl AttachmentScanObjectStore for FilesystemObjectStore {
    async fn head_object(&self, key: ObjectKey) -> Result<(), StorageError> {
        ObjectStore::head_object(self, key).await.map(|_| ())
    }

    async fn copy_object(
        &self,
        source: ObjectKey,
        destination: ObjectKey,
    ) -> Result<(), StorageError> {
        ObjectStore::copy_object(self, source, destination)
            .await
            .map(|_| ())
    }

    async fn delete_object(&self, key: ObjectKey) -> Result<(), StorageError> {
        ObjectStore::delete_object(self, key).await
    }
}

pub struct AttachmentScanHandler<R, S, C> {
    repository: Arc<R>,
    object_store: Arc<S>,
    scanner: Arc<C>,
}

impl<R, S, C> Clone for AttachmentScanHandler<R, S, C> {
    fn clone(&self) -> Self {
        Self {
            repository: self.repository.clone(),
            object_store: self.object_store.clone(),
            scanner: self.scanner.clone(),
        }
    }
}

impl<R, S, C> AttachmentScanHandler<R, S, C> {
    pub fn new(repository: Arc<R>, object_store: Arc<S>, scanner: Arc<C>) -> Self {
        Self {
            repository,
            object_store,
            scanner,
        }
    }
}

impl<R, S, C> AttachmentScanHandler<R, S, C>
where
    R: AttachmentScanRepository,
    S: AttachmentScanObjectStore,
    C: MalwareScanner + Send + Sync,
{
    pub async fn handle_scan_requested(
        &self,
        message: WorkerMessage,
    ) -> Result<(), RetryableWorkerError> {
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
            .load_pending_attachment_scan_work(
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
        if let Err(error) = self.object_store.head_object(quarantine_key.clone()).await {
            return match error {
                StorageError::NotFound => {
                    self.mark_failed(&work, "object-store", None, MISSING_OBJECT_FAILURE_REASON)
                        .await?;
                    Ok(())
                }
                other => Err(retryable(other)),
            };
        }

        let scan_result = self
            .scanner
            .scan_object(ScanObjectRequest {
                object_key: quarantine_key.clone(),
                content_type: work.content_type.clone(),
                content_length: scan_content_length(&work)?,
                sha256: work.checksum_sha256.clone(),
            })
            .await
            .map_err(scan_error)?;

        self.apply_scan_result(work, quarantine_key, scan_result)
            .await
    }

    async fn apply_scan_result(
        &self,
        work: PendingAttachmentScanWork,
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
                    .mark_attachment_scan_clean(
                        work.evidence_attachment_id,
                        quarantine_key.as_str(),
                        final_key.as_str(),
                        AttachmentScanCompletion {
                            scanner_name: scan_result.scanner_name,
                            scanner_version: scan_result.scanner_version,
                            scanned_at: Utc::now(),
                        },
                    )
                    .await
                    .map_err(retryable)?;

                if updated {
                    if let Err(error) = self.object_store.delete_object(quarantine_key).await {
                        tracing::warn!(
                            error = %error,
                            "failed to delete quarantined attachment object after finalization"
                        );
                    }
                }

                Ok(())
            }
            MalwareScanOutcome::Malicious { reason } => {
                self.mark_malicious(
                    &work,
                    scan_result.scanner_name,
                    scan_result.scanner_version,
                    reason,
                )
                .await
            }
            MalwareScanOutcome::Failed { reason } => {
                self.mark_failed(
                    &work,
                    scan_result.scanner_name,
                    scan_result.scanner_version,
                    reason,
                )
                .await
            }
        }
    }

    async fn mark_malicious(
        &self,
        work: &PendingAttachmentScanWork,
        scanner_name: impl Into<String>,
        scanner_version: Option<String>,
        reason: impl AsRef<str>,
    ) -> Result<(), RetryableWorkerError> {
        self.repository
            .mark_attachment_scan_malicious(
                work.evidence_attachment_id,
                &work.object_key,
                AttachmentScanFailure {
                    scanner_name: scanner_name.into(),
                    scanner_version,
                    scanned_at: Utc::now(),
                    reason: safe_failure_reason(reason.as_ref()),
                },
            )
            .await
            .map_err(retryable)?;

        Ok(())
    }

    async fn mark_failed(
        &self,
        work: &PendingAttachmentScanWork,
        scanner_name: impl Into<String>,
        scanner_version: Option<String>,
        reason: impl AsRef<str>,
    ) -> Result<(), RetryableWorkerError> {
        self.repository
            .mark_attachment_scan_failed(
                work.evidence_attachment_id,
                &work.object_key,
                AttachmentScanFailure {
                    scanner_name: scanner_name.into(),
                    scanner_version,
                    scanned_at: Utc::now(),
                    reason: safe_failure_reason(reason.as_ref()),
                },
            )
            .await
            .map_err(retryable)?;

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ScanRequestedPayload {
    evidence_attachment_id: EvidenceAttachmentId,
    evidence_submission_id: EvidenceSubmissionId,
    object_key: ObjectKey,
}

impl ScanRequestedPayload {
    fn try_from_message(message: &WorkerMessage) -> Result<Self, PermanentScanMessageError> {
        if message.aggregate_type != "evidence_attachment" {
            return Err(PermanentScanMessageError::InvalidAggregateType);
        }

        let dto = serde_json::from_value::<ScanRequestedPayloadDTO>(message.payload.clone())
            .map_err(|_| PermanentScanMessageError::InvalidPayload)?;
        let evidence_attachment_id = EvidenceAttachmentId::from(
            Uuid::parse_str(&dto.evidence_attachment_id)
                .map_err(|_| PermanentScanMessageError::InvalidAttachmentId)?,
        );
        let evidence_submission_id = EvidenceSubmissionId::from(
            Uuid::parse_str(&dto.evidence_submission_id)
                .map_err(|_| PermanentScanMessageError::InvalidSubmissionId)?,
        );
        let aggregate_id = Uuid::parse_str(&message.aggregate_id)
            .map_err(|_| PermanentScanMessageError::InvalidAggregateId)?;
        if aggregate_id != Uuid::from(evidence_attachment_id) {
            return Err(PermanentScanMessageError::MismatchedAggregateId);
        }

        let object_key =
            ObjectKey::parse(dto.object_key).map_err(|_| PermanentScanMessageError::InvalidKey)?;

        Ok(Self {
            evidence_attachment_id,
            evidence_submission_id,
            object_key,
        })
    }
}

#[derive(Debug, Deserialize)]
struct ScanRequestedPayloadDTO {
    evidence_attachment_id: String,
    evidence_submission_id: String,
    object_key: String,
}

#[derive(Debug, Error, PartialEq, Eq)]
enum PermanentScanMessageError {
    #[error("invalid aggregate type")]
    InvalidAggregateType,
    #[error("invalid scan-request payload")]
    InvalidPayload,
    #[error("invalid evidence attachment id")]
    InvalidAttachmentId,
    #[error("invalid evidence submission id")]
    InvalidSubmissionId,
    #[error("invalid aggregate id")]
    InvalidAggregateId,
    #[error("aggregate id does not match evidence attachment id")]
    MismatchedAggregateId,
    #[error("invalid quarantine object key")]
    InvalidKey,
}

fn final_attachment_object_key(
    work: &PendingAttachmentScanWork,
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

fn scan_content_length(work: &PendingAttachmentScanWork) -> Result<u64, RetryableWorkerError> {
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

    use crate::{
        domain::{AttachmentScanStatus, WorkspaceId},
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
            PermanentScanMessageError::InvalidPayload
        );

        let mut mismatched = message(attachment_id, submission_id, &key);
        mismatched.aggregate_id = Uuid::new_v4().to_string();
        assert_eq!(
            ScanRequestedPayload::try_from_message(&mismatched).unwrap_err(),
            PermanentScanMessageError::MismatchedAggregateId
        );

        let mut bad_key = message(attachment_id, submission_id, "not/workspace/key");
        assert_eq!(
            ScanRequestedPayload::try_from_message(&bad_key).unwrap_err(),
            PermanentScanMessageError::InvalidKey
        );

        bad_key.aggregate_id = "not-a-uuid".to_owned();
        assert_eq!(
            ScanRequestedPayload::try_from_message(&bad_key).unwrap_err(),
            PermanentScanMessageError::InvalidAggregateId
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
            assert!(store.deleted.is_empty());
        }
    }

    #[tokio::test]
    async fn scanner_adapter_errors_are_retryable() {
        let fixture = Fixture::scanner_error();

        let result = fixture
            .handler()
            .handle_scan_requested(fixture.message())
            .await;

        assert!(result.is_err());
        assert!(fixture.repository.state.lock().unwrap().failed.is_empty());
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
        assert_eq!(repo.failed[0].2.reason, MISSING_OBJECT_FAILURE_REASON);
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
            fixture.object_store = Arc::new(FakeObjectStore::missing());
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
        work: Option<PendingAttachmentScanWork>,
        cleaned: Vec<(
            EvidenceAttachmentId,
            String,
            String,
            AttachmentScanCompletion,
        )>,
        malicious: Vec<(EvidenceAttachmentId, String, AttachmentScanFailure)>,
        failed: Vec<(EvidenceAttachmentId, String, AttachmentScanFailure)>,
    }

    impl FakeRepository {
        fn with_work(work: PendingAttachmentScanWork) -> Self {
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
        async fn load_pending_attachment_scan_work(
            &self,
            _evidence_attachment_id: EvidenceAttachmentId,
            _quarantine_object_key: &str,
        ) -> Result<Option<PendingAttachmentScanWork>, AttachmentScanRepositoryError> {
            Ok(self.state.lock().unwrap().work.clone())
        }

        async fn mark_attachment_scan_clean(
            &self,
            evidence_attachment_id: EvidenceAttachmentId,
            quarantine_object_key: &str,
            final_object_key: &str,
            completion: AttachmentScanCompletion,
        ) -> Result<bool, AttachmentScanRepositoryError> {
            self.state.lock().unwrap().cleaned.push((
                evidence_attachment_id,
                quarantine_object_key.to_owned(),
                final_object_key.to_owned(),
                completion,
            ));
            Ok(true)
        }

        async fn mark_attachment_scan_malicious(
            &self,
            evidence_attachment_id: EvidenceAttachmentId,
            quarantine_object_key: &str,
            failure: AttachmentScanFailure,
        ) -> Result<bool, AttachmentScanRepositoryError> {
            self.state.lock().unwrap().malicious.push((
                evidence_attachment_id,
                quarantine_object_key.to_owned(),
                failure,
            ));
            Ok(true)
        }

        async fn mark_attachment_scan_failed(
            &self,
            evidence_attachment_id: EvidenceAttachmentId,
            quarantine_object_key: &str,
            failure: AttachmentScanFailure,
        ) -> Result<bool, AttachmentScanRepositoryError> {
            self.state.lock().unwrap().failed.push((
                evidence_attachment_id,
                quarantine_object_key.to_owned(),
                failure,
            ));
            Ok(true)
        }
    }

    struct FakeObjectStore {
        state: Mutex<FakeObjectStoreState>,
    }

    #[derive(Default)]
    struct FakeObjectStoreState {
        found: bool,
        copied: Vec<(String, String)>,
        deleted: Vec<String>,
    }

    impl FakeObjectStore {
        fn found() -> Self {
            Self {
                state: Mutex::new(FakeObjectStoreState {
                    found: true,
                    ..FakeObjectStoreState::default()
                }),
            }
        }

        fn missing() -> Self {
            Self {
                state: Mutex::new(FakeObjectStoreState::default()),
            }
        }
    }

    #[async_trait]
    impl AttachmentScanObjectStore for FakeObjectStore {
        async fn head_object(&self, _key: ObjectKey) -> Result<(), StorageError> {
            if self.state.lock().unwrap().found {
                Ok(())
            } else {
                Err(StorageError::NotFound)
            }
        }

        async fn copy_object(
            &self,
            source: ObjectKey,
            destination: ObjectKey,
        ) -> Result<(), StorageError> {
            self.state
                .lock()
                .unwrap()
                .copied
                .push((source.to_string(), destination.to_string()));
            Ok(())
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
    ) -> PendingAttachmentScanWork {
        PendingAttachmentScanWork {
            workspace_id: WorkspaceId::from(workspace_id),
            evidence_submission_id: EvidenceSubmissionId::from(submission_id),
            evidence_attachment_id: EvidenceAttachmentId::from(attachment_id),
            filename: "manual.txt".to_owned(),
            content_type: "text/plain".to_owned(),
            content_length: 5,
            object_key: object_key.to_owned(),
            checksum_sha256: "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
                .to_owned(),
            scan_status: AttachmentScanStatus::Pending,
        }
    }

    fn message(attachment_id: Uuid, submission_id: Uuid, object_key: &str) -> WorkerMessage {
        WorkerMessage {
            message_id: "message-1".to_owned(),
            event_type: "attachment.scan_requested".to_owned(),
            aggregate_type: "evidence_attachment".to_owned(),
            aggregate_id: attachment_id.to_string(),
            request_id: Some("request-1".to_owned()),
            payload: serde_json::json!({
                "evidence_attachment_id": attachment_id.to_string(),
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
