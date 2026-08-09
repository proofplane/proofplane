use std::sync::Arc;

use crate::{
    application::ExecutionMetadata,
    authentication::AgentConnectionContext,
    domain::{
        CoverageWindow, CreateDocumentPayload, Document, DocumentEvent, DocumentId,
        DocumentIdentity, DocumentOwner, DocumentTransitionOutcome, EvidenceId, EvidenceSubmission,
        EvidenceSubmissionId, EvidenceSubmitter, PolicyId, WorkspacePermission,
    },
    messaging::IntegrationMessage,
    pubsub::{TopicName, MESSAGE_BUS_TOPIC},
    repository::{Error as RepositoryError, NewOutboxMessage, Postgres},
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CreatedDocument {
    Created(Document),
    Replayed(Document),
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

macro_rules! handlers { ($($handler:ident),+ $(,)?) => { $(impl $handler { pub fn new(repository: Arc<Postgres>) -> Self { Self { repository } } })+ }; }
handlers!(
    CreateEvidenceSubmissionDocumentHandler,
    CreatePolicyDocumentHandler,
    ArchiveDocumentHandler,
    ScanDocumentHandler,
    FinalizeDocumentHandler
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
                    if submissions.get(command.submission_id).await?.is_some() {
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
    ) -> Result<CreatedDocument, DocumentCommandError> {
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
        let outcome = self
            .repository
            .in_agent_connection_workspace_context(
                command.connection.workspace_id,
                command.connection.user_id,
                command.connection.connection_id,
                async move |context| {
                    let documents = context.documents();
                    if let Some(document) = documents.get(identity).await? {
                        return Ok(Some(CreatedDocument::Replayed(document)));
                    }
                    if context.policies().get(command.policy_id).await?.is_none() {
                        return Ok(None);
                    }
                    if !matches!(
                        context
                            .lock_policy_document_upload_eligibility(command.policy_id)
                            .await?,
                        Some(crate::repository::PolicyDocumentUploadEligibility::Eligible)
                    ) {
                        return Ok(None);
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
                    Ok(Some(CreatedDocument::Created(document)))
                },
            )
            .await?;
        outcome.ok_or(DocumentCommandError::Unavailable)
    }
}

impl ArchiveDocumentHandler {
    pub async fn handle(
        &self,
        command: ArchiveDocument,
        _: ExecutionMetadata,
    ) -> Result<DocumentCommandOutcome, DocumentCommandError> {
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
                        return Ok(DocumentCommandOutcome::Rejected);
                    };
                    let transition = document.archive();
                    if transition.changed() {
                        documents.save(&document).await?;
                    }
                    Ok(outcome(transition.outcome))
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
}
