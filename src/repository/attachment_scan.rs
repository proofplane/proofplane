#[async_trait]
pub trait AttachmentScanRepository: Send + Sync {
    async fn load_pending_attachment_upload_work(
        &self,
        evidence_attachment_id: EvidenceAttachmentId,
        quarantine_object_key: &str,
    ) -> Result<Option<PendingAttachmentUploadWork>, Error>;

    async fn mark_attachment_uploaded(
        &self,
        evidence_attachment_id: EvidenceAttachmentId,
        quarantine_object_key: &str,
        final_object_key: &str,
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
