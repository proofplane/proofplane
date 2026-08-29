use std::sync::Arc;

use bytes::Bytes;
use chrono::Utc;
use futures_core::Stream;
use uuid::Uuid;

use crate::{
    application::{
        commands::documents::{
            CompleteAgentEvidenceUpload, CompleteAgentEvidenceUploadHandler,
            CompleteAgentEvidenceUploadOutcome, DocumentCommandError,
        },
        ExecutionMetadata,
    },
    domain::{
        AgentEvidenceUploadAuthority, AgentEvidenceUploadGrant,
        AgentEvidenceUploadGrantError as DomainGrantError, AgentEvidenceUploadGrantId,
        CreateDocumentPayload, Document, DocumentId, DocumentOwner, EvidenceSubmissionId,
        WorkspaceId,
    },
    object_storage::StorageError,
    observability::{
        agent_evidence_uploads::{
            record_attempt, record_received_bytes, AgentEvidenceUploadAttemptResult,
        },
        audit::{AuditActor, AuditClientType, AuditEvent, AuditObject, AuditOutcome},
    },
    persistence::{Error as RepositoryError, Postgres},
};

use super::{
    agent_evidence_upload_grants::{
        AgentEvidenceUploadCredentialVerifier, AgentEvidenceUploadGrantError,
    },
    evidence_submissions::{EvidenceSubmissionService, StagedEvidenceDocument},
    Error as ServiceError,
};

#[derive(Clone)]
pub struct AgentEvidenceUploadService {
    repository: Arc<Postgres>,
    submissions: EvidenceSubmissionService,
    credential_verifier: AgentEvidenceUploadCredentialVerifier,
    max_document_bytes: usize,
    complete_upload: CompleteAgentEvidenceUploadHandler,
}

pub struct AgentEvidenceUploadResult {
    pub submission_id: EvidenceSubmissionId,
    pub document: Document,
}

pub enum AgentEvidenceUploadOutcome {
    Created(AgentEvidenceUploadResult),
    Replayed(AgentEvidenceUploadResult),
}

impl AgentEvidenceUploadOutcome {
    pub fn result(&self) -> &AgentEvidenceUploadResult {
        match self {
            Self::Created(result) | Self::Replayed(result) => result,
        }
    }
}

impl AgentEvidenceUploadService {
    pub fn new(
        repository: Arc<Postgres>,
        submissions: EvidenceSubmissionService,
        credential_verifier: AgentEvidenceUploadCredentialVerifier,
        max_document_bytes: usize,
    ) -> Self {
        Self {
            complete_upload: CompleteAgentEvidenceUploadHandler::new(repository.clone()),
            repository,
            submissions,
            credential_verifier,
            max_document_bytes,
        }
    }

