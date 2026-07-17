use std::sync::Arc;

use futures_util::StreamExt;
use serde::Deserialize;
use thiserror::Error;
use uuid::Uuid;

use crate::{
    domain::EvidenceSubmissionId,
    object_storage::{FilesystemObjectStore, ObjectKey, ObjectStore, StorageError},
    observability::audit::{AuditActor, AuditClientType, AuditEvent, AuditObject, AuditOutcome},
    pubsub::{TopicName, MESSAGE_BUS_TOPIC},
    repository::{NewOutboxMessage, PendingSubmissionUploadWork, Postgres},
    scanner::{ClamAvMalwareScanner, MalwareScanError, MalwareScanOutcome, MalwareScanResult},
    validate,
    validation::Validation,
    worker::{RetryableWorkerError, WorkerMessage, SUBMISSION_FINALIZATION_REQUESTED},
};

const MISSING_OBJECT_FAILURE_REASON: &str = "quarantined object was not found";

pub struct SubmissionScanHandler {
    repository: Arc<Postgres>,
    object_store: Arc<FilesystemObjectStore>,
    scanner: Arc<ClamAvMalwareScanner>,
    max_delivery_attempts: u16,
}

impl Clone for SubmissionScanHandler {
    fn clone(&self) -> Self {
        Self {
            repository: self.repository.clone(),
            object_store: self.object_store.clone(),
            scanner: self.scanner.clone(),
            max_delivery_attempts: self.max_delivery_attempts,
        }
    }
}

impl SubmissionScanHandler {
    pub fn new(
        repository: Arc<Postgres>,
        object_store: Arc<FilesystemObjectStore>,
        scanner: Arc<ClamAvMalwareScanner>,
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

impl SubmissionScanHandler {
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
                    "skipping invalid submission scan message"
                );
                return Ok(());
            }
        };

        let Some(work) = self
            .repository
            .load_pending_submission_upload_work(
                payload.evidence_submission_id,
                payload.object_key.as_str(),
            )
            .await
            .map_err(retryable)?
        else {
            tracing::info!(
                evidence_submission_id = %payload.evidence_submission_id,
                object_key = %payload.object_key,
                "skipping duplicate or stale submission scan message"
            );
            return Ok(());
        };

        tracing::debug!("initiating scan");

        let quarantine_key = ObjectKey::parse(work.object_key.clone()).map_err(retryable)?;
        let object = match self.object_store.get_object(&quarantine_key).await {
            Ok(object) => object,
            Err(StorageError::NotFound) => {
                let updated = self
                    .mark_failed(&work, MISSING_OBJECT_FAILURE_REASON)
                    .await?;
                if updated {
                    emit_worker_submission_audit(
                        "evidence_submission_scan.completed",
                        AuditOutcome::Failure,
                        &work,
                        message.request_id,
                        "failed_upload",
                    );
                }
                return Ok(());
            }
            Err(error) if final_delivery => {
                let updated = self.mark_failed(&work, error.to_string()).await?;
                if updated {
                    emit_worker_submission_audit(
                        "evidence_submission_scan.completed",
                        AuditOutcome::Failure,
                        &work,
                        message.request_id,
                        "failed_upload",
                    );
                }
                return Ok(());
            }
            Err(error) => return Err(retryable(error)),
        };
        let content_length = scan_content_length(&work)?;
        if object.metadata.content_type != work.content_type
            || object.metadata.content_length != content_length
            || object.metadata.sha256 != work.checksum_sha256
        {
            let error = MalwareScanError::Internal {
                reason: "stored object metadata does not match submission metadata".to_owned(),
            };
            if final_delivery {
                let updated = self.mark_failed(&work, error.to_string()).await?;
                if updated {
                    emit_worker_submission_audit(
                        "evidence_submission_scan.completed",
                        AuditOutcome::Failure,
                        &work,
                        message.request_id,
                        "failed_upload",
                    );
                }
                return Ok(());
            }
            return Err(scan_error(error));
        }
        let chunks = object.chunks.map(|chunk| {
            chunk.map_err(|error| MalwareScanError::Internal {
                reason: format!("failed to read object from storage: {error}"),
            })
        });
        let scan_result = self.scanner.scan(chunks).await;

        tracing::debug!("scan completed");

        let scan_result = match scan_result {
            Ok(scan_result) => scan_result,
            Err(error) if final_delivery => {
                let updated = self.mark_failed(&work, error.to_string()).await?;
                if updated {
                    emit_worker_submission_audit(
                        "evidence_submission_scan.completed",
                        AuditOutcome::Failure,
                        &work,
                        message.request_id,
                        "failed_upload",
                    );
                }
                // TODO: don't ack here, let the message fail so it can be dead-lettered
                return Ok(());
            }
            Err(error) => return Err(scan_error(error)),
        };

        self.apply_scan_result(work, scan_result, message.request_id)
            .await
    }

    async fn apply_scan_result(
        &self,
        work: PendingSubmissionUploadWork,
        scan_result: MalwareScanResult,
        request_id: Option<Uuid>,
    ) -> Result<(), RetryableWorkerError> {
        match scan_result.outcome {
            MalwareScanOutcome::Clean => {
                tracing::debug!("got clean scan, requesting finalization");
                let message = submission_finalization_requested_message(&work, request_id);
                let transaction_work = work.clone();
                let updated = self
                    .repository
                    .in_transaction(async move |transaction| {
                        let updated = transaction
                            .request_submission_finalization(&transaction_work)
                            .await?;
                        if updated {
                            transaction.append_outbox_message(&message).await?;
                        }
                        Ok(updated)
                    })
                    .await
                    .map_err(retryable)?;
                if updated {
                    emit_worker_submission_audit(
                        "evidence_submission_scan.completed",
                        AuditOutcome::Success,
                        &work,
                        request_id,
                        "finalizing",
                    );
                }
                Ok(())
            }
            MalwareScanOutcome::Malicious { reason } => {
                tracing::debug!("scan found a virus, marking submission as malicious");
                let updated = self.mark_malicious(&work, reason).await?;
                if updated {
                    emit_worker_submission_audit(
                        "evidence_submission_scan.completed",
                        AuditOutcome::Failure,
                        &work,
                        request_id,
                        "contains_virus",
                    );
                }
                Ok(())
            }
            MalwareScanOutcome::Failed { reason } => {
                tracing::debug!("scan failed");
                let updated = self.mark_failed(&work, reason).await?;
                if updated {
                    emit_worker_submission_audit(
                        "evidence_submission_scan.completed",
                        AuditOutcome::Failure,
                        &work,
                        request_id,
                        "failed_upload",
                    );
                }
                Ok(())
            }
        }
    }

    async fn mark_malicious(
        &self,
        work: &PendingSubmissionUploadWork,
        reason: impl AsRef<str>,
    ) -> Result<bool, RetryableWorkerError> {
        let reason = reason.as_ref();
        let updated = self
            .repository
            .mark_submission_contains_virus(work.evidence_submission_id, &work.object_key)
            .await
            .map_err(retryable)?;
        tracing::warn!(
            evidence_submission_id = %work.evidence_submission_id,
            object_key = %work.object_key,
            scanner_reason = reason,
            "submission scan detected malicious content"
        );
        Ok(updated)
    }

    async fn mark_failed(
        &self,
        work: &PendingSubmissionUploadWork,
        reason: impl AsRef<str>,
    ) -> Result<bool, RetryableWorkerError> {
        let reason = reason.as_ref();
        let updated = self
            .repository
            .mark_submission_upload_failed(work.evidence_submission_id, &work.object_key)
            .await
            .map_err(retryable)?;
        tracing::warn!(
            evidence_submission_id = %work.evidence_submission_id,
            object_key = %work.object_key,
            scanner_reason = reason,
            "submission scan failed terminally"
        );
        Ok(updated)
    }
}

