use std::sync::Arc;

use bytes::Bytes;
use futures_core::Stream;
use uuid::Uuid;

use crate::{
    domain::{
        AgentPolicyDocumentUploadGrant, CreateDocumentPayload, Document, DocumentId, DocumentOwner,
        PolicyId, WorkspaceId,
    },
    messaging::IntegrationMessage,
    object_storage::{FilesystemObjectStore, StorageError},
    observability::audit::{AuditActor, AuditClientType, AuditEvent, AuditObject, AuditOutcome},
    pubsub::{TopicName, MESSAGE_BUS_TOPIC},
    repository::{ArchiveDocumentResult, CreatePolicyDocumentResult, NewOutboxMessage, Postgres},
};

use super::{
    agent_connections::AgentConnectionContext,
    documents::{
        delete_staged_document, stage_bounded_document, stage_document, BoundedStagingRequest,
    },
    Error,
};

#[derive(Clone)]
pub struct PolicyDocumentService {
    repository: Arc<Postgres>,
    object_store: Arc<FilesystemObjectStore>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UploadPolicyDocumentPayload {
    pub policy_id: PolicyId,
    pub filename: String,
    pub content_type: String,
    pub content_length: i64,
    pub object_key: String,
    pub checksum_sha256: String,
    pub checksum_crc32c: String,
}

impl PolicyDocumentService {
    pub fn new(repository: Arc<Postgres>, object_store: Arc<FilesystemObjectStore>) -> Self {
        Self {
            repository,
            object_store,
        }
    }

    pub async fn upload<S>(
        &self,
        connection: &AgentConnectionContext,
        policy_id: PolicyId,
        filename: String,
        content_type: String,
        chunks: S,
    ) -> Result<UploadPolicyDocumentPayload, Error>
    where
        S: Stream<Item = Result<Bytes, StorageError>> + Send,
    {
        let staged = stage_document(
            &self.object_store,
            connection.workspace_id,
            DocumentOwner::Policy(policy_id),
            Uuid::new_v4(),
            filename,
            content_type,
            chunks,
        )
        .await?;

        Ok(UploadPolicyDocumentPayload {
            policy_id,
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
        grant: &AgentPolicyDocumentUploadGrant,
        max_bytes: usize,
        chunks: S,
    ) -> Result<UploadPolicyDocumentPayload, Error>
    where
        S: Stream<Item = Result<Bytes, StorageError>> + Send,
    {
        let staged = stage_bounded_document(
            &self.object_store,
            BoundedStagingRequest {
                workspace_id: grant.workspace_id(),
                owner: DocumentOwner::Policy(grant.policy_id()),
                upload_id: Uuid::new_v4(),
                filename: grant.declaration().filename().to_owned(),
                content_type: grant.declaration().content_type().to_owned(),
                max_bytes,
                cleanup_operation: "agent_policy_document_length_mismatch",
            },
            chunks,
        )
        .await?;

        Ok(UploadPolicyDocumentPayload {
            policy_id: grant.policy_id(),
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
        policy_id: PolicyId,
        document_id: DocumentId,
    ) -> Result<Option<Document>, Error> {
        Ok(self
            .repository
            .in_workspace_context_read(workspace_id, async move |context| {
                context
                    .get_policy_document_for_agent_upload(policy_id, document_id)
                    .await
            })
            .await?)
    }

    pub async fn create(
        &self,
        connection: &AgentConnectionContext,
        request_id: Uuid,
        policy_id: PolicyId,
        mut payload: UploadPolicyDocumentPayload,
    ) -> Result<CreatePolicyDocumentResult, Error> {
        payload.policy_id = policy_id;
        let create_payload = CreateDocumentPayload {
            owner: DocumentOwner::Policy(policy_id),
            filename: payload.filename,
            content_type: payload.content_type,
            content_length: payload.content_length,
            object_key: payload.object_key.clone(),
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
                    let result = context.create_policy_document(&create_payload).await?;
                    if let CreatePolicyDocumentResult::Created(document) = &result {
                        context
                            .append_outbox_message(&policy_document_scan_requested_message(
                                document, request_id,
                            ))
                            .await?;
                    }
                    Ok(result)
                },
            )
            .await;

        match result {
            Ok(CreatePolicyDocumentResult::Created(document)) => {
                emit_document_audit(
                    "policy_document.accepted",
                    "accept_policy_document",
                    connection,
                    request_id,
                    policy_id,
                    &document,
                );
                Ok(CreatePolicyDocumentResult::Created(document))
            }
            Ok(result) => {
                self.delete_staged_object(&payload.object_key).await?;
                Ok(result)
            }
            Err(error) => {
                self.delete_staged_object(&payload.object_key).await?;
                Err(error.into())
            }
        }
    }

    pub async fn get_current(
        &self,
        connection: &AgentConnectionContext,
        policy_id: PolicyId,
    ) -> Result<Option<Document>, Error> {
        Ok(self
            .repository
            .in_workspace_context_read(connection.workspace_id, async move |context| {
                context.get_current_policy_document(policy_id).await
            })
            .await?)
    }

    pub async fn archive(
        &self,
        connection: &AgentConnectionContext,
        request_id: Uuid,
        policy_id: PolicyId,
        document_id: DocumentId,
    ) -> Result<ArchiveDocumentResult, Error> {
        let result = self
            .repository
            .in_agent_connection_workspace_context(
                connection.workspace_id,
                connection.user_id,
                connection.connection_id,
                async move |context| {
                    context
                        .archive_policy_document(policy_id, document_id)
                        .await
                },
            )
            .await?;
        if result == ArchiveDocumentResult::Archived {
            AuditEvent::new(
                "policy_document.archived",
                AuditOutcome::Success,
                connection_actor(connection),
                AuditClientType::Rest,
                "archive_policy_document",
            )
            .workspace_id(connection.workspace_id.into())
            .request_id(request_id)
            .metadata("policy_id", Uuid::from(policy_id))
            .metadata("policy_document_id", Uuid::from(document_id))
            .object(AuditObject::new("policy_document", document_id.into()))
            .emit();
        }
        Ok(result)
    }

    pub async fn delete_staged_object(&self, object_key: &str) -> Result<(), Error> {
        delete_staged_document(&self.object_store, object_key).await
    }
}

pub(crate) fn policy_document_scan_requested_message(
    document: &Document,
    request_id: Uuid,
) -> NewOutboxMessage {
    NewOutboxMessage::new(
        TopicName::new(MESSAGE_BUS_TOPIC),
        IntegrationMessage::scan_document(
            document.identity,
            document.object_key.clone(),
            Some(request_id),
            None,
        ),
    )
}

fn emit_document_audit(
    event_name: &'static str,
    operation: &'static str,
    connection: &AgentConnectionContext,
    request_id: Uuid,
    policy_id: PolicyId,
    document: &Document,
) {
    AuditEvent::new(
        event_name,
        AuditOutcome::Success,
        connection_actor(connection),
        AuditClientType::Rest,
        operation,
    )
    .workspace_id(connection.workspace_id.into())
    .request_id(request_id)
    .metadata("policy_id", Uuid::from(policy_id))
    .metadata("policy_document_id", Uuid::from(document.id()))
    .metadata("lifecycle_status", document.upload_status.as_str())
    .object(AuditObject::new("policy_document", document.id().into()))
    .emit();
}

fn connection_actor(connection: &AgentConnectionContext) -> AuditActor {
    AuditActor::AgentConnection {
        user_id: connection.user_id.into(),
        agent_connection_id: connection.connection_id.into(),
    }
}
