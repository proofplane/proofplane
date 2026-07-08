use deadpool_postgres::GenericClient;
use tokio_postgres::Row;
use uuid::Uuid;

use crate::{
    domain::{
        AttachmentUploadStatus, CreateEvidenceAttachmentPayload, CreateEvidenceSubmissionPayload,
        EvidenceAttachment, EvidenceAttachmentId, EvidenceRequestId, EvidenceSubmission,
        EvidenceSubmissionDetail, EvidenceSubmissionId, EvidenceSubmitter, WorkspaceId,
    },
    repository::{WorkspaceReadContext, WorkspaceTransactionContext},
};

use super::{Error, Postgres, TransactionContext};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingAttachmentUploadWork {
    pub workspace_id: WorkspaceId,
    pub evidence_submission_id: EvidenceSubmissionId,
    pub evidence_attachment_id: EvidenceAttachmentId,
    pub filename: String,
    pub content_type: String,
    pub content_length: i64,
    pub object_key: String,
    pub checksum_sha256: String,
    pub upload_status: AttachmentUploadStatus,
}

pub type FinalizingAttachmentUploadWork = PendingAttachmentUploadWork;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttachmentDownloadCandidate {
    pub workspace_id: WorkspaceId,
    pub attachment: EvidenceAttachment,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveAttachmentResult {
    Archived,
    NotFound,
    NotTerminal,
}

impl Postgres {
    pub async fn load_pending_attachment_upload_work(
        &self,
        evidence_attachment_id: EvidenceAttachmentId,
        quarantine_object_key: &str,
    ) -> Result<Option<PendingAttachmentUploadWork>, Error> {
        let client = self.get().await?;
        let rows = client
            .query(
                r#"
SELECT
    er.workspace_id,
    a.evidence_submission_id,
    a.id AS attachment_id,
    a.filename,
    a.content_type,
    a.content_length,
    a.object_key,
    a.checksum_sha256,
    a.upload_status
FROM evidence_attachments a
JOIN evidence_submissions s ON s.id = a.evidence_submission_id
JOIN evidence_requests er ON er.id = s.evidence_request_id
WHERE a.id = $1
  AND a.object_key = $2
  AND a.upload_status = 'pending'
"#,
                &[&Uuid::from(evidence_attachment_id), &quarantine_object_key],
            )
            .await?;

        rows.into_iter()
            .next()
            .map(|row| pending_attachment_upload_work_from_row(&row))
            .transpose()
    }

    pub async fn load_finalizing_attachment_upload_work(
        &self,
        evidence_attachment_id: EvidenceAttachmentId,
        evidence_submission_id: EvidenceSubmissionId,
        quarantine_object_key: &str,
    ) -> Result<Option<FinalizingAttachmentUploadWork>, Error> {
        let client = self.get().await?;
        let rows = client
            .query(
                r#"
SELECT
    er.workspace_id,
    a.evidence_submission_id,
    a.id AS attachment_id,
    a.filename,
    a.content_type,
    a.content_length,
    a.object_key,
    a.checksum_sha256,
    a.upload_status
FROM evidence_attachments a
JOIN evidence_submissions s ON s.id = a.evidence_submission_id
JOIN evidence_requests er ON er.id = s.evidence_request_id
WHERE a.id = $1
  AND a.evidence_submission_id = $2
  AND a.object_key = $3
  AND a.upload_status = 'finalizing'
"#,
                &[
                    &Uuid::from(evidence_attachment_id),
                    &Uuid::from(evidence_submission_id),
                    &quarantine_object_key,
                ],
            )
            .await?;

        rows.into_iter()
            .next()
            .map(|row| pending_attachment_upload_work_from_row(&row))
            .transpose()
    }

    pub async fn mark_attachment_uploaded(
        &self,
        evidence_attachment_id: EvidenceAttachmentId,
        quarantine_object_key: &str,
        final_object_key: &str,
    ) -> Result<bool, Error> {
        let client = self.get().await?;
        let rows = client
            .execute(
                r#"
UPDATE evidence_attachments a
SET object_key = $3,
    upload_status = 'uploaded'
WHERE a.id = $1
  AND a.object_key = $2
  AND a.upload_status = 'finalizing'
"#,
                &[
                    &Uuid::from(evidence_attachment_id),
                    &quarantine_object_key,
                    &final_object_key,
                ],
            )
            .await?;

        Ok(rows > 0)
    }

    pub async fn mark_attachment_contains_virus(
        &self,
        evidence_attachment_id: EvidenceAttachmentId,
        quarantine_object_key: &str,
    ) -> Result<bool, Error> {
        self.mark_attachment_terminal_upload_status(
            evidence_attachment_id,
            quarantine_object_key,
            AttachmentUploadStatus::ContainsVirus,
        )
        .await
    }

    pub async fn mark_attachment_upload_failed(
        &self,
        evidence_attachment_id: EvidenceAttachmentId,
        quarantine_object_key: &str,
    ) -> Result<bool, Error> {
        self.mark_attachment_terminal_upload_status(
            evidence_attachment_id,
            quarantine_object_key,
            AttachmentUploadStatus::FailedUpload,
        )
        .await
    }

    async fn mark_attachment_terminal_upload_status(
        &self,
        evidence_attachment_id: EvidenceAttachmentId,
        quarantine_object_key: &str,
        status: AttachmentUploadStatus,
    ) -> Result<bool, Error> {
        let client = self.get().await?;
        let rows = client
            .execute(
                r#"
UPDATE evidence_attachments a
SET upload_status = $3
WHERE a.id = $1
  AND a.object_key = $2
  AND a.upload_status = 'pending'
"#,
                &[
                    &Uuid::from(evidence_attachment_id),
                    &quarantine_object_key,
                    &status.as_str(),
                ],
            )
            .await?;

        Ok(rows > 0)
    }
}

impl TransactionContext<'_> {
    pub async fn request_attachment_finalization(
        &self,
        work: &PendingAttachmentUploadWork,
    ) -> Result<bool, Error> {
        let updated = self
            .transaction
            .execute(
                r#"
UPDATE evidence_attachments
SET upload_status = 'finalizing'
WHERE id = $1
  AND evidence_submission_id = $2
  AND object_key = $3
  AND upload_status = 'pending'
"#,
                &[
                    &Uuid::from(work.evidence_attachment_id),
                    &Uuid::from(work.evidence_submission_id),
                    &work.object_key,
                ],
            )
            .await?;

        Ok(updated > 0)
    }
}

