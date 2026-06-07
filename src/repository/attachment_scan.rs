use async_trait::async_trait;
use uuid::Uuid;

use crate::{
    domain::{EvidenceAttachmentId, EvidenceSubmissionId},
    repository::{Error, FinalizingAttachmentUploadWork, PendingAttachmentUploadWork, Postgres},
};

#[async_trait]
pub trait AttachmentScanRepository: Send + Sync {
    async fn load_pending_attachment_upload_work(
        &self,
        evidence_attachment_id: EvidenceAttachmentId,
        quarantine_object_key: &str,
    ) -> Result<Option<PendingAttachmentUploadWork>, Error>;

    async fn request_attachment_finalization(
        &self,
        work: &PendingAttachmentUploadWork,
        request_id: Option<Uuid>,
    ) -> Result<bool, Error>;

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
impl AttachmentScanRepository for Postgres {
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

    async fn request_attachment_finalization(
        &self,
        work: &PendingAttachmentUploadWork,
        request_id: Option<Uuid>,
    ) -> Result<bool, Error> {
        Postgres::request_attachment_finalization(self, work, request_id).await
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
