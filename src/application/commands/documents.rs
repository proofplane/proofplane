use std::sync::Arc;

use crate::{
    application::ExecutionMetadata,
    authentication::AgentConnectionContext,
    domain::{
        AgentEvidenceUploadAuthority, AgentEvidenceUploadGrant, AgentPolicyDocumentUploadAuthority,
        AgentPolicyDocumentUploadGrant, CoverageWindow, CreateDocumentPayload, Document,
        DocumentEvent, DocumentId, DocumentIdentity, DocumentOwner, DocumentTransitionOutcome,
        EvidenceId, EvidenceSubmission, EvidenceSubmissionId, EvidenceSubmitter, PolicyId,
        WorkspacePermission,
    },
    messaging::IntegrationMessage,
    pubsub::{TopicName, MESSAGE_BUS_TOPIC},
    repository::{
        Error as RepositoryError, NewOutboxMessage, PolicyDocumentUploadEligibility, Postgres,
    },
};
use chrono::{DateTime, Utc};

#[derive(Debug, Clone)]
pub struct CreateEvidenceSubmissionDocument {
    pub connection: AgentConnectionContext,
    pub submission_id: EvidenceSubmissionId,
    pub evidence_id: EvidenceId,
    pub coverage: CoverageWindow,
    pub document_id: DocumentId,
    pub document: CreateDocumentPayload,
    pub received_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct CreatePolicyDocument {
    pub connection: AgentConnectionContext,
    pub policy_id: PolicyId,
    pub document_id: DocumentId,
    pub document: CreateDocumentPayload,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy)]
pub struct ArchiveDocument {
    pub connection: AgentConnectionContext,
    pub identity: DocumentIdentity,
}

#[derive(Debug, Clone)]
pub enum ScanDocumentResult {
    Clean,
    Malicious,
    Failed,
}

#[derive(Debug, Clone)]
pub struct ScanDocument {
    pub identity: DocumentIdentity,
    pub object_key: String,
    pub result: ScanDocumentResult,
}

#[derive(Debug, Clone)]
pub struct FinalizeDocument {
    pub identity: DocumentIdentity,
    pub quarantine_object_key: String,
    pub final_object_key: String,
}

pub struct CompleteAgentEvidenceUpload {
    pub grant: AgentEvidenceUploadGrant,
    pub authority: AgentEvidenceUploadAuthority,
    pub staged_submission_id: EvidenceSubmissionId,
    pub document_id: DocumentId,
    pub document: CreateDocumentPayload,
    pub completed_at: DateTime<Utc>,
}