impl WorkspaceTransactionContext<'_> {
    pub async fn create_evidence_submission(
        &self,
        payload: &CreateEvidenceSubmissionPayload,
    ) -> Result<Option<EvidenceSubmission>, Error> {
        let api_token_id = self.credential.api_token_uuid();
        let agent_connection_id = self.credential.agent_connection_uuid();
        let rows = self
            .transaction
            .query(
                r#"
WITH inserted AS (
	    INSERT INTO evidence_submissions (
	        evidence_request_id,
	        submitted_by_api_token_id,
	        submitted_by_agent_connection_id,
	        coverage_start_at,
	        coverage_end_at,
	        source_system,
        collection_method,
	        summary,
	        description
	    )
	    SELECT er.id, $2, $3, $4, $5, $6, $7, $8, $9
	    FROM evidence_requests er
	    WHERE er.id = $1
	      AND er.workspace_id = $10
	    RETURNING
	        id,
	        evidence_request_id,
	        submitted_by_api_token_id,
	        submitted_by_agent_connection_id,
	        received_at,
	        coverage_start_at,
        coverage_end_at,
        source_system,
        collection_method,
        summary,
        description
)
SELECT
	    inserted.id,
	    inserted.evidence_request_id,
	    inserted.submitted_by_api_token_id,
	    inserted.submitted_by_agent_connection_id,
	    COALESCE(t.user_id, c.user_id) AS submitted_by_user_id,
	    inserted.received_at,
    inserted.coverage_start_at,
    inserted.coverage_end_at,
    inserted.source_system,
    inserted.collection_method,
	    inserted.summary,
	    inserted.description
	FROM inserted
	LEFT JOIN api_tokens t ON t.id = inserted.submitted_by_api_token_id
	LEFT JOIN agent_connections c ON c.id = inserted.submitted_by_agent_connection_id
	"#,
                &[
                    &Uuid::from(payload.evidence_request_id),
                    &api_token_id,
                    &agent_connection_id,
                    &payload.coverage_start_at,
                    &payload.coverage_end_at,
                    &payload.source_system,
                    &payload.collection_method,
                    &payload.summary,
                    &payload.description,
                    &Uuid::from(self.workspace_id),
                ],
            )
            .await?;

        rows.into_iter()
            .next()
            .map(|row| evidence_submission_from_row(&row))
            .transpose()
    }
}

