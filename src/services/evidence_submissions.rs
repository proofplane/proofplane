use std::sync::Arc;

use crate::{
    domain::{
        CoverageWindow, CreateDocumentPayload, CreateEvidenceSubmissionPayload, Document,
        DocumentId, DocumentOwner, EvidenceId, EvidenceSubmissionDetail, EvidenceSubmissionId,
    },
    object_storage::{FilesystemObjectStore, StorageError},
    pubsub::{TopicName, MESSAGE_BUS_TOPIC},
    repository::{ArchiveDocumentResult, NewOutboxMessage, Postgres},
    services::Error,
    worker::DOCUMENT_SCAN_REQUESTED,
};

use super::{
    agent_connections::AgentConnectionContext,
    documents::{delete_staged_document, stage_evidence_document},
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

pub struct StageEvidenceDocumentInput<S> {
    pub evidence_submission_id: EvidenceSubmissionId,
    pub filename: String,
    pub content_type: String,
    pub max_bytes: usize,
    pub chunks: S,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StagedEvidenceDocument {
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

    pub async fn get(
        &self,
        connection: AgentConnectionContext,
        id: EvidenceSubmissionId,
    ) -> Result<Option<EvidenceSubmissionDetail>, Error> {
        Ok(self
            .repository
            .in_workspace_context_read(connection.workspace_id, async move |context| {
                context.get_evidence_submission(id).await
            })
            .await?)
    }

    pub async fn list_for_evidence(
        &self,
        connection: AgentConnectionContext,
        evidence_id: EvidenceId,
    ) -> Result<Vec<EvidenceSubmissionDetail>, Error> {
        Ok(self
            .repository
            .in_workspace_context_read(connection.workspace_id, async move |context| {
                context.list_evidence_submissions(evidence_id).await
            })
            .await?)
    }

    pub async fn list_for_coverage(
        &self,
        connection: AgentConnectionContext,
        evidence_id: EvidenceId,
        coverage: CoverageWindow,
    ) -> Result<Vec<EvidenceSubmissionDetail>, Error> {
        Ok(self
            .repository
            .in_workspace_context_read(connection.workspace_id, async move |context| {
                context
                    .list_evidence_submissions_for_coverage(evidence_id, coverage)
                    .await
            })
            .await?)
    }

    pub async fn latest_for_evidence(
        &self,
        connection: AgentConnectionContext,
        evidence_id: EvidenceId,
    ) -> Result<Option<EvidenceSubmissionDetail>, Error> {
        Ok(self
            .repository
            .in_workspace_context_read(connection.workspace_id, async move |context| {
                context
                    .latest_evidence_submission_for_evidence(evidence_id)
                    .await
            })
            .await?)
    }

    pub async fn stage_document<S>(
        &self,
        connection: &AgentConnectionContext,
        input: StageEvidenceDocumentInput<S>,
    ) -> Result<StagedEvidenceDocument, Error>
    where
        S: Stream<Item = Result<Bytes, StorageError>> + Send,
    {
        let staged = stage_evidence_document(
            &self.object_store,
            connection.workspace_id,
            input.evidence_submission_id,
            input.filename,
            input.content_type,
            input.max_bytes,
            input.chunks,
        )
        .await?;
        Ok(StagedEvidenceDocument {
            evidence_submission_id: input.evidence_submission_id,
            filename: staged.filename,
            content_type: staged.content_type,
            content_length: staged.content_length,
            object_key: staged.object_key,
            checksum_sha256: staged.checksum_sha256,
            checksum_crc32c: staged.checksum_crc32c,
        })
    }

    pub async fn delete_uploaded_document_object(&self, object_key: &str) -> Result<(), Error> {
        delete_staged_document(&self.object_store, object_key).await
    }

    pub async fn create_submission(
        &self,
        connection: &AgentConnectionContext,
        request_id: Uuid,
        submission_id: EvidenceSubmissionId,
        evidence_id: EvidenceId,
        coverage: CoverageWindow,
        payload: StagedEvidenceDocument,
    ) -> Result<Option<Document>, Error> {
        let object_key = payload.object_key.clone();
        let submission_payload = CreateEvidenceSubmissionPayload {
            id: submission_id,
            evidence_id,
            coverage,
        };
        let document_payload = CreateDocumentPayload {
            owner: DocumentOwner::EvidenceSubmission(submission_id),
            filename: payload.filename,
            content_type: payload.content_type,
            content_length: payload.content_length,
            object_key: payload.object_key,
            checksum_sha256: payload.checksum_sha256,
            checksum_crc32c: payload.checksum_crc32c,
        };

        let result = self
            .repository
            .in_agent_connection_workspace_context(
                connection.workspace_id,
                connection.user_id,
                connection.connection_id,
                async move |context| {
                    if context
                        .create_evidence_submission(&submission_payload)
                        .await?
                        .is_none()
                    {
                        return Ok(None);
                    }
                    let document = context.create_evidence_document(&document_payload).await?;
                    context
                        .append_outbox_message(&document_scan_requested_message(
                            &document, request_id,
                        ))
                        .await?;

                    Ok(Some(document))
                },
            )
            .await;

        match result {
            Ok(Some(document)) => Ok(Some(document)),
            Ok(None) => {
                let _ = self.delete_uploaded_document_object(&object_key).await;
                Ok(None)
            }
            Err(error) => {
                let _ = self.delete_uploaded_document_object(&object_key).await;
                Err(error.into())
            }
        }
    }

    pub async fn archive_document(
        &self,
        connection: &AgentConnectionContext,
        submission_id: EvidenceSubmissionId,
        document_id: DocumentId,
    ) -> Result<ArchiveDocumentResult, Error> {
        Ok(self
            .repository
            .in_agent_connection_workspace_context(
                connection.workspace_id,
                connection.user_id,
                connection.connection_id,
                async move |context| {
                    context
                        .archive_evidence_document(submission_id, document_id)
                        .await
                },
            )
            .await?)
    }
}

fn document_scan_requested_message(document: &Document, request_id: Uuid) -> NewOutboxMessage {
    NewOutboxMessage {
        topic: TopicName::new(MESSAGE_BUS_TOPIC),
        event_type: DOCUMENT_SCAN_REQUESTED.to_owned(),
        aggregate_type: "evidence_document".to_owned(),
        aggregate_id: Uuid::from(document.id()).to_string(),
        payload: serde_json::json!({
            "evidence_submission_id": document.owner().owner_uuid().to_string(),
            "object_key": document.object_key,
        }),
        request_id: Some(request_id),
    }
}
