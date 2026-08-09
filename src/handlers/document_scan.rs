use std::sync::Arc;

use futures_util::StreamExt;
use thiserror::Error;
use uuid::Uuid;

use crate::{
    application::{
        commands::documents::{
            DocumentCommandOutcome, ScanDocument,
            ScanDocumentHandler as ScanDocumentCommandHandler, ScanDocumentResult,
        },
        ExecutionMetadata,
    },
    domain::{DocumentId, DocumentIdentity, EvidenceSubmissionId, PolicyId},
    object_storage::{FilesystemObjectStore, ObjectKey, ObjectStore, StorageError},
    observability::audit::{AuditActor, AuditClientType, AuditEvent, AuditObject, AuditOutcome},
    repository::{Postgres, TypedDocumentUploadWork},
    scanner::{ClamAvMalwareScanner, MalwareScanError, MalwareScanOutcome, MalwareScanResult},
    worker::{RetryableWorkerError, WorkerMessage},
};

const MISSING_OBJECT_FAILURE_REASON: &str = "quarantined object was not found";

pub struct DocumentScanHandler {
    repository: Arc<Postgres>,
    object_store: Arc<FilesystemObjectStore>,
    scanner: Arc<ClamAvMalwareScanner>,
    max_delivery_attempts: u16,
    command_handler: ScanDocumentCommandHandler,
}

impl Clone for DocumentScanHandler {
    fn clone(&self) -> Self {
        Self {
            repository: self.repository.clone(),
            object_store: self.object_store.clone(),
            scanner: self.scanner.clone(),
            max_delivery_attempts: self.max_delivery_attempts,
            command_handler: self.command_handler.clone(),
        }
    }
}

impl DocumentScanHandler {
    pub fn new(
        repository: Arc<Postgres>,
        object_store: Arc<FilesystemObjectStore>,
        scanner: Arc<ClamAvMalwareScanner>,
        max_delivery_attempts: u16,
    ) -> Self {
        Self {
            command_handler: ScanDocumentCommandHandler::new(repository.clone()),
            repository,
            object_store,
            scanner,
            max_delivery_attempts,
        }
    }
}

