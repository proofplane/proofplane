use std::sync::Arc;

use crate::{
    domain::{
        CoverageWindow, CreateEvidenceSubmissionPayload, EvidenceId, EvidenceSubmission,
        EvidenceSubmissionId,
    },
    object_storage::{
        FilesystemObjectStore, ObjectKey, ObjectStore, PutObjectRequest, StorageError,
    },
    pubsub::{TopicName, MESSAGE_BUS_TOPIC},
    repository::{ArchiveSubmissionResult, NewOutboxMessage, Postgres},
    services::Error,
    worker::SUBMISSION_SCAN_REQUESTED,
};

use super::agent_connections::AgentConnectionContext;
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
    ) -> Result<Option<EvidenceSubmission>, Error> {
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
    ) -> Result<Option<Vec<EvidenceSubmission>>, Error> {
        Ok(self
            .repository
            .in_workspace_context_read(connection.workspace_id, async move |context| {
                if !context.evidence_exists(evidence_id).await? {
                    return Ok(None);
                }

                context
                    .list_evidence_submissions(evidence_id)
                    .await
                    .map(Some)
            })
            .await?)
    }

    pub async fn list_for_coverage(
        &self,
        connection: &AgentConnectionContext,
        evidence_id: EvidenceId,
        coverage: CoverageWindow,
    ) -> Result<Vec<EvidenceSubmission>, Error> {
        Ok(self
            .repository
            .in_workspace_context_read(connection.workspace_id, async move |context| {
                context
                    .list_evidence_submissions_for_coverage(evidence_id, coverage)
                    .await
            })
            .await?)
    }

    pub async fn evidence_exists(
        &self,
        connection: &AgentConnectionContext,
        evidence_id: EvidenceId,
    ) -> Result<bool, Error> {
        Ok(self
            .repository
            .in_workspace_context_read(connection.workspace_id, async move |context| {
                context.evidence_exists(evidence_id).await
            })
            .await?)
    }

    /// Streams the uploaded bytes into quarantine. The submission row does not
    /// exist yet, so the key is namespaced by a fresh upload id.
    pub async fn upload_file<S>(
        &self,
        connection: &AgentConnectionContext,
        evidence_id: EvidenceId,
        filename: String,
        content_type: String,
        chunks: S,
    ) -> Result<CreateEvidenceSubmissionPayload, Error>
    where
        S: Stream<Item = Result<Bytes, StorageError>> + Send,
    {
        let upload_id = Uuid::new_v4();
        let stable_prefix = format!("quarantine/evidence/{evidence_id}/submissions/{upload_id}");
        let key = ObjectKey::new(connection.workspace_id, stable_prefix, &filename)?;

        let metadata = self
            .object_store
            .put_object(PutObjectRequest {
                key,
                content_type,
                chunks,
            })
            .await?;

        let content_length = i64::try_from(metadata.content_length).map_err(|_| {
            Error::Storage(crate::object_storage::StorageError::StreamRead {
                message: "file is too large".to_owned(),
                payload_too_large: true,
            })
        })?;

        Ok(CreateEvidenceSubmissionPayload {
            filename,
            content_type: metadata.content_type,
            content_length,
            object_key: metadata.key.to_string(),
            checksum_sha256: metadata.sha256,
            // The object store computes only sha256; the caller streaming the
            // body fills this in from its own running crc32c.
            checksum_crc32c: String::new(),
        })
    }

    pub async fn delete_uploaded_object(&self, object_key: &str) -> Result<(), Error> {
        self.object_store
            .delete_object(&ObjectKey::parse(object_key.to_owned())?)
            .await?;
        Ok(())
    }

    /// Records one uploaded file as a submission and enqueues its scan in the
    /// same transaction. If either fails the quarantined object is removed, so
    /// no object outlives the row that owns it.
    pub async fn create_submission(
        &self,
        connection: &AgentConnectionContext,
        request_id: Uuid,
        evidence_id: EvidenceId,
        coverage: CoverageWindow,
        payload: CreateEvidenceSubmissionPayload,
    ) -> Result<Option<EvidenceSubmission>, Error> {
        let object_key = payload.object_key.clone();
        let result = self
            .repository
            .in_agent_connection_workspace_context(
                connection.workspace_id,
                connection.user_id,
                connection.connection_id,
                async move |context| {
                    let Some(submission) = context
                        .create_evidence_submission(evidence_id, coverage, &payload)
                        .await?
                    else {
                        return Ok(None);
                    };
                    context
                        .append_outbox_message(&submission_scan_requested_message(
                            &submission,
                            request_id,
                        ))
                        .await?;

                    Ok(Some(submission))
                },
            )
            .await;

        match result {
            Ok(Some(submission)) => Ok(Some(submission)),
            Ok(None) => {
                let _ = self.delete_uploaded_object(&object_key).await;
                Ok(None)
            }
            Err(error) => {
                let _ = self.delete_uploaded_object(&object_key).await;
                Err(error.into())
            }
        }
    }

    pub async fn archive_submission(
        &self,
        connection: &AgentConnectionContext,
        submission_id: EvidenceSubmissionId,
    ) -> Result<ArchiveSubmissionResult, Error> {
        Ok(self
            .repository
            .in_agent_connection_workspace_context(
                connection.workspace_id,
                connection.user_id,
                connection.connection_id,
                async move |context| context.archive_evidence_submission(submission_id).await,
            )
            .await?)
    }
}

fn submission_scan_requested_message(
    submission: &EvidenceSubmission,
    request_id: Uuid,
) -> NewOutboxMessage {
    NewOutboxMessage {
        topic: TopicName::new(MESSAGE_BUS_TOPIC),
        event_type: SUBMISSION_SCAN_REQUESTED.to_owned(),
        aggregate_type: "evidence_submission".to_owned(),
        aggregate_id: Uuid::from(submission.id).to_string(),
        payload: serde_json::json!({
            "evidence_id": Uuid::from(submission.evidence_id).to_string(),
            "object_key": submission.object_key,
        }),
        request_id: Some(request_id),
    }
}
