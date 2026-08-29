use std::sync::Arc;

use bytes::Bytes;
use chrono::Utc;
use futures_core::Stream;
use uuid::Uuid;

use crate::{
    application::{
        commands::documents::{
            CompleteAgentPolicyDocumentUpload, CompleteAgentPolicyDocumentUploadHandler,
            CompleteAgentPolicyDocumentUploadOutcome, DocumentCommandError,
        },
        ExecutionMetadata,
    },
    domain::{
        AgentPolicyDocumentUploadAuthority, AgentPolicyDocumentUploadGrant,
        AgentPolicyDocumentUploadGrantError as DomainGrantError, AgentPolicyDocumentUploadGrantId,
        CreateDocumentPayload, Document, DocumentId, DocumentOwner, PolicyId, WorkspaceId,
    },
    object_storage::{QuarantineObjectStore, StorageError},
    observability::{
        agent_policy_document_uploads::{
            record_attempt, record_received_bytes, AgentPolicyDocumentUploadAttemptResult,
        },
        audit::{AuditActor, AuditClientType, AuditEvent, AuditObject, AuditOutcome},
    },
    persistence::{Error as RepositoryError, PolicyDocumentUploadEligibility, Postgres},
};

use super::{
    agent_policy_document_upload_grants::{
        AgentPolicyDocumentUploadCredentialVerifier, AgentPolicyDocumentUploadGrantError,
    },
    policy_documents::{PolicyDocumentService, UploadPolicyDocumentPayload},
    Error as ServiceError,
};

#[derive(Clone)]
pub struct AgentPolicyDocumentUploadService {
    repository: Arc<Postgres>,
    documents: PolicyDocumentService,
    credential_verifier: AgentPolicyDocumentUploadCredentialVerifier,
    max_document_bytes: usize,
    complete_upload: CompleteAgentPolicyDocumentUploadHandler,
}

pub struct AgentPolicyDocumentUploadResult {
    pub policy_id: PolicyId,
    pub document: Document,
}

pub enum AgentPolicyDocumentUploadOutcome {
    Created(AgentPolicyDocumentUploadResult),
    Replayed(AgentPolicyDocumentUploadResult),
}

#[derive(Clone, Copy)]
struct UploadCompletionAuditContext {
    upload_id: AgentPolicyDocumentUploadGrantId,
    workspace_id: WorkspaceId,
    policy_id: PolicyId,
    user_id: crate::domain::UserId,
    agent_connection_id: crate::domain::AgentConnectionId,
    request_id: Uuid,
}

impl AgentPolicyDocumentUploadOutcome {
    pub fn result(&self) -> &AgentPolicyDocumentUploadResult {
        match self {
            Self::Created(result) | Self::Replayed(result) => result,
        }
    }
}

impl AgentPolicyDocumentUploadService {
    pub fn new(
        repository: Arc<Postgres>,
        quarantine_store: QuarantineObjectStore,
        credential_verifier: AgentPolicyDocumentUploadCredentialVerifier,
        max_document_bytes: usize,
    ) -> Self {
        Self {
            documents: PolicyDocumentService::new(repository.clone(), quarantine_store),
            complete_upload: CompleteAgentPolicyDocumentUploadHandler::new(repository.clone()),
            repository,
            credential_verifier,
            max_document_bytes,
        }
    }