fn emit_worker_submission_audit(
    event_name: &'static str,
    outcome: AuditOutcome,
    work: &PendingSubmissionUploadWork,
    request_id: Option<Uuid>,
    lifecycle_status: &'static str,
) {
    let mut event = AuditEvent::new(
        event_name,
        outcome,
        AuditActor::System { name: "worker" },
        AuditClientType::Worker,
        "handle_submission_scan",
    )
    .workspace_id(work.workspace_id.into())
    .metadata(
        "evidence_submission_id",
        Uuid::from(work.evidence_submission_id),
    )
    .metadata(
        "evidence_submission_id",
        Uuid::from(work.evidence_submission_id),
    )
    .metadata("lifecycle_status", lifecycle_status)
    .object(AuditObject::new(
        "evidence_submission",
        work.evidence_submission_id.into(),
    ));
    if let Some(request_id) = request_id {
        event = event.request_id(request_id);
    }
    event.emit();
}

fn submission_finalization_requested_message(
    work: &PendingSubmissionUploadWork,
    request_id: Option<Uuid>,
) -> NewOutboxMessage {
    NewOutboxMessage {
        topic: TopicName::new(MESSAGE_BUS_TOPIC),
        event_type: SUBMISSION_FINALIZATION_REQUESTED.to_owned(),
        aggregate_type: "evidence_submission".to_owned(),
        aggregate_id: Uuid::from(work.evidence_submission_id).to_string(),
        payload: serde_json::json!({
            "evidence_id": Uuid::from(work.evidence_id).to_string(),
            "object_key": work.object_key,
        }),
        request_id,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ScanRequestedPayload {
    evidence_submission_id: EvidenceSubmissionId,
    object_key: ObjectKey,
}

impl ScanRequestedPayload {
    fn try_from_message(message: &WorkerMessage) -> Result<Self, PermanentScanMessageErrors> {
        let dto = ScanRequestedPayloadDTO::deserialize(&message.payload)
            .map_err(|_| PermanentScanMessageErrors(vec![PermanentScanMessageError::Payload]))?;

        if message.aggregate_type != "evidence_submission" {
            return Err(PermanentScanMessageErrors(vec![
                PermanentScanMessageError::AggregateType,
            ]));
        }

        let (evidence_submission_id, object_key) = validate! {
            evidence_submission_id <- validate_aggregate_id(&message.aggregate_id),
            object_key <- validate_object_key(dto.object_key),
            => (evidence_submission_id, object_key),
        }
        .into_result()
        .map_err(PermanentScanMessageErrors)?;

        Ok(Self {
            evidence_submission_id,
            object_key,
        })
    }
}

fn validate_aggregate_id(
    value: &str,
) -> Validation<EvidenceSubmissionId, PermanentScanMessageError> {
    Uuid::parse_str(value)
        .map(EvidenceSubmissionId::from)
        .map(Validation::valid)
        .unwrap_or_else(|_| Validation::invalid(PermanentScanMessageError::AggregateId))
}

fn validate_object_key(value: String) -> Validation<ObjectKey, PermanentScanMessageError> {
    ObjectKey::parse(value)
        .map(Validation::valid)
        .unwrap_or_else(|_| Validation::invalid(PermanentScanMessageError::Key))
}

#[derive(Debug, Deserialize)]
struct ScanRequestedPayloadDTO {
    object_key: String,
}

#[derive(Debug, Error, PartialEq, Eq)]
#[allow(clippy::enum_variant_names)]
enum PermanentScanMessageError {
    #[error("invalid aggregate type")]
    AggregateType,
    #[error("invalid scan-request payload")]
    Payload,
    #[error("invalid aggregate id")]
    AggregateId,
    #[error("invalid quarantine object key")]
    Key,
}

#[derive(Debug, Error, PartialEq, Eq)]
#[error("invalid submission scan message: {0:?}")]
struct PermanentScanMessageErrors(Vec<PermanentScanMessageError>);

fn scan_content_length(work: &PendingSubmissionUploadWork) -> Result<u64, RetryableWorkerError> {
    u64::try_from(work.content_length)
        .map_err(|_| RetryableWorkerError("pending submission has negative length".to_owned()))
}

fn scan_error(error: MalwareScanError) -> RetryableWorkerError {
    RetryableWorkerError(format!("malware scanner adapter failed: {error}"))
}

fn retryable(error: impl ToString) -> RetryableWorkerError {
    RetryableWorkerError(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_payload_takes_its_submission_from_the_aggregate_id() {
        let submission_id = Uuid::new_v4();
        let key = quarantine_key();

        let payload = ScanRequestedPayload::try_from_message(&message(submission_id, &key))
            .expect("payload parses");

        assert_eq!(Uuid::from(payload.evidence_submission_id), submission_id);
        assert_eq!(payload.object_key.as_str(), key);
    }

    #[test]
    fn scan_payload_parsing_rejects_permanent_payload_errors() {
        let submission_id = Uuid::new_v4();
        let key = quarantine_key();

        let mut invalid = message(submission_id, &key);
        invalid.payload = serde_json::json!({});
        assert_eq!(
            ScanRequestedPayload::try_from_message(&invalid).unwrap_err(),
            PermanentScanMessageErrors(vec![PermanentScanMessageError::Payload])
        );

        let mut invalid_aggregate_type = message(submission_id, &key);
        invalid_aggregate_type.aggregate_type = "unsupported_aggregate".to_owned();
        assert_eq!(
            ScanRequestedPayload::try_from_message(&invalid_aggregate_type).unwrap_err(),
            PermanentScanMessageErrors(vec![PermanentScanMessageError::AggregateType])
        );

        let mut invalid_fields = message(submission_id, "not/workspace/key");
        invalid_fields.aggregate_id = "not-a-uuid".to_owned();
        assert_eq!(
            ScanRequestedPayload::try_from_message(&invalid_fields).unwrap_err(),
            PermanentScanMessageErrors(vec![
                PermanentScanMessageError::AggregateId,
                PermanentScanMessageError::Key,
            ])
        );
    }

    fn quarantine_key() -> String {
        format!(
            "workspaces/{}/quarantine/evidence/{}/submissions/{}/manual.txt",
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4()
        )
    }

    fn message(submission_id: Uuid, object_key: &str) -> WorkerMessage {
        WorkerMessage {
            message_id: "message-1".to_owned(),
            event_type: "submission.scan_requested".to_owned(),
            aggregate_type: "evidence_submission".to_owned(),
            aggregate_id: submission_id.to_string(),
            request_id: Some(Uuid::from_u128(1)),
            payload: serde_json::json!({ "object_key": object_key }),
            delivery_attempt: Some(1),
        }
    }
}