    pub async fn upload<S>(
        &self,
        upload_id: AgentEvidenceUploadGrantId,
        credential: &str,
        content_type: &str,
        content_length: u64,
        request_id: Uuid,
        chunks: S,
    ) -> Result<AgentEvidenceUploadOutcome, AgentEvidenceUploadError>
    where
        S: Stream<Item = Result<Bytes, StorageError>> + Send,
    {
        let authority = match self.credential_verifier.verify(credential) {
            Ok(authority) => authority,
            Err(error) => {
                record_attempt(AgentEvidenceUploadAttemptResult::Unavailable);
                return Err(error.into());
            }
        };
        if authority.upload_id() != upload_id {
            record_attempt(AgentEvidenceUploadAttemptResult::Unavailable);
            return Err(AgentEvidenceUploadError::Unavailable);
        }
        let grant = match self
            .repository
            .agent_evidence_upload_grants()
            .get(upload_id, authority.workspace_id())
            .await
        {
            Ok(Some(grant)) => grant,
            Ok(None) => {
                record_attempt(AgentEvidenceUploadAttemptResult::Unavailable);
                return Err(AgentEvidenceUploadError::Unavailable);
            }
            Err(error) => {
                record_attempt(AgentEvidenceUploadAttemptResult::DatabaseFailed);
                return Err(error.into());
            }
        };
        if grant.matches_authority(&authority).is_err() {
            record_attempt(AgentEvidenceUploadAttemptResult::Unavailable);
            return Err(AgentEvidenceUploadError::Unavailable);
        }
        let configured_max = u64::try_from(self.max_document_bytes)
            .map_err(|_| AgentEvidenceUploadError::PayloadTooLarge)?;
        if content_length > configured_max {
            record_attempt(AgentEvidenceUploadAttemptResult::ValidationRejected);
            return Err(AgentEvidenceUploadError::PayloadTooLarge);
        }
        if let Err(error) = grant.validate_declared_file(content_type, content_length) {
            record_attempt(AgentEvidenceUploadAttemptResult::ValidationRejected);
            return Err(error.into());
        }

        let completed_document = match grant.completed_document_at(Utc::now()) {
            Ok(completed_document) => completed_document,
            Err(_) => {
                record_attempt(AgentEvidenceUploadAttemptResult::Unavailable);
                return Err(AgentEvidenceUploadError::Unavailable);
            }
        };
        if let Some(document_id) = completed_document {
            let replay = self.load_completed_result(&grant, document_id).await;
            match replay {
                Ok(result) => {
                    record_attempt(AgentEvidenceUploadAttemptResult::Replayed);
                    return Ok(AgentEvidenceUploadOutcome::Replayed(result));
                }
                Err(error) => {
                    record_attempt(AgentEvidenceUploadAttemptResult::DatabaseFailed);
                    return Err(error);
                }
            }
        }

        let staged = match self
            .submissions
            .stage_agent_upload(&grant, self.max_document_bytes, chunks)
            .await
        {
            Ok(staged) => staged,
            Err(
                error @ ServiceError::Storage(StorageError::StreamRead {
                    payload_too_large: true,
                    ..
                }),
            ) => {
                record_attempt(AgentEvidenceUploadAttemptResult::ValidationRejected);
                return Err(error.into());
            }
            Err(error @ ServiceError::Storage(StorageError::StreamRead { .. })) => {
                record_attempt(AgentEvidenceUploadAttemptResult::StreamFailed);
                return Err(error.into());
            }
            Err(error @ ServiceError::Storage(_)) => {
                record_attempt(AgentEvidenceUploadAttemptResult::StorageFailed);
                return Err(error.into());
            }
            Err(error @ ServiceError::Repository(_)) => {
                record_attempt(AgentEvidenceUploadAttemptResult::DatabaseFailed);
                return Err(error.into());
            }
        };
        if let Ok(received_bytes) = u64::try_from(staged.content_length) {
            record_received_bytes(received_bytes);
        }
        if let Err(error) =
            grant.validate_staged_file(staged.content_length, &staged.checksum_sha256)
        {
            self.delete_staged(&staged.object_key, request_id).await;
            record_attempt(AgentEvidenceUploadAttemptResult::ValidationRejected);
            return Err(error.into());
        }

        self.complete(grant, authority, request_id, staged).await
    }