impl DocumentScanHandler {
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
                    "skipping invalid document scan message"
                );
                return Ok(());
            }
        };

        let Some(work) = self
            .repository
            .load_pending_typed_document_upload_work(payload.identity, payload.object_key.as_str())
            .await
            .map_err(retryable)?
        else {
            tracing::info!(
                document_id = %payload.identity.document_uuid(),
                "skipping duplicate or stale document scan message"
            );
            return Ok(());
        };

        tracing::debug!("initiating scan");

        let quarantine_key = ObjectKey::parse(work.object_key.clone()).map_err(retryable)?;
        let object = match self.object_store.get_object(&quarantine_key).await {
            Ok(object) => object,
            Err(StorageError::NotFound) => {
                let updated = self
                    .mark_failed(&work, MISSING_OBJECT_FAILURE_REASON, &message)
                    .await?;
                if updated {
                    emit_worker_document_audit(
                        scan_event_name(work.identity),
                        AuditOutcome::Failure,
                        &work,
                        message.request_id,
                        "failed_upload",
                    );
                }
                return Ok(());
            }
            Err(error) if final_delivery => {
                let updated = self.mark_failed(&work, error.to_string(), &message).await?;
                if updated {
                    emit_worker_document_audit(
                        scan_event_name(work.identity),
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
                reason: "stored object metadata does not match document metadata".to_owned(),
            };
            if final_delivery {
                let updated = self.mark_failed(&work, error.to_string(), &message).await?;
                if updated {
                    emit_worker_document_audit(
                        scan_event_name(work.identity),
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
                let updated = self.mark_failed(&work, error.to_string(), &message).await?;
                if updated {
                    emit_worker_document_audit(
                        scan_event_name(work.identity),
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

        self.apply_scan_result(work, scan_result, &message).await
    }

    async fn apply_scan_result(
        &self,
        work: TypedDocumentUploadWork,
        scan_result: MalwareScanResult,
        message: &WorkerMessage,
    ) -> Result<(), RetryableWorkerError> {
        match scan_result.outcome {
            MalwareScanOutcome::Clean => {
                tracing::debug!("got clean scan, requesting finalization");
                let updated = self
                    .apply_result(&work, ScanDocumentResult::Clean, message)
                    .await?;
                if updated {
                    emit_worker_document_audit(
                        scan_event_name(work.identity),
                        AuditOutcome::Success,
                        &work,
                        message.request_id,
                        "finalizing",
                    );
                }
                Ok(())
            }
            MalwareScanOutcome::Malicious { reason } => {
                tracing::debug!("scan found a virus, marking document as malicious");
                let updated = self.mark_malicious(&work, reason, message).await?;
                if updated {
                    emit_worker_document_audit(
                        scan_event_name(work.identity),
                        AuditOutcome::Failure,
                        &work,
                        message.request_id,
                        "contains_virus",
                    );
                }
                Ok(())
            }
            MalwareScanOutcome::Failed { reason } => {
                tracing::debug!("scan failed");
                let updated = self.mark_failed(&work, reason, message).await?;
                if updated {
                    emit_worker_document_audit(
                        scan_event_name(work.identity),
                        AuditOutcome::Failure,
                        &work,
                        message.request_id,
                        "failed_upload",
                    );
                }
                Ok(())
            }
        }
    }

    async fn mark_malicious(
        &self,
        work: &TypedDocumentUploadWork,
        _reason: impl AsRef<str>,
        message: &WorkerMessage,
    ) -> Result<bool, RetryableWorkerError> {
        let updated = self
            .apply_result(work, ScanDocumentResult::Malicious, message)
            .await?;
        tracing::warn!(
            document_id = %work.identity.document_uuid(),
            "document scan detected malicious content"
        );
        Ok(updated)
    }

    async fn mark_failed(
        &self,
        work: &TypedDocumentUploadWork,
        _reason: impl AsRef<str>,
        message: &WorkerMessage,
    ) -> Result<bool, RetryableWorkerError> {
        let updated = self
            .apply_result(work, ScanDocumentResult::Failed, message)
            .await?;
        tracing::warn!(
            document_id = %work.identity.document_uuid(),
            "document scan failed terminally"
        );
        Ok(updated)
    }

    async fn apply_result(
        &self,
        work: &TypedDocumentUploadWork,
        result: ScanDocumentResult,
        message: &WorkerMessage,
    ) -> Result<bool, RetryableWorkerError> {
        let outcome = self
            .command_handler
            .handle(
                ScanDocument {
                    identity: work.identity,
                    object_key: work.object_key.clone(),
                    result,
                },
                worker_metadata(message),
            )
            .await
            .map_err(retryable)?;
        Ok(outcome == DocumentCommandOutcome::Applied)
    }
}

fn emit_worker_document_audit(
    event_name: &'static str,
    outcome: AuditOutcome,
    work: &TypedDocumentUploadWork,
    request_id: Option<Uuid>,
    lifecycle_status: &'static str,
) {
    let mut event = AuditEvent::new(
        event_name,
        outcome,
        AuditActor::System { name: "worker" },
        AuditClientType::Worker,
        "handle_document_scan",
    )
    .workspace_id(work.workspace_id.into());
    event = match work.identity {
        DocumentIdentity::Evidence {
            evidence_submission_id,
            document_id: evidence_document_id,
        } => event
            .metadata("evidence_submission_id", Uuid::from(evidence_submission_id))
            .metadata("evidence_document_id", Uuid::from(evidence_document_id))
            .object(AuditObject::new(
                "evidence_document",
                evidence_document_id.into(),
            )),
        DocumentIdentity::Policy {
            policy_id,
            document_id: policy_document_id,
        } => event
            .metadata("policy_id", Uuid::from(policy_id))
            .metadata("policy_document_id", Uuid::from(policy_document_id))
            .object(AuditObject::new(
                "policy_document",
                policy_document_id.into(),
            )),
    };
    event = event.metadata("lifecycle_status", lifecycle_status);
    if let Some(request_id) = request_id {
        event = event.request_id(request_id);
    }
    event.emit();
}

fn worker_metadata(message: &WorkerMessage) -> ExecutionMetadata {
    let mut metadata = ExecutionMetadata::background();
    if let Some(correlation_id) = message.request_id {
        metadata = metadata.with_correlation_id(correlation_id);
    }
    if let Ok(causation_id) = Uuid::parse_str(&message.message_id) {
        metadata = metadata.with_causation_id(causation_id);
    }
    metadata
}

fn scan_event_name(identity: DocumentIdentity) -> &'static str {
    match identity {
        DocumentIdentity::Evidence { .. } => "evidence_document_scan.completed",
        DocumentIdentity::Policy { .. } => "policy_document_scan.completed",
    }
}

fn payload_uuid(payload: &serde_json::Value, field: &str) -> Result<Uuid, ()> {
    payload
        .get(field)
        .and_then(|value| value.as_str())
        .ok_or(())
        .and_then(|value| Uuid::parse_str(value).map_err(|_| ()))
}

fn errors(error: PermanentScanMessageError) -> PermanentScanMessageErrors {
    PermanentScanMessageErrors(vec![error])
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ScanRequestedPayload {
    identity: DocumentIdentity,
    object_key: ObjectKey,
}

impl ScanRequestedPayload {
    fn try_from_message(message: &WorkerMessage) -> Result<Self, PermanentScanMessageErrors> {
        let document_id = Uuid::parse_str(&message.aggregate_id)
            .map_err(|_| errors(PermanentScanMessageError::AggregateId))?;
        let object_key = message
            .payload
            .get("object_key")
            .and_then(|value| value.as_str())
            .ok_or_else(|| errors(PermanentScanMessageError::Payload))?;
        let object_key = ObjectKey::parse(object_key.to_owned())
            .map_err(|_| errors(PermanentScanMessageError::Key))?;
        let identity = match message.aggregate_type.as_str() {
            "evidence_document" => {
                let owner_id = payload_uuid(&message.payload, "evidence_submission_id")
                    .map_err(|_| errors(PermanentScanMessageError::OwnerId))?;
                DocumentIdentity::Evidence {
                    evidence_submission_id: EvidenceSubmissionId::from(owner_id),
                    document_id: DocumentId::from(document_id),
                }
            }
            "policy_document" => {
                let owner_id = payload_uuid(&message.payload, "policy_id")
                    .map_err(|_| errors(PermanentScanMessageError::OwnerId))?;
                DocumentIdentity::Policy {
                    policy_id: PolicyId::from(owner_id),
                    document_id: DocumentId::from(document_id),
                }
            }
            _ => return Err(errors(PermanentScanMessageError::AggregateType)),
        };

        Ok(Self {
            identity,
            object_key,
        })
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
#[allow(clippy::enum_variant_names)]
enum PermanentScanMessageError {
    #[error("invalid aggregate type")]
    AggregateType,
    #[error("invalid scan-request payload")]
    Payload,
    #[error("invalid document owner id")]
    OwnerId,
    #[error("invalid aggregate id")]
    AggregateId,
    #[error("invalid quarantine object key")]
    Key,
}

#[derive(Debug, Error, PartialEq, Eq)]
#[error("invalid document scan message: {0:?}")]
struct PermanentScanMessageErrors(Vec<PermanentScanMessageError>);

fn scan_content_length(work: &TypedDocumentUploadWork) -> Result<u64, RetryableWorkerError> {
    u64::try_from(work.content_length)
        .map_err(|_| RetryableWorkerError("pending document has negative length".to_owned()))
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
        let document_id = Uuid::new_v4();
        let submission_id = Uuid::new_v4();
        let workspace_id = Uuid::new_v4();
        let key = format!(
            "workspaces/{workspace_id}/quarantine/evidence-submissions/{submission_id}/documents/upload/manual.txt"
        );

        let payload =
            ScanRequestedPayload::try_from_message(&message(document_id, submission_id, &key))
                .expect("payload parses");

        assert_eq!(
            payload.identity,
            DocumentIdentity::Evidence {
                evidence_submission_id: submission_id.into(),
                document_id: document_id.into(),
            }
        );
        assert_eq!(payload.object_key.as_str(), key);
    }

    #[test]
    fn scan_payload_parsing_rejects_permanent_payload_errors() {
        let document_id = Uuid::new_v4();
        let submission_id = Uuid::new_v4();
        let key = format!(
            "workspaces/{}/quarantine/evidence-submissions/{submission_id}/documents/upload/manual.txt",
            Uuid::new_v4()
        );

        let mut invalid = message(document_id, submission_id, &key);
        invalid.payload = serde_json::json!({});
        assert_eq!(
            ScanRequestedPayload::try_from_message(&invalid).unwrap_err(),
            PermanentScanMessageErrors(vec![PermanentScanMessageError::Payload])
        );

        let mut invalid_aggregate_type = message(document_id, submission_id, &key);
        invalid_aggregate_type.aggregate_type = "evidence_submission".to_owned();
        assert_eq!(
            ScanRequestedPayload::try_from_message(&invalid_aggregate_type).unwrap_err(),
            PermanentScanMessageErrors(vec![PermanentScanMessageError::AggregateType])
        );

        let mut invalid_submission_id = message(document_id, submission_id, &key);
        invalid_submission_id.payload["evidence_submission_id"] =
            serde_json::Value::String("not-a-uuid".to_owned());
        assert_eq!(
            ScanRequestedPayload::try_from_message(&invalid_submission_id).unwrap_err(),
            PermanentScanMessageErrors(vec![PermanentScanMessageError::OwnerId])
        );

        let mut invalid_fields = message(document_id, submission_id, "not/workspace/key");
        invalid_fields.aggregate_id = "not-a-uuid".to_owned();
        assert_eq!(
            ScanRequestedPayload::try_from_message(&invalid_fields).unwrap_err(),
            PermanentScanMessageErrors(vec![PermanentScanMessageError::AggregateId])
        );
    }

    #[test]
    fn scan_payload_parsing_accepts_policy_owner() {
        let document_id = Uuid::new_v4();
        let policy_id = Uuid::new_v4();
        let workspace_id = Uuid::new_v4();
        let key = format!(
            "workspaces/{workspace_id}/quarantine/policies/{policy_id}/documents/upload/manual.txt"
        );
        let mut message = message(document_id, policy_id, &key);
        message.aggregate_type = "policy_document".to_owned();
        message.payload = serde_json::json!({
            "policy_id": policy_id.to_string(),
            "object_key": key,
        });

        let payload =
            ScanRequestedPayload::try_from_message(&message).expect("policy payload parses");
        assert_eq!(
            payload.identity,
            DocumentIdentity::Policy {
                policy_id: policy_id.into(),
                document_id: document_id.into(),
            }
        );
    }

    fn message(document_id: Uuid, submission_id: Uuid, object_key: &str) -> WorkerMessage {
        WorkerMessage {
            message_id: "message-1".to_owned(),
            event_type: "document.scan_requested".to_owned(),
            aggregate_type: "evidence_document".to_owned(),
            aggregate_id: document_id.to_string(),
            request_id: Some(Uuid::from_u128(1)),
            payload: serde_json::json!({
                "evidence_submission_id": submission_id.to_string(),
                "object_key": object_key,
            }),
            delivery_attempt: Some(1),
        }
    }
}
