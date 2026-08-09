use std::sync::Arc;

use crate::{
    application::{
        commands::documents::{
            ArchiveDocument, ArchiveDocumentHandler, ArchiveDocumentOutcome,
            CreateEvidenceSubmissionDocument, CreateEvidenceSubmissionDocumentHandler,
            CreatedDocument, DocumentCommandError,
        },
        ExecutionMetadata,
    },
    authentication::AgentConnectionContext,
    domain::{
        AgentEvidenceUploadGrant, CoverageWindow, CreateDocumentPayload, Document, DocumentId,
        DocumentIdentity, DocumentOwner, EvidenceId, EvidenceSubmissionDetail,
        EvidenceSubmissionId, WorkspaceId,
    },
    object_storage::{FilesystemObjectStore, StorageError},
    repository::{ArchiveDocumentResult, Postgres},
    services::Error,
};

use super::documents::{delete_staged_document, stage_evidence_document};
use bytes::Bytes;
use futures_core::Stream;
use uuid::Uuid;

pub struct EvidenceSubmissionService {
    repository: Arc<Postgres>,
    object_store: Arc<FilesystemObjectStore>,
    create_document: CreateEvidenceSubmissionDocumentHandler,
    archive_document: ArchiveDocumentHandler,
}

impl Clone for EvidenceSubmissionService {
    fn clone(&self) -> Self {
        Self {
            repository: self.repository.clone(),
            object_store: self.object_store.clone(),
            create_document: self.create_document.clone(),
            archive_document: self.archive_document.clone(),
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
            create_document: CreateEvidenceSubmissionDocumentHandler::new(repository.clone()),
            archive_document: ArchiveDocumentHandler::new(repository.clone()),
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

    pub(crate) async fn stage_agent_upload<S>(
        &self,
        grant: &AgentEvidenceUploadGrant,
        max_bytes: usize,
        chunks: S,
    ) -> Result<StagedEvidenceDocument, Error>
    where
        S: Stream<Item = Result<Bytes, StorageError>> + Send,
    {
        let staged = stage_evidence_document(
            &self.object_store,
            grant.workspace_id(),
            grant.submission_id(),
            grant.declaration().filename().to_owned(),
            grant.declaration().content_type().to_owned(),
            max_bytes,
            chunks,
        )
        .await?;
        Ok(StagedEvidenceDocument {
            evidence_submission_id: grant.submission_id(),
            filename: staged.filename,
            content_type: staged.content_type,
            content_length: staged.content_length,
            object_key: staged.object_key,
            checksum_sha256: staged.checksum_sha256,
            checksum_crc32c: staged.checksum_crc32c,
        })
    }

    pub(crate) async fn get_agent_upload_document(
        &self,
        workspace_id: WorkspaceId,
        submission_id: EvidenceSubmissionId,
        document_id: DocumentId,
    ) -> Result<Option<Document>, Error> {
        Ok(self
            .repository
            .in_workspace_context_read(workspace_id, async move |context| {
                context
                    .get_agent_upload_document(submission_id, document_id)
                    .await
            })
            .await?)
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
        let document_id = DocumentId::from(Uuid::new_v4());
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
            .create_document
            .handle(
                CreateEvidenceSubmissionDocument {
                    connection: *connection,
                    submission_id,
                    evidence_id,
                    coverage,
                    document_id,
                    document: document_payload,
                    received_at: chrono::Utc::now(),
                },
                ExecutionMetadata::for_request(request_id),
            )
            .await;

        match result {
            Ok(CreatedDocument::Created(document) | CreatedDocument::Replayed(document)) => {
                Ok(Some(document))
            }
            Err(DocumentCommandError::Unavailable | DocumentCommandError::Invalid) => {
                let _ = self.delete_uploaded_document_object(&object_key).await;
                Ok(None)
            }
            Err(error) => {
                let _ = self.delete_uploaded_document_object(&object_key).await;
                Err(command_error(error))
            }
        }
    }

    pub async fn archive_document(
        &self,
        connection: &AgentConnectionContext,
        submission_id: EvidenceSubmissionId,
        document_id: DocumentId,
    ) -> Result<ArchiveDocumentResult, Error> {
        let outcome = self
            .archive_document
            .handle(
                ArchiveDocument {
                    connection: *connection,
                    identity: DocumentIdentity::Evidence {
                        evidence_submission_id: submission_id,
                        document_id,
                    },
                },
                ExecutionMetadata::background(),
            )
            .await
            .map_err(command_error)?;
        Ok(match outcome {
            ArchiveDocumentOutcome::Archived => ArchiveDocumentResult::Archived,
            ArchiveDocumentOutcome::Replayed => ArchiveDocumentResult::Archived,
            ArchiveDocumentOutcome::Unavailable => ArchiveDocumentResult::NotFound,
            ArchiveDocumentOutcome::NotTerminal => ArchiveDocumentResult::NotTerminal,
        })
    }
}

fn command_error(error: DocumentCommandError) -> Error {
    match error {
        DocumentCommandError::Repository(error) => Error::Repository(error),
        DocumentCommandError::Unavailable | DocumentCommandError::Invalid => Error::Repository(
            crate::repository::Error::InvariantViolation("validated document command was rejected"),
        ),
    }
}
