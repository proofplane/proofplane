use std::sync::Arc;

use bytes::Bytes;
use chrono::Utc;
use futures_core::Stream;
use uuid::Uuid;

use crate::{
    domain::{
        AgentEvidenceUploadGrant, AgentEvidenceUploadGrantError as DomainGrantError,
        AgentEvidenceUploadGrantId, CreateDocumentPayload, CreateEvidenceSubmissionPayload,
        Document, DocumentOwner,
    },
    object_storage::StorageError,
    repository::{Error as RepositoryError, Postgres},
};

use super::{
    agent_evidence_upload_grants::{
        AgentEvidenceUploadCredentialVerifier, AgentEvidenceUploadGrantError,
    },
    evidence_submissions::{
        document_scan_requested_message, EvidenceSubmissionService, StagedEvidenceDocument,
    },
    Error as ServiceError,
};

#[derive(Clone)]
pub struct AgentEvidenceUploadService {
    repository: Arc<Postgres>,
    submissions: EvidenceSubmissionService,
    credential_verifier: AgentEvidenceUploadCredentialVerifier,
    max_document_bytes: usize,
}

pub struct AgentEvidenceUploadResult {
    pub submission_id: crate::domain::EvidenceSubmissionId,
    pub document: Document,
}

impl AgentEvidenceUploadService {
    pub fn new(
        repository: Arc<Postgres>,
        submissions: EvidenceSubmissionService,
        credential_verifier: AgentEvidenceUploadCredentialVerifier,
        max_document_bytes: usize,
    ) -> Self {
        Self {
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
    ) -> Result<AgentEvidenceUploadResult, AgentEvidenceUploadError>
    where
        S: Stream<Item = Result<Bytes, StorageError>> + Send,
    {
        let authority = self.credential_verifier.verify(credential)?;
        if authority.upload_id() != upload_id {
            return Err(AgentEvidenceUploadError::Unavailable);
        }
        let grant = self
            .repository
            .agent_evidence_upload_grants()
            .get(upload_id, authority.workspace_id())
            .await?
            .ok_or(AgentEvidenceUploadError::Unavailable)?;
        grant
            .matches_authority(&authority)
            .map_err(|_| AgentEvidenceUploadError::Unavailable)?;
        grant
            .ensure_pending_at(Utc::now())
            .map_err(|_| AgentEvidenceUploadError::Unavailable)?;

        let configured_max = u64::try_from(self.max_document_bytes)
            .map_err(|_| AgentEvidenceUploadError::PayloadTooLarge)?;
        if content_length > configured_max {
            return Err(AgentEvidenceUploadError::PayloadTooLarge);
        }
        grant.validate_declared_file(content_type, content_length)?;

        let staged = self
            .submissions
            .stage_agent_upload(&grant, self.max_document_bytes, chunks)
            .await?;
        if let Err(error) =
            grant.validate_staged_file(staged.content_length, &staged.checksum_sha256)
        {
            self.delete_staged(&staged.object_key).await;
            return Err(error.into());
        }

        self.complete(grant, authority, request_id, staged).await
    }

    async fn complete(
        &self,
        grant: AgentEvidenceUploadGrant,
        authority: crate::domain::AgentEvidenceUploadAuthority,
        request_id: Uuid,
        staged: StagedEvidenceDocument,
    ) -> Result<AgentEvidenceUploadResult, AgentEvidenceUploadError> {
        let object_key = staged.object_key.clone();
        let upload_id = grant.id();
        let workspace_id = grant.workspace_id();
        let result = self
            .repository
            .in_agent_connection_workspace_context(
                workspace_id,
                grant.issued_by_user_id(),
                grant.issued_via_agent_connection_id(),
                async move |context| {
                    let repository = context.agent_evidence_upload_grants();
                    let Some(mut locked_grant) = repository.get(upload_id, workspace_id).await?
                    else {
                        return Ok(None);
                    };
                    if locked_grant.matches_authority(&authority).is_err()
                        || locked_grant.ensure_pending_at(Utc::now()).is_err()
                        || locked_grant
                            .validate_staged_file(staged.content_length, &staged.checksum_sha256)
                            .is_err()
                    {
                        return Ok(None);
                    }
                    if staged.evidence_submission_id != locked_grant.submission_id() {
                        return Ok(None);
                    }

                    let submission = CreateEvidenceSubmissionPayload {
                        id: locked_grant.submission_id(),
                        evidence_id: locked_grant.evidence_id(),
                        coverage: locked_grant.coverage(),
                    };
                    if context
                        .create_evidence_submission(&submission)
                        .await?
                        .is_none()
                    {
                        return Ok(None);
                    }
                    let document = context
                        .create_evidence_document(&CreateDocumentPayload {
                            owner: DocumentOwner::EvidenceSubmission(locked_grant.submission_id()),
                            filename: staged.filename,
                            content_type: staged.content_type,
                            content_length: staged.content_length,
                            object_key: staged.object_key,
                            checksum_sha256: staged.checksum_sha256,
                            checksum_crc32c: staged.checksum_crc32c,
                        })
                        .await?;
                    context
                        .append_outbox_message(&document_scan_requested_message(
                            &document, request_id,
                        ))
                        .await?;
                    locked_grant
                        .complete(document.id(), Utc::now())
                        .map_err(|_| {
                            RepositoryError::InvariantViolation(
                                "machine upload grant changed during completion",
                            )
                        })?;
                    repository.save(&locked_grant).await?;
                    Ok(Some(AgentEvidenceUploadResult {
                        submission_id: locked_grant.submission_id(),
                        document,
                    }))
                },
            )
            .await;

        match result {
            Ok(Some(result)) => Ok(result),
            Ok(None) => {
                self.delete_staged(&object_key).await;
                Err(AgentEvidenceUploadError::Unavailable)
            }
            Err(error) => {
                self.delete_staged(&object_key).await;
                Err(error.into())
            }
        }
    }

    async fn delete_staged(&self, object_key: &str) {
        let _ = self
            .submissions
            .delete_uploaded_document_object(object_key)
            .await;
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