impl WorkspaceReadContext {
    pub async fn get_attachment_for_download_grant(
        &self,
        evidence_submission_id: EvidenceSubmissionId,
        evidence_attachment_id: EvidenceAttachmentId,
    ) -> Result<Option<AttachmentDownloadCandidate>, Error> {
        let rows = &self
            .client
            .query(
                r#"
SELECT
    er.workspace_id,
    a.id AS attachment_id,
    a.evidence_submission_id AS attachment_submission_id,
    a.filename,
    a.content_type,
    a.content_length,
    a.object_key,
    a.checksum_sha256,
    a.checksum_crc32c,
    a.upload_status
FROM evidence_attachments a
JOIN evidence_submissions s ON s.id = a.evidence_submission_id
JOIN evidence_requests er ON er.id = s.evidence_request_id
WHERE er.workspace_id = $1
  AND s.id = $2
  AND a.id = $3
  AND a.archived = false
"#,
                &[
                    &Uuid::from(self.workspace_id),
                    &Uuid::from(evidence_submission_id),
                    &Uuid::from(evidence_attachment_id),
                ],
            )
            .await?;

        rows.iter()
            .next()
            .map(attachment_download_candidate_from_row)
            .transpose()
    }

    pub async fn evidence_submission_exists(
        &self,
        id: EvidenceSubmissionId,
    ) -> Result<bool, Error> {
        let rows = self
            .client
            .query(
                r#"
SELECT 1
FROM evidence_submissions s
JOIN evidence_requests er ON er.id = s.evidence_request_id
WHERE s.id = $1
  AND er.workspace_id = $2
LIMIT 1
"#,
                &[&Uuid::from(id), &Uuid::from(self.workspace_id)],
            )
            .await?;

        Ok(!rows.is_empty())
    }

    pub async fn get_evidence_submission(
        &self,
        id: EvidenceSubmissionId,
    ) -> Result<Option<EvidenceSubmissionDetail>, Error> {
        let rows = self
            .client
            .query(
                r#"
SELECT
	    s.id,
	    s.evidence_request_id,
	    s.submitted_by_api_token_id,
	    s.submitted_by_agent_connection_id,
	    COALESCE(t.user_id, c.user_id) AS submitted_by_user_id,
    s.received_at,
    s.coverage_start_at,
    s.coverage_end_at,
    s.source_system,
    s.collection_method,
    s.summary,
    s.description,
    a.id AS attachment_id,
    a.evidence_submission_id AS attachment_submission_id,
    a.filename,
    a.content_type,
    a.content_length,
    a.object_key,
    a.checksum_sha256,
    a.checksum_crc32c,
    a.upload_status
	FROM evidence_submissions s
	JOIN evidence_requests er ON er.id = s.evidence_request_id
	LEFT JOIN api_tokens t ON t.id = s.submitted_by_api_token_id
	LEFT JOIN agent_connections c ON c.id = s.submitted_by_agent_connection_id
LEFT JOIN evidence_attachments a ON a.evidence_submission_id = s.id
    AND a.archived = false
WHERE s.id = $1
  AND er.workspace_id = $2
ORDER BY a.filename, a.id
"#,
                &[&Uuid::from(id), &Uuid::from(self.workspace_id)],
            )
            .await?;

        evidence_submission_detail_from_rows(rows)
    }

