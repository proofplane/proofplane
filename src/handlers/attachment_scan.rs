use std::sync::Arc;

use serde::Deserialize;
use thiserror::Error;
use uuid::Uuid;

use crate::{
    domain::{EvidenceAttachmentId, EvidenceSubmissionId},
    object_storage::ObjectKey,
    pubsub::{TopicName, MESSAGE_BUS_TOPIC},
    repository::{NewOutboxMessage, PendingAttachmentUploadWork, Postgres},
    scanner::{
        MalwareScanError, MalwareScanOutcome, MalwareScanResult, MalwareScanner, ScanObjectRequest,
    },
    validate,
    validation::Validation,
    worker::{RetryableWorkerError, WorkerMessage, ATTACHMENT_FINALIZATION_REQUESTED},
};

const MISSING_OBJECT_FAILURE_REASON: &str = "quarantined object was not found";
const SAFE_REASON_MAX_CHARS: usize = 512;

pub struct AttachmentScanHandler<C> {
    repository: Arc<Postgres>,
    scanner: Arc<C>,
    max_delivery_attempts: u16,
}

impl<C> Clone for AttachmentScanHandler<C> {
    fn clone(&self) -> Self {
        Self {
            repository: self.repository.clone(),
            scanner: self.scanner.clone(),
            max_delivery_attempts: self.max_delivery_attempts,
        }
    }
}

impl<C> AttachmentScanHandler<C> {
    pub fn new(repository: Arc<Postgres>, scanner: Arc<C>, max_delivery_attempts: u16) -> Self {
        Self {
            repository,
            scanner,
            max_delivery_attempts,
        }
    }
}

impl<C> AttachmentScanHandler<C>
where
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
                    "skipping invalid attachment scan message"
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
                "skipping duplicate or stale attachment scan message"
            );
            return Ok(());
        };

        if payload.evidence_submission_id != work.evidence_submission_id {
            tracing::warn!(
                evidence_attachment_id = %payload.evidence_attachment_id,
                payload_submission_id = %payload.evidence_submission_id,
                work_submission_id = %work.evidence_submission_id,
                "skipping attachment scan message with mismatched submission id"
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

        self.apply_scan_result(work, scan_result, message.request_id)
            .await
    }

    async fn apply_scan_result(
        &self,
        work: PendingAttachmentUploadWork,
        scan_result: MalwareScanResult,
        request_id: Option<Uuid>,
    ) -> Result<(), RetryableWorkerError> {
        match scan_result.outcome {
            MalwareScanOutcome::Clean => {
                let message = attachment_finalization_requested_message(&work, request_id);
                self.repository
                    .in_transaction(async move |transaction| {
                        if transaction.request_attachment_finalization(&work).await? {
                            transaction.append_outbox_message(&message).await?;
                        }
                        Ok(())
                    })
                    .await
                    .map_err(retryable)?;
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
        let reason = safe_failure_reason(reason.as_ref());
        self.repository
            .mark_attachment_contains_virus(work.evidence_attachment_id, &work.object_key, &reason)
            .await
            .map_err(retryable)?;

        Ok(())
    }

    async fn mark_failed(
        &self,
        work: &PendingAttachmentUploadWork,
        reason: impl AsRef<str>,
    ) -> Result<(), RetryableWorkerError> {
        let reason = safe_failure_reason(reason.as_ref());
        self.repository
            .mark_attachment_upload_failed(work.evidence_attachment_id, &work.object_key, &reason)
            .await
            .map_err(retryable)?;
        Ok(())
    }
}

fn attachment_finalization_requested_message(
    work: &PendingAttachmentUploadWork,
    request_id: Option<Uuid>,
) -> NewOutboxMessage {
    NewOutboxMessage {
        topic: TopicName::new(MESSAGE_BUS_TOPIC),
        event_type: ATTACHMENT_FINALIZATION_REQUESTED.to_owned(),
        aggregate_type: "evidence_attachment".to_owned(),
        aggregate_id: Uuid::from(work.evidence_attachment_id).to_string(),
        payload: serde_json::json!({
            "evidence_submission_id": Uuid::from(work.evidence_submission_id).to_string(),
            "object_key": work.object_key,
        }),
        request_id,
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

    #[test]
    fn failure_reason_is_trimmed_and_bounded() {
        let reason = format!("  {}  ", "x".repeat(SAFE_REASON_MAX_CHARS + 10));
        let sanitized = safe_failure_reason(&reason);

        assert_eq!(sanitized.chars().count(), SAFE_REASON_MAX_CHARS);
        assert!(!sanitized.starts_with(char::is_whitespace));
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
}