    pub async fn upload<S>(
        &self,
        upload_id: AgentPolicyDocumentUploadGrantId,
        credential: &str,
        content_type: &str,
        content_length: u64,
        request_id: Uuid,
        chunks: S,
    ) -> Result<AgentPolicyDocumentUploadOutcome, AgentPolicyDocumentUploadError>
    where
        S: Stream<Item = Result<Bytes, StorageError>> + Send,
    {
        let authority = match self.credential_verifier.verify(credential) {
            Ok(authority) => authority,
            Err(error) => {
                record_attempt(AgentPolicyDocumentUploadAttemptResult::Unavailable);
                return Err(error.into());
            }
        };
        if authority.upload_id() != upload_id {
            record_attempt(AgentPolicyDocumentUploadAttemptResult::Unavailable);
            return Err(AgentPolicyDocumentUploadError::Unavailable);
        }
        let grant = match self
            .repository
            .agent_policy_document_upload_grants()
            .get(upload_id, authority.workspace_id())
            .await
        {
            Ok(Some(grant)) => grant,
            Ok(None) => {
                record_attempt(AgentPolicyDocumentUploadAttemptResult::Unavailable);
                return Err(AgentPolicyDocumentUploadError::Unavailable);
            }
            Err(error) => {
                record_attempt(AgentPolicyDocumentUploadAttemptResult::DatabaseFailed);
                return Err(error.into());
            }
        };
        if grant.matches_authority(&authority).is_err() {
            record_attempt(AgentPolicyDocumentUploadAttemptResult::Unavailable);
            return Err(AgentPolicyDocumentUploadError::Unavailable);
        }
        let configured_max = u64::try_from(self.max_document_bytes)
            .map_err(|_| AgentPolicyDocumentUploadError::PayloadTooLarge)?;
        if content_length > configured_max {
            record_attempt(AgentPolicyDocumentUploadAttemptResult::ValidationRejected);
            return Err(AgentPolicyDocumentUploadError::PayloadTooLarge);
        }
        if let Err(error) = grant.validate_declared_file(content_type, content_length) {
            record_attempt(AgentPolicyDocumentUploadAttemptResult::ValidationRejected);
            return Err(error.into());
        }

        let completed_document = match grant.completed_document_at(Utc::now()) {
            Ok(completed_document) => completed_document,
            Err(_) => {
                record_attempt(AgentPolicyDocumentUploadAttemptResult::Unavailable);
                return Err(AgentPolicyDocumentUploadError::Unavailable);
            }
        };
        if let Some(document_id) = completed_document {
            return match self.load_completed_result(&grant, document_id).await {
                Ok(result) => {
                    record_attempt(AgentPolicyDocumentUploadAttemptResult::Replayed);
                    Ok(AgentPolicyDocumentUploadOutcome::Replayed(result))
                }
                Err(error) => {
                    record_attempt(AgentPolicyDocumentUploadAttemptResult::DatabaseFailed);
                    Err(error)
                }
            };
        }

        if let Err(error) = self.ensure_policy_eligible(&grant).await {
            record_attempt(match &error {
                AgentPolicyDocumentUploadError::CurrentDocument => {
                    AgentPolicyDocumentUploadAttemptResult::CurrentDocument
                }
                AgentPolicyDocumentUploadError::Unavailable => {
                    AgentPolicyDocumentUploadAttemptResult::Unavailable
                }
                _ => AgentPolicyDocumentUploadAttemptResult::DatabaseFailed,
            });
            return Err(error);
        }
        let staged = match self
            .documents
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
                record_attempt(AgentPolicyDocumentUploadAttemptResult::ValidationRejected);
                return Err(error.into());
            }
            Err(error @ ServiceError::Storage(StorageError::StreamRead { .. })) => {
                record_attempt(AgentPolicyDocumentUploadAttemptResult::StreamFailed);
                return Err(error.into());
            }
            Err(error @ ServiceError::Storage(_)) => {
                record_attempt(AgentPolicyDocumentUploadAttemptResult::StorageFailed);
                return Err(error.into());
            }
            Err(error @ ServiceError::Repository(_)) => {
                record_attempt(AgentPolicyDocumentUploadAttemptResult::DatabaseFailed);
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
            record_attempt(AgentPolicyDocumentUploadAttemptResult::ValidationRejected);
            return Err(error.into());
        }