    pub async fn latest_evidence_submission_for_request(
        &self,
        evidence_request_id: EvidenceRequestId,
    ) -> Result<Option<EvidenceSubmissionDetail>, Error> {
        let rows = self
            .client
            .query(
                r#"
WITH latest_submission AS (
    SELECT s.id
    FROM evidence_submissions s
    JOIN evidence_requests er ON er.id = s.evidence_request_id
    WHERE s.evidence_request_id = $1
      AND er.workspace_id = $2
    ORDER BY s.received_at DESC, s.id DESC
    LIMIT 1
)
SELECT
	    s.id,
	    s.evidence_request_id,
	    s.submitted_by_api_token_id,
	    s.submitted_by_agent_connection_id,
	    COALESCE(t.user_id, c.user_id) AS submitted_by_user_id,
    s.received_at,
    s.coverage_start_at,
    s.coverage_end_at,
    s.source_system,
    s.collection_method,
    s.summary,
    s.description,
    a.id AS attachment_id,
    a.evidence_submission_id AS attachment_submission_id,
    a.filename,
    a.content_type,
    a.content_length,
    a.object_key,
    a.checksum_sha256,
    a.checksum_crc32c,
    a.upload_status
	FROM latest_submission latest
	JOIN evidence_submissions s ON s.id = latest.id
	LEFT JOIN api_tokens t ON t.id = s.submitted_by_api_token_id
	LEFT JOIN agent_connections c ON c.id = s.submitted_by_agent_connection_id
LEFT JOIN evidence_attachments a ON a.evidence_submission_id = s.id
    AND a.archived = false
ORDER BY a.filename, a.id
"#,
                &[
                    &Uuid::from(evidence_request_id),
                    &Uuid::from(self.workspace_id),
                ],
            )
            .await?;

        evidence_submission_detail_from_rows(rows)
    }
}

