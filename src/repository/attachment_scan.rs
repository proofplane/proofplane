use std::{future::Future, pin::Pin};

use async_trait::async_trait;

use crate::{
    domain::{EvidenceAttachmentId, EvidenceSubmissionId},
    repository::{
        Error, FinalizingAttachmentUploadWork, NewOutboxMessage, OutboxMessage,
        PendingAttachmentUploadWork, Postgres, TransactionContext,
    },
};

#[async_trait]
pub trait AttachmentScanRepository: Send + Sync {
    type Transaction<'a>: AttachmentScanTransaction + Send
    where
        Self: 'a;

    async fn in_transaction<T, F>(&self, operation: F) -> Result<T, Error>
    where
        T: Send,
        F: for<'context, 'transaction> FnOnce(
                &'context mut Self::Transaction<'transaction>,
            ) -> Pin<
                Box<dyn Future<Output = Result<T, Error>> + Send + 'context>,
            > + Send;

    async fn load_pending_attachment_upload_work(
        &self,
        evidence_attachment_id: EvidenceAttachmentId,
        quarantine_object_key: &str,
    ) -> Result<Option<PendingAttachmentUploadWork>, Error>;

    async fn mark_attachment_contains_virus(
        &self,
        evidence_attachment_id: EvidenceAttachmentId,
        quarantine_object_key: &str,
        reason: String,
    ) -> Result<bool, Error>;

    async fn mark_attachment_upload_failed(
        &self,
        evidence_attachment_id: EvidenceAttachmentId,
        quarantine_object_key: &str,
        reason: String,
    ) -> Result<bool, Error>;
}

#[async_trait]
pub trait AttachmentScanTransaction {
    async fn request_attachment_finalization(
        &mut self,
        work: &PendingAttachmentUploadWork,
    ) -> Result<bool, Error>;

    async fn append_outbox_message(
        &mut self,
        message: &NewOutboxMessage,
    ) -> Result<OutboxMessage, Error>;
}

#[async_trait]
impl AttachmentScanTransaction for TransactionContext<'_> {
    async fn request_attachment_finalization(
        &mut self,
        work: &PendingAttachmentUploadWork,
    ) -> Result<bool, Error> {
        TransactionContext::request_attachment_finalization(self, work).await
    }

    async fn append_outbox_message(
        &mut self,
        message: &NewOutboxMessage,
    ) -> Result<OutboxMessage, Error> {
        TransactionContext::append_outbox_message(self, message).await
    }
}

#[async_trait]
impl AttachmentScanRepository for Postgres {
    type Transaction<'a> = TransactionContext<'a>;

    async fn in_transaction<T, F>(&self, operation: F) -> Result<T, Error>
    where
        T: Send,
        F: for<'context, 'transaction> FnOnce(
                &'context mut Self::Transaction<'transaction>,
            ) -> Pin<
                Box<dyn Future<Output = Result<T, Error>> + Send + 'context>,
            > + Send,
    {
        Postgres::in_transaction(self, operation).await
    }

    async fn load_pending_attachment_upload_work(
        &self,
        evidence_attachment_id: EvidenceAttachmentId,
        quarantine_object_key: &str,
    ) -> Result<Option<PendingAttachmentUploadWork>, Error> {
        Postgres::load_pending_attachment_upload_work(
            self,
            evidence_attachment_id,
            quarantine_object_key,
        )
        .await
    }

    async fn mark_attachment_contains_virus(
        &self,
        evidence_attachment_id: EvidenceAttachmentId,
        quarantine_object_key: &str,
        reason: String,
    ) -> Result<bool, Error> {
        Postgres::mark_attachment_contains_virus(
            self,
            evidence_attachment_id,
            quarantine_object_key,
            &reason,
        )
        .await
    }

    async fn mark_attachment_upload_failed(
        &self,
        evidence_attachment_id: EvidenceAttachmentId,
        quarantine_object_key: &str,
        reason: String,
    ) -> Result<bool, Error> {
        Postgres::mark_attachment_upload_failed(
            self,
            evidence_attachment_id,
            quarantine_object_key,
            &reason,
        )
        .await
    }
}

#[async_trait]
pub trait AttachmentFinalizationRepository: Send + Sync {
    async fn load_finalizing_attachment_upload_work(
        &self,
        evidence_attachment_id: EvidenceAttachmentId,
        evidence_submission_id: EvidenceSubmissionId,
        quarantine_object_key: &str,
    ) -> Result<Option<FinalizingAttachmentUploadWork>, Error>;

    async fn mark_attachment_uploaded(
        &self,
        evidence_attachment_id: EvidenceAttachmentId,
        quarantine_object_key: &str,
        final_object_key: &str,
    ) -> Result<bool, Error>;
}

#[async_trait]
impl AttachmentFinalizationRepository for Postgres {
    async fn load_finalizing_attachment_upload_work(
        &self,
        evidence_attachment_id: EvidenceAttachmentId,
        evidence_submission_id: EvidenceSubmissionId,
        quarantine_object_key: &str,
    ) -> Result<Option<FinalizingAttachmentUploadWork>, Error> {
        Postgres::load_finalizing_attachment_upload_work(
            self,
            evidence_attachment_id,
            evidence_submission_id,
            quarantine_object_key,
        )
        .await
    }

    async fn mark_attachment_uploaded(
        &self,
        evidence_attachment_id: EvidenceAttachmentId,
        quarantine_object_key: &str,
        final_object_key: &str,
    ) -> Result<bool, Error> {
        Postgres::mark_attachment_uploaded(
            self,
            evidence_attachment_id,
            quarantine_object_key,
            final_object_key,
        )
        .await
    }
}