        self.complete(grant, authority, request_id, staged).await
    }

    async fn ensure_policy_eligible(
        &self,
        grant: &AgentPolicyDocumentUploadGrant,
    ) -> Result<(), AgentPolicyDocumentUploadError> {
        let policy_id = grant.policy_id();
        let eligibility = self
            .repository
            .in_unit_of_work(async move |unit_of_work| {
                let workspace = unit_of_work.workspace(grant.workspace_id());
                workspace
                    .reads()
                    .policies()
                    .lock_document_upload_eligibility(policy_id)
                    .await
            })
            .await?;
        match eligibility {
            Some(PolicyDocumentUploadEligibility::Eligible) => Ok(()),
            Some(PolicyDocumentUploadEligibility::CurrentDocument) => {
                Err(AgentPolicyDocumentUploadError::CurrentDocument)
            }
            None => Err(AgentPolicyDocumentUploadError::Unavailable),
        }
    }

    async fn complete(
        &self,
        grant: AgentPolicyDocumentUploadGrant,
        authority: AgentPolicyDocumentUploadAuthority,
        request_id: Uuid,
        staged: UploadPolicyDocumentPayload,
    ) -> Result<AgentPolicyDocumentUploadOutcome, AgentPolicyDocumentUploadError> {
        let object_key = staged.object_key.clone();
        let upload_id = grant.id();
        let workspace_id = grant.workspace_id();
        let policy_id = grant.policy_id();
        let user_id = grant.issued_by_user_id();
        let agent_connection_id = grant.issued_via_agent_connection_id();
        let audit_context = UploadCompletionAuditContext {
            upload_id,
            workspace_id,
            policy_id,
            user_id,
            agent_connection_id,
            request_id,
        };
        let result = self
            .complete_upload
            .handle(
                CompleteAgentPolicyDocumentUpload {
                    grant,
                    authority,
                    policy_id,
                    document_id: DocumentId::from(Uuid::new_v4()),
                    document: CreateDocumentPayload {
                        owner: DocumentOwner::Policy(policy_id),
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
            Ok(CompleteAgentPolicyDocumentUploadOutcome::Created(document)) => {
                let result = AgentPolicyDocumentUploadResult {
                    policy_id,
                    document,
                };
                self.record_created(audit_context, &result);
                Ok(AgentPolicyDocumentUploadOutcome::Created(result))
            }
            Ok(CompleteAgentPolicyDocumentUploadOutcome::Replayed(document_id)) => {
                let result = self
                    .load_completed_result_by_id(workspace_id, policy_id, document_id)
                    .await;
                self.delete_staged(&object_key, request_id).await;
                match result {
                    Ok(result) => {
                        record_attempt(AgentPolicyDocumentUploadAttemptResult::ConcurrencyLost);
                        Ok(AgentPolicyDocumentUploadOutcome::Replayed(result))
                    }
                    Err(error) => {
                        record_attempt(AgentPolicyDocumentUploadAttemptResult::DatabaseFailed);
                        Err(error)
                    }
                }
            }
            Ok(CompleteAgentPolicyDocumentUploadOutcome::CurrentDocument) => {
                self.delete_staged(&object_key, request_id).await;
                record_attempt(AgentPolicyDocumentUploadAttemptResult::CurrentDocument);
                Err(AgentPolicyDocumentUploadError::CurrentDocument)
            }
            Ok(CompleteAgentPolicyDocumentUploadOutcome::Unavailable)
            | Err(DocumentCommandError::Unavailable | DocumentCommandError::Invalid) => {
                self.delete_staged(&object_key, request_id).await;
                record_attempt(AgentPolicyDocumentUploadAttemptResult::Unavailable);
                Err(AgentPolicyDocumentUploadError::Unavailable)
            }
            Err(error) => {
                let error = match error {
                    DocumentCommandError::Repository(error) => error,
                    DocumentCommandError::Unavailable | DocumentCommandError::Invalid => {
                        RepositoryError::InvariantViolation(
                            "validated machine policy upload command was rejected",
                        )
                    }
                };
                match self
                    .reconcile_failed_completion(
                        upload_id,
                        workspace_id,
                        policy_id,
                        &authority,
                        &object_key,
                    )
                    .await
                {
                    Ok(Some((outcome, staged_attempt_won))) => {
                        if !staged_attempt_won {
                            self.delete_staged(&object_key, request_id).await;
                            record_attempt(AgentPolicyDocumentUploadAttemptResult::ConcurrencyLost);
                        } else {
                            self.record_created(audit_context, outcome.result());
                        }
                        Ok(outcome)
                    }
                    Ok(None) => {
                        self.delete_staged(&object_key, request_id).await;
                        record_attempt(AgentPolicyDocumentUploadAttemptResult::DatabaseFailed);
                        Err(error.into())
                    }
                    Err(_) => {
                        // Reconciliation failure leaves commit status unknown. Preserve the
                        // staged object: deleting it could corrupt a document whose transaction
                        // committed before the database connection failed.
                        record_attempt(AgentPolicyDocumentUploadAttemptResult::DatabaseFailed);
                        Err(error.into())
                    }
                }
            }
        }
    }

    fn record_created(
        &self,
        context: UploadCompletionAuditContext,
        result: &AgentPolicyDocumentUploadResult,
    ) {
        record_attempt(AgentPolicyDocumentUploadAttemptResult::Created);
        AuditEvent::new(
            "agent_policy_document_upload.completed",
            AuditOutcome::Success,
            AuditActor::AgentConnection {
                user_id: context.user_id.into(),
                agent_connection_id: context.agent_connection_id.into(),
            },
            AuditClientType::Rest,
            "upload_agent_policy_document",
        )
        .workspace_id(context.workspace_id.into())
        .request_id(context.request_id)
        .metadata("policy_id", Uuid::from(context.policy_id))
        .metadata("policy_document_id", Uuid::from(result.document.id()))
        .metadata("lifecycle_status", result.document.upload_status.as_str())
        .object(AuditObject::new(
            "agent_policy_document_upload_grant",
            context.upload_id.into(),
        ))
        .emit();
    }

    async fn reconcile_failed_completion(
        &self,
        upload_id: AgentPolicyDocumentUploadGrantId,
        workspace_id: WorkspaceId,
        policy_id: PolicyId,
        authority: &AgentPolicyDocumentUploadAuthority,
        staged_object_key: &str,
    ) -> Result<Option<(AgentPolicyDocumentUploadOutcome, bool)>, AgentPolicyDocumentUploadError>
    {
        let Some(grant) = self
            .repository
            .agent_policy_document_upload_grants()
            .get(upload_id, workspace_id)
            .await?
        else {
            return Ok(None);
        };
        if grant.matches_authority(authority).is_err() {
            return Ok(None);
        }
        let Some(document_id) = grant.document_id() else {
            return Ok(None);
        };
        let result = self
            .load_completed_result_by_id(workspace_id, policy_id, document_id)
            .await?;
        let staged_attempt_won = result.document.object_key == staged_object_key;
        let outcome = if staged_attempt_won {
            AgentPolicyDocumentUploadOutcome::Created(result)
        } else {
            AgentPolicyDocumentUploadOutcome::Replayed(result)
        };
        Ok(Some((outcome, staged_attempt_won)))
    }

    async fn delete_staged(&self, object_key: &str, request_id: Uuid) {
        if let Err(error) = self.documents.delete_staged_object(object_key).await {
            crate::observability::record_cleanup_failure(
                &error,
                "agent_policy_document_upload_cleanup",
                Some(request_id),
            );
        }
    }

    async fn load_completed_result(
        &self,
        grant: &AgentPolicyDocumentUploadGrant,
        document_id: DocumentId,
    ) -> Result<AgentPolicyDocumentUploadResult, AgentPolicyDocumentUploadError> {
        self.load_completed_result_by_id(grant.workspace_id(), grant.policy_id(), document_id)
            .await
    }

    async fn load_completed_result_by_id(
        &self,
        workspace_id: WorkspaceId,
        policy_id: PolicyId,
        document_id: DocumentId,
    ) -> Result<AgentPolicyDocumentUploadResult, AgentPolicyDocumentUploadError> {
        let document = self
            .documents
            .get_agent_upload_document(workspace_id, policy_id, document_id)
            .await?
            .ok_or(RepositoryError::InvariantViolation(
                "completed policy machine upload document is missing",
            ))?;
        Ok(AgentPolicyDocumentUploadResult {
            policy_id,
            document,
        })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum AgentPolicyDocumentUploadError {
    #[error("agent policy document upload is unavailable")]
    Unavailable,
    #[error("policy already has a current document")]
    CurrentDocument,
    #[error("agent policy document upload exceeds the configured limit")]
    PayloadTooLarge,
    #[error(transparent)]
    Grant(#[from] DomainGrantError),
    #[error(transparent)]
    GrantCredential(#[from] AgentPolicyDocumentUploadGrantError),
    #[error(transparent)]
    Service(#[from] ServiceError),
    #[error(transparent)]
    Repository(#[from] RepositoryError),
}