    async fn complete(
        &self,
        grant: AgentEvidenceUploadGrant,
        authority: AgentEvidenceUploadAuthority,
        request_id: Uuid,
        staged: StagedEvidenceDocument,
    ) -> Result<AgentEvidenceUploadOutcome, AgentEvidenceUploadError> {
        let object_key = staged.object_key.clone();
        let upload_id = grant.id();
        let workspace_id = grant.workspace_id();
        let evidence_id = grant.evidence_id();
        let user_id = grant.issued_by_user_id();
        let agent_connection_id = grant.issued_via_agent_connection_id();
        let result = self
            .complete_upload
            .handle(
                CompleteAgentEvidenceUpload {
                    grant,
                    authority,
                    staged_submission_id: staged.evidence_submission_id,
                    document_id: DocumentId::from(Uuid::new_v4()),
                    document: CreateDocumentPayload {
                        owner: DocumentOwner::EvidenceSubmission(staged.evidence_submission_id),
                        filename: staged.filename,
                        content_type: staged.content_type,
                        content_length: staged.content_length,
                        object_key: staged.object_key,
                        checksum_sha256: staged.checksum_sha256,
                        checksum_crc32c: staged.checksum_crc32c,
                    },
                    completed_at: Utc::now(),
                },
                ExecutionMetadata::for_request(request_id),
            )
            .await;

        match result {
            Ok(CompleteAgentEvidenceUploadOutcome::Created {
                submission_id,
                document,
            }) => {
                let result = AgentEvidenceUploadResult {
                    submission_id,
                    document,
                };
                record_attempt(AgentEvidenceUploadAttemptResult::Created);
                AuditEvent::new(
                    "agent_evidence_upload.completed",
                    AuditOutcome::Success,
                    AuditActor::AgentConnection {
                        user_id: user_id.into(),
                        agent_connection_id: agent_connection_id.into(),
                    },
                    AuditClientType::Rest,
                    "upload_agent_evidence",
                )
                .workspace_id(workspace_id.into())
                .request_id(request_id)
                .metadata("evidence_id", Uuid::from(evidence_id))
                .metadata("evidence_submission_id", Uuid::from(result.submission_id))
                .metadata("evidence_document_id", Uuid::from(result.document.id()))
                .metadata("lifecycle_status", result.document.upload_status.as_str())
                .object(AuditObject::new(
                    "agent_evidence_upload_grant",
                    upload_id.into(),
                ))
                .emit();
                Ok(AgentEvidenceUploadOutcome::Created(result))
            }
            Ok(CompleteAgentEvidenceUploadOutcome::Replayed {
                submission_id,
                document_id,
            }) => {
                let result = self
                    .load_completed_result_by_id(workspace_id, submission_id, document_id)
                    .await;
                self.delete_staged(&object_key, request_id).await;
                match result {
                    Ok(result) => {
                        record_attempt(AgentEvidenceUploadAttemptResult::ConcurrencyLost);
                        Ok(AgentEvidenceUploadOutcome::Replayed(result))
                    }
                    Err(error) => {
                        record_attempt(AgentEvidenceUploadAttemptResult::DatabaseFailed);
                        Err(error)
                    }
                }
            }
            Ok(CompleteAgentEvidenceUploadOutcome::Unavailable)
            | Err(DocumentCommandError::Unavailable | DocumentCommandError::Invalid) => {
                self.delete_staged(&object_key, request_id).await;
                record_attempt(AgentEvidenceUploadAttemptResult::Unavailable);
                Err(AgentEvidenceUploadError::Unavailable)
            }
            Err(error) => {
                self.delete_staged(&object_key, request_id).await;
                record_attempt(AgentEvidenceUploadAttemptResult::DatabaseFailed);
                Err(match error {
                    DocumentCommandError::Repository(error) => error.into(),
                    DocumentCommandError::Unavailable | DocumentCommandError::Invalid => {
                        AgentEvidenceUploadError::Unavailable
                    }
                })
            }
        }
    }

    async fn delete_staged(&self, object_key: &str, request_id: Uuid) {
        if let Err(error) = self.submissions.delete_staged_object(object_key).await {
            crate::observability::record_cleanup_failure(
                &error,
                "agent_evidence_upload_cleanup",
                Some(request_id),
            );
        }
    }

    async fn load_completed_result(
        &self,
        grant: &AgentEvidenceUploadGrant,
        document_id: DocumentId,
    ) -> Result<AgentEvidenceUploadResult, AgentEvidenceUploadError> {
        self.load_completed_result_by_id(grant.workspace_id(), grant.submission_id(), document_id)
            .await
    }

    async fn load_completed_result_by_id(
        &self,
        workspace_id: WorkspaceId,
        submission_id: EvidenceSubmissionId,
        document_id: DocumentId,
    ) -> Result<AgentEvidenceUploadResult, AgentEvidenceUploadError> {
        let document = self
            .submissions
            .get_agent_upload_document(workspace_id, submission_id, document_id)
            .await?
            .ok_or(RepositoryError::InvariantViolation(
                "completed machine upload document is missing",
            ))?;
        Ok(AgentEvidenceUploadResult {
            submission_id,
            document,
        })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum AgentEvidenceUploadError {
    #[error("agent evidence upload is unavailable")]
    Unavailable,
    #[error("agent evidence upload exceeds the configured limit")]
    PayloadTooLarge,
    #[error(transparent)]
    Grant(#[from] DomainGrantError),
    #[error(transparent)]
    GrantCredential(#[from] AgentEvidenceUploadGrantError),
    #[error(transparent)]
    Service(#[from] ServiceError),
    #[error(transparent)]
    Repository(#[from] RepositoryError),
}