pub struct CompleteAgentPolicyDocumentUpload {
    pub grant: AgentPolicyDocumentUploadGrant,
    pub authority: AgentPolicyDocumentUploadAuthority,
    pub policy_id: PolicyId,
    pub document_id: DocumentId,
    pub document: CreateDocumentPayload,
    pub completed_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CreatedDocument {
    Created(Document),
    Replayed(Document),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CreatePolicyDocumentOutcome {
    Created(Document),
    Replayed(Document),
    PolicyUnavailable,
    CurrentDocument,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveDocumentOutcome {
    Archived,
    Unavailable,
    NotTerminal,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompleteAgentEvidenceUploadOutcome {
    Created {
        submission_id: EvidenceSubmissionId,
        document: Document,
    },
    Replayed {
        submission_id: EvidenceSubmissionId,
        document_id: DocumentId,
    },
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompleteAgentPolicyDocumentUploadOutcome {
    Created(Document),
    Replayed(DocumentId),
    CurrentDocument,
    Unavailable,
}
impl CreatedDocument {
    pub fn document(&self) -> &Document {
        match self {
            Self::Created(value) | Self::Replayed(value) => value,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentCommandOutcome {
    Applied,
    Ignored,
    Rejected,
}

#[derive(Clone)]
pub struct CreateEvidenceSubmissionDocumentHandler {
    repository: Arc<Postgres>,
}
#[derive(Clone)]
pub struct CreatePolicyDocumentHandler {
    repository: Arc<Postgres>,
}
#[derive(Clone)]
pub struct ArchiveDocumentHandler {
    repository: Arc<Postgres>,
}
#[derive(Clone)]
pub struct ScanDocumentHandler {
    repository: Arc<Postgres>,
}
#[derive(Clone)]
pub struct FinalizeDocumentHandler {
    repository: Arc<Postgres>,
}
#[derive(Clone)]
pub struct CompleteAgentEvidenceUploadHandler {
    repository: Arc<Postgres>,
}
#[derive(Clone)]
pub struct CompleteAgentPolicyDocumentUploadHandler {
    repository: Arc<Postgres>,
}

macro_rules! handlers { ($($handler:ident),+ $(,)?) => { $(impl $handler { pub fn new(repository: Arc<Postgres>) -> Self { Self { repository } } })+ }; }
handlers!(
    CreateEvidenceSubmissionDocumentHandler,
    CreatePolicyDocumentHandler,
    ArchiveDocumentHandler,
    ScanDocumentHandler,
    FinalizeDocumentHandler,
    CompleteAgentEvidenceUploadHandler,
    CompleteAgentPolicyDocumentUploadHandler
);

#[derive(Debug, thiserror::Error)]
pub enum DocumentCommandError {
    #[error("document command is unavailable")]
    Unavailable,
    #[error("document command is invalid")]
    Invalid,
    #[error("document persistence failed")]
    Repository(#[from] RepositoryError),
}

impl CreateEvidenceSubmissionDocumentHandler {
    pub async fn handle(
        &self,
        command: CreateEvidenceSubmissionDocument,
        metadata: ExecutionMetadata,
    ) -> Result<CreatedDocument, DocumentCommandError> {
        if !command
            .connection
            .permissions
            .has(WorkspacePermission::WriteEvidenceSubmissions)
        {
            return Err(DocumentCommandError::Unavailable);
        }
        let identity = DocumentIdentity::Evidence {
            evidence_submission_id: command.submission_id,
            document_id: command.document_id,
        };
        if command.document.owner != identity.owner() {
            return Err(DocumentCommandError::Invalid);
        }
        let outcome = self
            .repository
            .in_agent_connection_workspace_context(
                command.connection.workspace_id,
                command.connection.user_id,
                command.connection.connection_id,
                async move |context| {
                    if context.get_evidence(command.evidence_id).await?.is_none() {
                        return Ok(None);
                    }
                    let submissions = context.evidence_submissions();
                    if let Some(existing) = submissions.get(command.submission_id).await? {
                        if existing.evidence_id != command.evidence_id
                            || existing.coverage() != command.coverage
                            || existing.submitted_by
                                != (EvidenceSubmitter::AgentConnection {
                                    agent_connection_id: command.connection.connection_id,
                                    user_id: command.connection.user_id,
                                })
                        {
                            return Ok(None);
                        }
                        let documents = context.documents();
                        return documents
                            .get(identity)
                            .await
                            .map(|document| document.map(CreatedDocument::Replayed));
                    }
                    let (submission, _) = EvidenceSubmission::create(
                        command.submission_id,
                        command.evidence_id,
                        EvidenceSubmitter::AgentConnection {
                            agent_connection_id: command.connection.connection_id,
                            user_id: command.connection.user_id,
                        },
                        command.coverage,
                        command.received_at,
                    );
                    submissions.save(&submission).await?;
                    let (document, transition) = Document::create(
                        identity,
                        command.connection.user_id,
                        command.document,
                        command.received_at,
                    )
                    .map_err(|_| {
                        RepositoryError::InvariantViolation(
                            "validated document creation is invalid",
                        )
                    })?;
                    let documents = context.documents();
                    documents.save(&document).await?;
                    append_transition_message(context, transition.event, metadata).await?;
                    Ok(Some(CreatedDocument::Created(document)))
                },
            )
            .await?;
        outcome.ok_or(DocumentCommandError::Unavailable)
    }
}

impl CreatePolicyDocumentHandler {
    pub async fn handle(
        &self,
        command: CreatePolicyDocument,
        metadata: ExecutionMetadata,
    ) -> Result<CreatePolicyDocumentOutcome, DocumentCommandError> {
        if !command
            .connection
            .permissions
            .has(WorkspacePermission::WriteEvidence)
        {
            return Err(DocumentCommandError::Unavailable);
        }
        let identity = DocumentIdentity::Policy {
            policy_id: command.policy_id,
            document_id: command.document_id,
        };
        if command.document.owner != identity.owner() {
            return Err(DocumentCommandError::Invalid);
        }
        self.repository
            .in_agent_connection_workspace_context(
                command.connection.workspace_id,
                command.connection.user_id,
                command.connection.connection_id,
                async move |context| {
                    let documents = context.documents();
                    if let Some(document) = documents.get(identity).await? {
                        return Ok(CreatePolicyDocumentOutcome::Replayed(document));
                    }
                    if context.policies().get(command.policy_id).await?.is_none() {
                        return Ok(CreatePolicyDocumentOutcome::PolicyUnavailable);
                    }
                    match context
                        .lock_policy_document_upload_eligibility(command.policy_id)
                        .await?
                    {
                        Some(crate::repository::PolicyDocumentUploadEligibility::Eligible) => {}
                        Some(
                            crate::repository::PolicyDocumentUploadEligibility::CurrentDocument,
                        ) => {
                            return Ok(CreatePolicyDocumentOutcome::CurrentDocument);
                        }
                        None => return Ok(CreatePolicyDocumentOutcome::PolicyUnavailable),
                    }
                    let (document, transition) = Document::create(
                        identity,
                        command.connection.user_id,
                        command.document,
                        command.created_at,
                    )
                    .map_err(|_| {
                        RepositoryError::InvariantViolation(
                            "validated document creation is invalid",
                        )
                    })?;
                    documents.save(&document).await?;
                    append_transition_message(context, transition.event, metadata).await?;
                    Ok(CreatePolicyDocumentOutcome::Created(document))
                },
            )
            .await
            .map_err(Into::into)
    }
}

impl CompleteAgentEvidenceUploadHandler {
    pub async fn handle(
        &self,
        command: CompleteAgentEvidenceUpload,
        metadata: ExecutionMetadata,
    ) -> Result<CompleteAgentEvidenceUploadOutcome, DocumentCommandError> {
        let workspace_id = command.grant.workspace_id();
        let user_id = command.grant.issued_by_user_id();
        let connection_id = command.grant.issued_via_agent_connection_id();
        let upload_id = command.grant.id();
        self.repository
            .in_agent_connection_workspace_context(
                workspace_id,
                user_id,
                connection_id,
                async move |context| {
                    let grants = context.agent_evidence_upload_grants();
                    let Some(mut grant) = grants.get(upload_id, workspace_id).await? else {
                        return Ok(CompleteAgentEvidenceUploadOutcome::Unavailable);
                    };
                    if grant.matches_authority(&command.authority).is_err()
                        || grant
                            .validate_staged_file(
                                command.document.content_length,
                                &command.document.checksum_sha256,
                            )
                            .is_err()
                        || command.staged_submission_id != grant.submission_id()
                        || command.document.owner
                            != DocumentOwner::EvidenceSubmission(grant.submission_id())
                    {
                        return Ok(CompleteAgentEvidenceUploadOutcome::Unavailable);
                    }
                    match grant.completed_document_at(command.completed_at) {
                        Ok(Some(document_id)) => {
                            return Ok(CompleteAgentEvidenceUploadOutcome::Replayed {
                                submission_id: grant.submission_id(),
                                document_id,
                            });
                        }
                        Ok(None) => {}
                        Err(_) => return Ok(CompleteAgentEvidenceUploadOutcome::Unavailable),
                    }
                    if context.get_evidence(grant.evidence_id()).await?.is_none() {
                        return Ok(CompleteAgentEvidenceUploadOutcome::Unavailable);
                    }
                    let submissions = context.evidence_submissions();
                    if submissions.get(grant.submission_id()).await?.is_some() {
                        return Ok(CompleteAgentEvidenceUploadOutcome::Unavailable);
                    }
                    let (submission, _) = EvidenceSubmission::create(
                        grant.submission_id(),
                        grant.evidence_id(),
                        EvidenceSubmitter::AgentConnection {
                            agent_connection_id: connection_id,
                            user_id,
                        },
                        grant.coverage(),
                        command.completed_at,
                    );
                    submissions.save(&submission).await?;
                    let identity = DocumentIdentity::Evidence {
                        evidence_submission_id: grant.submission_id(),
                        document_id: command.document_id,
                    };
                    let (document, transition) =
                        Document::create(identity, user_id, command.document, command.completed_at)
                            .map_err(|_| {
                                RepositoryError::InvariantViolation(
                                    "validated machine evidence document is invalid",
                                )
                            })?;
                    context.documents().save(&document).await?;
                    append_transition_message(context, transition.event, metadata).await?;
                    grant
                        .complete(document.id(), command.completed_at)
                        .map_err(|_| {
                            RepositoryError::InvariantViolation(
                                "machine upload grant changed during completion",
                            )
                        })?;
                    grants.save(&grant).await?;
                    Ok(CompleteAgentEvidenceUploadOutcome::Created {
                        submission_id: grant.submission_id(),
                        document,
                    })
                },
            )
            .await
            .map_err(Into::into)
    }
}

impl CompleteAgentPolicyDocumentUploadHandler {
    pub async fn handle(
        &self,
        command: CompleteAgentPolicyDocumentUpload,
        metadata: ExecutionMetadata,
    ) -> Result<CompleteAgentPolicyDocumentUploadOutcome, DocumentCommandError> {
        let workspace_id = command.grant.workspace_id();
        let user_id = command.grant.issued_by_user_id();
        let connection_id = command.grant.issued_via_agent_connection_id();
        let upload_id = command.grant.id();
        self.repository
            .in_agent_connection_workspace_context(
                workspace_id,
                user_id,
                connection_id,
                async move |context| {
                    let grants = context.agent_policy_document_upload_grants();
                    let Some(mut grant) = grants.get(upload_id, workspace_id).await? else {
                        return Ok(CompleteAgentPolicyDocumentUploadOutcome::Unavailable);
                    };
                    if grant.matches_authority(&command.authority).is_err()
                        || grant
                            .validate_staged_file(
                                command.document.content_length,
                                &command.document.checksum_sha256,
                            )
                            .is_err()
                        || command.policy_id != grant.policy_id()
                        || command.document.owner != DocumentOwner::Policy(grant.policy_id())
                    {
                        return Ok(CompleteAgentPolicyDocumentUploadOutcome::Unavailable);
                    }
                    match grant.completed_document_at(command.completed_at) {
                        Ok(Some(document_id)) => {
                            return Ok(CompleteAgentPolicyDocumentUploadOutcome::Replayed(
                                document_id,
                            ));
                        }
                        Ok(None) => {}
                        Err(_) => return Ok(CompleteAgentPolicyDocumentUploadOutcome::Unavailable),
                    }
                    match context
                        .lock_policy_document_upload_eligibility(grant.policy_id())
                        .await?
                    {
                        Some(PolicyDocumentUploadEligibility::Eligible) => {}
                        Some(PolicyDocumentUploadEligibility::CurrentDocument) => {
                            return Ok(CompleteAgentPolicyDocumentUploadOutcome::CurrentDocument);
                        }
                        None => return Ok(CompleteAgentPolicyDocumentUploadOutcome::Unavailable),
                    }
                    let identity = DocumentIdentity::Policy {
                        policy_id: grant.policy_id(),
                        document_id: command.document_id,
                    };
                    let (document, transition) =
                        Document::create(identity, user_id, command.document, command.completed_at)
                            .map_err(|_| {
                                RepositoryError::InvariantViolation(
                                    "validated machine policy document is invalid",
                                )
                            })?;
                    context.documents().save(&document).await?;
                    append_transition_message(context, transition.event, metadata).await?;
                    grant
                        .complete(document.id(), command.completed_at)
                        .map_err(|_| {
                            RepositoryError::InvariantViolation(
                                "policy machine upload grant changed during completion",
                            )
                        })?;
                    grants.save(&grant).await?;
                    Ok(CompleteAgentPolicyDocumentUploadOutcome::Created(document))
                },
            )
            .await
            .map_err(Into::into)
    }
}

impl ArchiveDocumentHandler {
    pub async fn handle(
        &self,
        command: ArchiveDocument,
        _: ExecutionMetadata,
    ) -> Result<ArchiveDocumentOutcome, DocumentCommandError> {
        let permission = match command.identity.owner() {
            DocumentOwner::EvidenceSubmission(_) => WorkspacePermission::WriteEvidenceSubmissions,
            DocumentOwner::Policy(_) => WorkspacePermission::WriteEvidence,
        };
        if !command.connection.permissions.has(permission) {
            return Err(DocumentCommandError::Unavailable);
        }
        let identity = command.identity;
        self.repository
            .in_agent_connection_workspace_context(
                command.connection.workspace_id,
                command.connection.user_id,
                command.connection.connection_id,
                async move |context| {
                    let documents = context.documents();
                    let Some(mut document) = documents.get(identity).await? else {
                        return Ok(ArchiveDocumentOutcome::Unavailable);
                    };
                    let transition = document.archive();
                    if transition.changed() {
                        documents.save(&document).await?;
                    }
                    Ok(match transition.outcome {
                        DocumentTransitionOutcome::Archived
                        | DocumentTransitionOutcome::Ignored => ArchiveDocumentOutcome::Archived,
                        DocumentTransitionOutcome::Rejected => ArchiveDocumentOutcome::NotTerminal,
                        _ => {
                            return Err(RepositoryError::InvariantViolation(
                                "archive transition returned an unrelated outcome",
                            ));
                        }
                    })
                },
            )
            .await
            .map_err(Into::into)
    }
}

impl ScanDocumentHandler {
    pub async fn handle(
        &self,
        command: ScanDocument,
        metadata: ExecutionMetadata,
    ) -> Result<DocumentCommandOutcome, DocumentCommandError> {
        self.repository
            .in_transaction(async move |context| {
                let documents = context.documents();
                let Some(mut document) = documents.get(command.identity).await? else {
                    return Ok(DocumentCommandOutcome::Ignored);
                };
                if document.object_key != command.object_key {
                    return Ok(DocumentCommandOutcome::Ignored);
                }
                let transition = match command.result {
                    ScanDocumentResult::Clean => document.scan_clean(),
                    ScanDocumentResult::Malicious => document.scan_malicious(),
                    ScanDocumentResult::Failed => document.scan_failed(),
                };
                if transition.changed() {
                    documents.save(&document).await?;
                    append_transaction_message(context, transition.event, metadata).await?;
                }
                Ok(outcome(transition.outcome))
            })
            .await
            .map_err(Into::into)
    }
}

impl FinalizeDocumentHandler {
    pub async fn handle(
        &self,
        command: FinalizeDocument,
        _: ExecutionMetadata,
    ) -> Result<DocumentCommandOutcome, DocumentCommandError> {
        self.repository
            .in_transaction(async move |context| {
                let documents = context.documents();
                let Some(mut document) = documents.get(command.identity).await? else {
                    return Ok(DocumentCommandOutcome::Ignored);
                };
                if document.object_key != command.quarantine_object_key {
                    return Ok(DocumentCommandOutcome::Ignored);
                }
                let transition = document.finalize_uploaded(command.final_object_key);
                if transition.changed() {
                    documents.save(&document).await?;
                }
                Ok(outcome(transition.outcome))
            })
            .await
            .map_err(Into::into)
    }
}

fn outcome(value: DocumentTransitionOutcome) -> DocumentCommandOutcome {
    match value {
        DocumentTransitionOutcome::Ignored => DocumentCommandOutcome::Ignored,
        DocumentTransitionOutcome::Rejected => DocumentCommandOutcome::Rejected,
        _ => DocumentCommandOutcome::Applied,
    }
}

async fn append_transition_message(
    context: &crate::repository::WorkspaceTransactionContext<'_>,
    event: Option<DocumentEvent>,
    metadata: ExecutionMetadata,
) -> Result<(), RepositoryError> {
    if let Some(message) = message_for(event, metadata) {
        context.append_outbox_message(&message).await?;
    }
    Ok(())
}
async fn append_transaction_message(
    context: &crate::repository::TransactionContext<'_>,
    event: Option<DocumentEvent>,
    metadata: ExecutionMetadata,
) -> Result<(), RepositoryError> {
    if let Some(message) = message_for(event, metadata) {
        context.append_outbox_message(&message).await?;
    }
    Ok(())
}
fn message_for(
    event: Option<DocumentEvent>,
    metadata: ExecutionMetadata,
) -> Option<NewOutboxMessage> {
    let (identity, key, finalization) = match event? {
        DocumentEvent::ScanRequested {
            identity,
            object_key,
        } => (identity, object_key, false),
        DocumentEvent::FinalizationRequested {
            identity,
            object_key,
        } => (identity, object_key, true),
    };
    let message = if finalization {
        IntegrationMessage::finalize_document(
            identity,
            key,
            metadata.correlation_id().or(metadata.request_id()),
            metadata.causation_id(),
        )
    } else {
        IntegrationMessage::scan_document(
            identity,
            key,
            metadata.correlation_id().or(metadata.request_id()),
            metadata.causation_id(),
        )
    };
    Some(NewOutboxMessage::new(
        TopicName::new(MESSAGE_BUS_TOPIC),
        message,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{domain::WorkspacePermissions, repository::test_support};
    #[test]
    fn ignored_and_rejected_transitions_have_no_side_effect_outcome() {
        assert_eq!(
            outcome(DocumentTransitionOutcome::Ignored),
            DocumentCommandOutcome::Ignored
        );
        assert_eq!(
            outcome(DocumentTransitionOutcome::Rejected),
            DocumentCommandOutcome::Rejected
        );
    }

    #[tokio::test]
    async fn evidence_submission_and_document_snapshot_are_created_atomically() {
        let postgres = Arc::new(test_support::database().await);
        let workspace = test_support::workspace(&postgres, "Evidence owner").await;
        let evidence_id =
            test_support::evidence(&postgres, workspace.workspace_id, "Bank statement").await;
        let submission_id = EvidenceSubmissionId::from(uuid::Uuid::new_v4());
        let document_id = DocumentId::from(uuid::Uuid::new_v4());
        let identity = DocumentIdentity::Evidence {
            evidence_submission_id: submission_id,
            document_id,
        };
        let outcome = CreateEvidenceSubmissionDocumentHandler::new(postgres)
            .handle(
                CreateEvidenceSubmissionDocument {
                    connection: AgentConnectionContext {
                        user_id: workspace.user_id,
                        connection_id: workspace.agent_connection_id,
                        workspace_id: workspace.workspace_id,
                        permissions: WorkspacePermissions::all(),
                    },
                    submission_id,
                    evidence_id,
                    coverage: CoverageWindow::new(
                        Utc::now(),
                        Utc::now() + chrono::Duration::days(1),
                    )
                    .expect("test coverage is valid"),
                    document_id,
                    document: CreateDocumentPayload {
                        owner: identity.owner(),
                        filename: "statement.pdf".to_owned(),
                        content_type: "application/pdf".to_owned(),
                        content_length: 1,
                        object_key: format!("quarantine/{}", uuid::Uuid::new_v4()),
                        checksum_sha256: "sha".to_owned(),
                        checksum_crc32c: "crc".to_owned(),
                    },
                    received_at: Utc::now(),
                },
                ExecutionMetadata::background(),
            )
            .await
            .expect("evidence document command succeeds");
        assert!(matches!(outcome, CreatedDocument::Created(_)));
    }
}
