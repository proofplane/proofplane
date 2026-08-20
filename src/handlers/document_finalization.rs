use std::sync::Arc;

use uuid::Uuid;

use crate::{
    application::{
        commands::documents::{
            DocumentCommandOutcome, FinalizeDocument,
            FinalizeDocumentHandler as FinalizeDocumentCommandHandler,
        },
        ExecutionMetadata,
    },
    domain::{DocumentId, DocumentIdentity, EvidenceSubmissionId, PolicyId},
    object_storage::{ObjectKey, ObjectStore, RuntimeObjectStore, StorageError},
    observability::audit::{AuditActor, AuditClientType, AuditEvent, AuditObject, AuditOutcome},
    persistence::{Postgres, TypedDocumentUploadWork},
    worker::{RetryableWorkerError, WorkerMessage},
};

#[derive(Clone)]
pub struct DocumentFinalizationHandler {
    repository: Arc<Postgres>,
    object_store: Arc<RuntimeObjectStore>,
    command_handler: FinalizeDocumentCommandHandler,
}

impl DocumentFinalizationHandler {
    pub fn new(repository: Arc<Postgres>, object_store: Arc<RuntimeObjectStore>) -> Self {
        Self {
            command_handler: FinalizeDocumentCommandHandler::new(repository.clone()),
            repository,
            object_store,
        }
    }

    pub async fn handle_finalization_requested(
        &self,
        message: WorkerMessage,
    ) -> Result<(), RetryableWorkerError> {
        let payload = match FinalizationRequestedPayload::try_from_message(&message) {
            Some(payload) => payload,
            None => {
                tracing::warn!(
                    message_id = %message.message_id,
                    "skipping invalid document finalization message"
                );
                return Ok(());
            }
        };

        let reads = self.repository.reads().await.map_err(retryable)?;
        let Some(work) = reads
            .documents()
            .load_finalizing_upload_work(payload.identity, payload.object_key.as_str())
            .await
            .map_err(retryable)?
        else {
            tracing::info!(
                document_id = %payload.identity.document_uuid(),
                "skipping duplicate or stale document finalization message"
            );
            return Ok(());
        };

        tracing::debug!("finalizing document");

        let final_key = final_document_object_key(&work).map_err(retryable)?;
        let copied = self
            .object_store
            .copy_object(&payload.object_key, &final_key)
            .await
            .map_err(retryable)?;

        if copied.key != final_key
            || copied.content_type != work.content_type
            || copied.content_length != work.content_length as u64
            || copied.sha256 != work.checksum_sha256
        {
            self.object_store
                .delete_object(&final_key)
                .await
                .inspect_err(|error| {
                    tracing::warn!(
                        error = %error,
                        "failed to delete document object after finalization integrity mismatch"
                    );
                })
                .ok();
            return Err(retryable(StorageError::Integrity));
        }

        tracing::debug!("object copied");

        let outcome = self
            .command_handler
            .handle(
                FinalizeDocument {
                    identity: work.identity,
                    quarantine_object_key: work.object_key.clone(),
                    final_object_key: final_key.as_str().to_owned(),
                },
                worker_metadata(&message),
            )
            .await
            .map_err(retryable)?;

        tracing::debug!("document marked as uploaded in repository");

        if outcome == DocumentCommandOutcome::Applied {
            emit_worker_finalization_audit(&work, message.request_id);
            self.object_store
                .delete_object(&payload.object_key)
                .await
                .inspect_err(|error| {
                    tracing::warn!(
                        error = %error,
                        "failed to delete quarantined document object after finalization"
                    );
                })
                .ok();
        }

        Ok(())
    }
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

fn emit_worker_finalization_audit(work: &TypedDocumentUploadWork, request_id: Option<Uuid>) {
    let mut event = AuditEvent::new(
        finalization_event_name(work.identity),
        AuditOutcome::Success,
        AuditActor::System { name: "worker" },
        AuditClientType::Worker,
        "handle_document_finalization",
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
    event = event.metadata("lifecycle_status", "uploaded");
    if let Some(request_id) = request_id {
        event = event.request_id(request_id);
    }
    event.emit();
}

struct FinalizationRequestedPayload {
    identity: DocumentIdentity,
    object_key: ObjectKey,
}

impl FinalizationRequestedPayload {
    fn try_from_message(message: &WorkerMessage) -> Option<Self> {
        let document_id = Uuid::parse_str(&message.aggregate_id).ok()?;
        let object_key =
            ObjectKey::parse(message.payload.get("object_key")?.as_str()?.to_owned()).ok()?;
        let identity = match message.aggregate_type.as_str() {
            "evidence_document" => {
                let owner_id = payload_uuid(&message.payload, "evidence_submission_id")?;
                DocumentIdentity::Evidence {
                    evidence_submission_id: EvidenceSubmissionId::from(owner_id),
                    document_id: DocumentId::from(document_id),
                }
            }
            "policy_document" => {
                let owner_id = payload_uuid(&message.payload, "policy_id")?;
                DocumentIdentity::Policy {
                    policy_id: PolicyId::from(owner_id),
                    document_id: DocumentId::from(document_id),
                }
            }
            _ => return None,
        };

        Some(Self {
            identity,
            object_key,
        })
    }
}

fn final_document_object_key(
    work: &TypedDocumentUploadWork,
) -> Result<ObjectKey, crate::object_storage::StorageError> {
    let prefix = match work.identity {
        DocumentIdentity::Evidence {
            evidence_submission_id,
            document_id: evidence_document_id,
        } => format!(
            "evidence-submissions/{evidence_submission_id}/documents/{evidence_document_id}"
        ),
        DocumentIdentity::Policy {
            policy_id,
            document_id: policy_document_id,
        } => {
            format!("policies/{policy_id}/documents/{policy_document_id}")
        }
    };
    ObjectKey::new(work.workspace_id, prefix, &work.filename)
}

fn payload_uuid(payload: &serde_json::Value, field: &str) -> Option<Uuid> {
    Uuid::parse_str(payload.get(field)?.as_str()?).ok()
}

fn finalization_event_name(identity: DocumentIdentity) -> &'static str {
    match identity {
        DocumentIdentity::Evidence { .. } => "evidence_document_finalization.completed",
        DocumentIdentity::Policy { .. } => "policy_document_finalization.completed",
    }
}

fn retryable(error: impl ToString) -> RetryableWorkerError {
    RetryableWorkerError(error.to_string())
}

#[cfg(test)]
mod tests {
    use bytes::Bytes;
    use futures_util::stream;