impl WorkspaceTransactionContext<'_> {
    pub async fn create_evidence_attachment(
        &self,
        payload: &CreateEvidenceAttachmentPayload,
    ) -> Result<EvidenceAttachment, Error> {
        let rows = self
            .transaction
            .query(
                r#"
INSERT INTO evidence_attachments (
    evidence_submission_id,
    filename,
    content_type,
    content_length,
    object_key,
    checksum_sha256,
    checksum_crc32c,
    upload_status
)
SELECT s.id, $2, $3, $4, $5, $6, $7, 'pending'
FROM evidence_submissions s
JOIN evidence_requests er ON er.id = s.evidence_request_id
WHERE s.id = $1
  AND er.workspace_id = $8
RETURNING
    id,
    evidence_submission_id,
    filename,
    content_type,
    content_length,
    object_key,
    checksum_sha256,
    checksum_crc32c,
    upload_status
"#,
                &[
                    &Uuid::from(payload.evidence_submission_id),
                    &payload.filename,
                    &payload.content_type,
                    &payload.content_length,
                    &payload.object_key,
                    &payload.checksum_sha256,
                    &payload.checksum_crc32c,
                    &Uuid::from(self.workspace_id),
                ],
            )
            .await?;

        let Some(row) = rows.into_iter().next() else {
            return Err(Error::InvariantViolation(
                "attachment insert requires an existing workspace-scoped submission",
            ));
        };
        evidence_attachment_from_row(&row)
    }

    pub async fn create_first_evidence_attachment(
        &self,
        payload: &CreateEvidenceAttachmentPayload,
    ) -> Result<Option<EvidenceAttachment>, Error> {
        let locked_submission = self
            .transaction
            .query_opt(
                r#"
SELECT s.id
FROM evidence_submissions s
JOIN evidence_requests er ON er.id = s.evidence_request_id
WHERE s.id = $1
  AND er.workspace_id = $2
FOR UPDATE OF s
"#,
                &[
                    &Uuid::from(payload.evidence_submission_id),
                    &Uuid::from(self.workspace_id),
                ],
            )
            .await?;

        if locked_submission.is_none() {
            return Err(Error::InvariantViolation(
                "attachment insert requires an existing workspace-scoped submission",
            ));
        }

        let row = self
            .transaction
            .query_opt(
                r#"
INSERT INTO evidence_attachments (
    evidence_submission_id,
    filename,
    content_type,
    content_length,
    object_key,
    checksum_sha256,
    checksum_crc32c,
    upload_status
)
SELECT $1, $2, $3, $4, $5, $6, $7, 'pending'
WHERE NOT EXISTS (
    SELECT 1
    FROM evidence_attachments
    WHERE evidence_submission_id = $1
)
RETURNING
    id,
    evidence_submission_id,
    filename,
    content_type,
    content_length,
    object_key,
    checksum_sha256,
    checksum_crc32c,
    upload_status
"#,
                &[
                    &Uuid::from(payload.evidence_submission_id),
                    &payload.filename,
                    &payload.content_type,
                    &payload.content_length,
                    &payload.object_key,
                    &payload.checksum_sha256,
                    &payload.checksum_crc32c,
                ],
            )
            .await?;

        row.map(|row| evidence_attachment_from_row(&row))
            .transpose()
    }

    pub async fn archive_evidence_attachment(
        &self,
        evidence_submission_id: EvidenceSubmissionId,
        evidence_attachment_id: EvidenceAttachmentId,
    ) -> Result<ArchiveAttachmentResult, Error> {
        let row = self
            .transaction
            .query_opt(
                r#"
WITH scoped AS (
    SELECT a.id, a.upload_status, a.archived
    FROM evidence_attachments a
    JOIN evidence_submissions s ON s.id = a.evidence_submission_id
    JOIN evidence_requests er ON er.id = s.evidence_request_id
    WHERE er.workspace_id = $1
      AND s.id = $2
      AND a.id = $3
),
updated AS (
    UPDATE evidence_attachments a
    SET archived = true
    FROM scoped
    WHERE a.id = scoped.id
      AND (scoped.archived = true OR scoped.upload_status IN ('uploaded', 'contains_virus', 'failed'))
    RETURNING a.id
)
SELECT
    EXISTS (SELECT 1 FROM scoped) AS found,
    EXISTS (SELECT 1 FROM updated) AS archived
"#,
                &[
                    &Uuid::from(self.workspace_id),
                    &Uuid::from(evidence_submission_id),
                    &Uuid::from(evidence_attachment_id),
                ],
            )
            .await?
            .ok_or(Error::InvariantViolation("archive query must return one row"))?;

        match (
            row.try_get::<_, bool>("found")?,
            row.try_get::<_, bool>("archived")?,
        ) {
            (false, _) => Ok(ArchiveAttachmentResult::NotFound),
            (true, true) => Ok(ArchiveAttachmentResult::Archived),
            (true, false) => Ok(ArchiveAttachmentResult::NotTerminal),
        }
    }
}

fn evidence_submission_detail_from_rows(
    rows: Vec<Row>,
) -> Result<Option<EvidenceSubmissionDetail>, Error> {
    let Some(first_row) = rows.first() else {
        return Ok(None);
    };

    let submission = evidence_submission_from_row(first_row)?;
    let attachments = rows
        .iter()
        .filter_map(evidence_attachment_from_optional_row)
        .collect::<Result<Vec<_>, _>>()?;

    Ok(Some(EvidenceSubmissionDetail {
        submission,
        attachments,
    }))
}

fn evidence_attachment_from_optional_row(row: &Row) -> Option<Result<EvidenceAttachment, Error>> {
    match row.try_get::<_, Option<Uuid>>("attachment_id") {
        Ok(Some(_)) => {}
        Ok(None) => return None,
        Err(error) => return Some(Err(Error::Database(error))),
    }

    Some(evidence_attachment_from_row(row))
}