    use crate::{
        config::ObjectStorageConfig,
        domain::{DocumentUploadStatus, WorkspaceId},
        object_storage::{PutObjectRequest, StorageError},
        persistence::{param, test_support},
    };

    use super::*;

    #[tokio::test]
    async fn mismatched_copied_metadata_leaves_the_document_finalizing() {
        let database = test_support::database().await;
        let postgres = Arc::new(database.postgres);
        let workspace = test_support::workspace(&postgres, "Finalization owner").await;
        let policy_id =
            test_support::policy(&postgres, workspace.workspace_id, "Access policy").await;
        let document_id = DocumentId::from(Uuid::new_v4());
        let identity = DocumentIdentity::Policy {
            policy_id,
            document_id,
        };
        let quarantine_key = ObjectKey::new(
            workspace.workspace_id,
            format!("quarantine/policies/{policy_id}/documents/{document_id}"),
            "manual.txt",
        )
        .unwrap();
        let root = std::env::temp_dir().join(format!(
            "proofplane-finalization-integrity-{}",
            Uuid::new_v4()
        ));
        let object_store = Arc::new(
            RuntimeObjectStore::from_config(&ObjectStorageConfig::Filesystem { root })
                .await
                .unwrap(),
        );
        object_store
            .put_object(PutObjectRequest {
                key: quarantine_key.clone(),
                content_type: "text/plain".to_owned(),
                chunks: stream::once(async { Ok(Bytes::from_static(b"wrong")) }),
            })
            .await
            .unwrap();

        let client = postgres.get().await.unwrap();
        client
            .execute_typed(
                r#"
INSERT INTO documents (
    id, workspace_id, owner_type, owner_id, created_by_user_id, filename,
    content_type, content_length, object_key, checksum_sha256, checksum_crc32c,
    upload_status
)
VALUES ($1, $2, 'policy', $3, $4, 'manual.txt', 'text/plain', 6, $5, $6, 'ignored',
        'finalizing')
"#,
                &[
                    param(&Uuid::from(document_id)),
                    param(&Uuid::from(workspace.workspace_id)),
                    param(&Uuid::from(policy_id)),
                    param(&Uuid::from(workspace.user_id)),
                    param(&quarantine_key.as_str()),
                    param(&"f1b2f12c3f2c85eab7c8b2f87a735d66e4ebdc7a8e03c8bd421bd66c835033cd"),
                ],
            )
            .await
            .unwrap();
        drop(client);

        let message = WorkerMessage {
            message_id: Uuid::new_v4().to_string(),
            event_type: "document.finalization_requested".to_owned(),
            aggregate_type: "policy_document".to_owned(),
            aggregate_id: Uuid::from(document_id).to_string(),
            request_id: None,
            payload: serde_json::json!({
                "policy_id": Uuid::from(policy_id).to_string(),
                "object_key": quarantine_key.as_str(),
            }),
            delivery_attempt: Some(1),
        };

        let result = DocumentFinalizationHandler::new(postgres.clone(), object_store.clone())
            .handle_finalization_requested(message)
            .await;

        assert!(result.is_err(), "metadata mismatch remains retryable");
        assert!(postgres
            .reads()
            .await
            .unwrap()
            .documents()
            .load_finalizing_upload_work(identity, quarantine_key.as_str())
            .await
            .unwrap()
            .is_some());
        let final_key = ObjectKey::new(
            workspace.workspace_id,
            format!("policies/{policy_id}/documents/{document_id}"),
            "manual.txt",
        )
        .unwrap();
        assert!(matches!(
            object_store.head_object(&final_key).await,
            Err(StorageError::NotFound)
        ));
    }