fn evidence_submission_from_row(row: &Row) -> Result<EvidenceSubmission, Error> {
    Ok(EvidenceSubmission {
        id: EvidenceSubmissionId::from(row.try_get::<_, Uuid>("id")?),
        evidence_request_id: EvidenceRequestId::from(
            row.try_get::<_, Uuid>("evidence_request_id")?,
        ),
        submitted_by: evidence_submitter_from_row(row)?,
        received_at: row.try_get("received_at")?,
        coverage_start_at: row.try_get("coverage_start_at")?,
        coverage_end_at: row.try_get("coverage_end_at")?,
        source_system: row.try_get("source_system")?,
        collection_method: row.try_get("collection_method")?,
        summary: row.try_get("summary")?,
        description: row.try_get("description")?,
    })
}

fn evidence_submitter_from_row(row: &Row) -> Result<EvidenceSubmitter, Error> {
    let user_id = row.try_get::<_, Uuid>("submitted_by_user_id")?.into();
    let api_token_id = row.try_get::<_, Option<Uuid>>("submitted_by_api_token_id")?;
    let agent_connection_id = row.try_get::<_, Option<Uuid>>("submitted_by_agent_connection_id")?;

    match (api_token_id, agent_connection_id) {
        (Some(api_token_id), None) => Ok(EvidenceSubmitter::ApiToken {
            api_token_id: api_token_id.into(),
            user_id,
        }),
        (None, Some(agent_connection_id)) => Ok(EvidenceSubmitter::AgentConnection {
            agent_connection_id: agent_connection_id.into(),
            user_id,
        }),
        _ => Err(Error::InvariantViolation(
            "evidence submission must have exactly one submitter source",
        )),
    }
}

fn evidence_attachment_from_row(row: &Row) -> Result<EvidenceAttachment, Error> {
    Ok(EvidenceAttachment {
        id: EvidenceAttachmentId::from(
            row.try_get::<_, Uuid>("attachment_id")
                .or_else(|_| row.try_get::<_, Uuid>("id"))?,
        ),
        evidence_submission_id: EvidenceSubmissionId::from(
            row.try_get::<_, Uuid>("attachment_submission_id")
                .or_else(|_| row.try_get::<_, Uuid>("evidence_submission_id"))?,
        ),
        filename: row.try_get("filename")?,
        content_type: row.try_get("content_type")?,
        content_length: row.try_get("content_length")?,
        object_key: row.try_get("object_key")?,
        checksum_sha256: row.try_get("checksum_sha256")?,
        checksum_crc32c: row.try_get("checksum_crc32c")?,
        upload_status: row
            .try_get::<_, String>("upload_status")?
            .parse::<AttachmentUploadStatus>()?,
    })
}

fn pending_attachment_upload_work_from_row(
    row: &Row,
) -> Result<PendingAttachmentUploadWork, Error> {
    let upload_status = row
        .try_get::<_, String>("upload_status")?
        .parse::<AttachmentUploadStatus>()?;

    Ok(PendingAttachmentUploadWork {
        workspace_id: WorkspaceId::from(row.try_get::<_, Uuid>("workspace_id")?),
        evidence_submission_id: EvidenceSubmissionId::from(
            row.try_get::<_, Uuid>("evidence_submission_id")?,
        ),
        evidence_attachment_id: EvidenceAttachmentId::from(
            row.try_get::<_, Uuid>("attachment_id")?,
        ),
        filename: row.try_get("filename")?,
        content_type: row.try_get("content_type")?,
        content_length: row.try_get("content_length")?,
        object_key: row.try_get("object_key")?,
        checksum_sha256: row.try_get("checksum_sha256")?,
        upload_status,
    })
}

fn attachment_download_candidate_from_row(row: &Row) -> Result<AttachmentDownloadCandidate, Error> {
    Ok(AttachmentDownloadCandidate {
        workspace_id: WorkspaceId::from(row.try_get::<_, Uuid>("workspace_id")?),
        attachment: evidence_attachment_from_row(row)?,
    })
}