    #[test]
    fn finalization_payload_parses_valid_message_and_rejects_invalid_message() {
        let document_id = Uuid::new_v4();
        let submission_id = Uuid::new_v4();
        let object_key = format!(
            "workspaces/{}/quarantine/evidence-submissions/{submission_id}/documents/upload/manual.txt",
            Uuid::new_v4()
        );
        let mut message = WorkerMessage {
            message_id: "message-1".to_owned(),
            event_type: "document.finalization_requested".to_owned(),
            aggregate_type: "evidence_document".to_owned(),
            aggregate_id: document_id.to_string(),
            request_id: None,
            payload: serde_json::json!({
                "evidence_submission_id": submission_id.to_string(),
                "object_key": object_key,
            }),
            delivery_attempt: Some(1),
        };

        let payload =
            FinalizationRequestedPayload::try_from_message(&message).expect("payload parses");
        assert_eq!(
            payload.identity,
            DocumentIdentity::Evidence {
                evidence_submission_id: submission_id.into(),
                document_id: document_id.into(),
            }
        );

        message.aggregate_type = "evidence_submission".to_owned();
        assert!(FinalizationRequestedPayload::try_from_message(&message).is_none());
    }

    #[test]
    fn final_document_key_uses_stable_document_path() {
        let workspace_id = Uuid::new_v4();
        let submission_id = Uuid::new_v4();
        let document_id = Uuid::new_v4();
        let work = TypedDocumentUploadWork {
            workspace_id: WorkspaceId::from(workspace_id),
            identity: DocumentIdentity::Evidence {
                evidence_submission_id: submission_id.into(),
                document_id: document_id.into(),
            },
            filename: "manual.txt".to_owned(),
            content_type: "text/plain".to_owned(),
            content_length: 5,
            object_key: "unused".to_owned(),
            checksum_sha256: "checksum".to_owned(),
            upload_status: DocumentUploadStatus::Finalizing,
        };

        assert_eq!(
            final_document_object_key(&work)
                .expect("key is valid")
                .as_str(),
            format!(
                "workspaces/{workspace_id}/evidence-submissions/{submission_id}/documents/{document_id}/manual.txt"
            )
        );

        let policy_id = Uuid::new_v4();
        let policy_document_id = Uuid::new_v4();
        let policy_work = TypedDocumentUploadWork {
            identity: DocumentIdentity::Policy {
                policy_id: policy_id.into(),
                document_id: policy_document_id.into(),
            },
            ..work
        };
        assert_eq!(
            final_document_object_key(&policy_work)
                .expect("policy key is valid")
                .as_str(),
            format!(
                "workspaces/{workspace_id}/policies/{policy_id}/documents/{policy_document_id}/manual.txt"
            )
        );
    }
}
