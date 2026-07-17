use deadpool_postgres::GenericClient;
use tokio_postgres::Row;
use uuid::Uuid;

use crate::{
    domain::{
        CoverageWindow, CreateEvidenceSubmissionPayload, EvidenceId, EvidenceSubmission,
        EvidenceSubmissionId, EvidenceSubmitter, SubmissionUploadStatus, WorkspaceId,
    },
    repository::{WorkspaceReadContext, WorkspaceTransactionContext},
};

use super::{Error, Postgres, TransactionContext};

/// The columns every submission read selects, aliased `s`. Kept in one place
/// because the row mapper reads them back by name.
const SUBMISSION_COLUMNS: &str = r#"
    s.id,
    s.evidence_id,
    s.submitted_by_agent_connection_id,
    c.user_id AS submitted_by_user_id,
    s.received_at,
    s.valid_from,
    s.valid_until,
    s.filename,
    s.content_type,
    s.content_length,
    s.object_key,
    s.checksum_sha256,
    s.checksum_crc32c,
    s.upload_status,
    s.archived
"#;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingSubmissionUploadWork {
    pub workspace_id: WorkspaceId,
    pub evidence_id: EvidenceId,
    pub evidence_submission_id: EvidenceSubmissionId,
    pub filename: String,
    pub content_type: String,
    pub content_length: i64,
    pub object_key: String,
    pub checksum_sha256: String,
    pub upload_status: SubmissionUploadStatus,
}

pub type FinalizingSubmissionUploadWork = PendingSubmissionUploadWork;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubmissionDownloadCandidate {
    pub workspace_id: WorkspaceId,
    pub submission: EvidenceSubmission,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveSubmissionResult {
    Archived,
    NotFound,
    NotTerminal,
}

impl Postgres {
    pub async fn load_pending_submission_upload_work(
        &self,
        evidence_submission_id: EvidenceSubmissionId,
        quarantine_object_key: &str,
    ) -> Result<Option<PendingSubmissionUploadWork>, Error> {
        let client = self.get().await?;
        let rows = client
            .query(
                r#"
SELECT
    e.workspace_id,
    s.id,
    s.evidence_id,
    s.filename,
    s.content_type,
    s.content_length,
    s.object_key,
    s.checksum_sha256,
    s.upload_status
FROM evidence_submissions s
JOIN evidence e ON e.id = s.evidence_id
WHERE s.id = $1
  AND s.object_key = $2
  AND s.upload_status = 'pending'
"#,
                &[&Uuid::from(evidence_submission_id), &quarantine_object_key],
            )
            .await?;

        rows.into_iter()
            .next()
            .map(|row| pending_submission_upload_work_from_row(&row))
            .transpose()
    }

    pub async fn load_finalizing_submission_upload_work(
        &self,
        evidence_submission_id: EvidenceSubmissionId,
        quarantine_object_key: &str,
    ) -> Result<Option<FinalizingSubmissionUploadWork>, Error> {
        let client = self.get().await?;
        let rows = client
            .query(
                r#"
SELECT
    e.workspace_id,
    s.id,
    s.evidence_id,
    s.filename,
    s.content_type,
    s.content_length,
    s.object_key,
    s.checksum_sha256,
    s.upload_status
FROM evidence_submissions s
JOIN evidence e ON e.id = s.evidence_id
WHERE s.id = $1
  AND s.object_key = $2
  AND s.upload_status = 'finalizing'
"#,
                &[&Uuid::from(evidence_submission_id), &quarantine_object_key],
            )
            .await?;

        rows.into_iter()
            .next()
            .map(|row| pending_submission_upload_work_from_row(&row))
            .transpose()
    }

    pub async fn mark_submission_uploaded(
        &self,
        evidence_submission_id: EvidenceSubmissionId,
        quarantine_object_key: &str,
        final_object_key: &str,
    ) -> Result<bool, Error> {
        let client = self.get().await?;
        let rows = client
            .execute(
                r#"
UPDATE evidence_submissions
SET object_key = $3,
    upload_status = 'uploaded'
WHERE id = $1
  AND object_key = $2
  AND upload_status = 'finalizing'
"#,
                &[
                    &Uuid::from(evidence_submission_id),
                    &quarantine_object_key,
                    &final_object_key,
                ],
            )
            .await?;

        Ok(rows > 0)
    }

    pub async fn mark_submission_contains_virus(
        &self,
        evidence_submission_id: EvidenceSubmissionId,
        quarantine_object_key: &str,
    ) -> Result<bool, Error> {
        self.mark_submission_terminal_upload_status(
            evidence_submission_id,
            quarantine_object_key,
            SubmissionUploadStatus::ContainsVirus,
        )
        .await
    }

    pub async fn mark_submission_upload_failed(
        &self,
        evidence_submission_id: EvidenceSubmissionId,
        quarantine_object_key: &str,
    ) -> Result<bool, Error> {
        self.mark_submission_terminal_upload_status(
            evidence_submission_id,
            quarantine_object_key,
            SubmissionUploadStatus::FailedUpload,
        )
        .await
    }

    async fn mark_submission_terminal_upload_status(
        &self,
        evidence_submission_id: EvidenceSubmissionId,
        quarantine_object_key: &str,
        status: SubmissionUploadStatus,
    ) -> Result<bool, Error> {
        let client = self.get().await?;
        let rows = client
            .execute(
                r#"
UPDATE evidence_submissions
SET upload_status = $3
WHERE id = $1
  AND object_key = $2
  AND upload_status = 'pending'
"#,
                &[
                    &Uuid::from(evidence_submission_id),
                    &quarantine_object_key,
                    &status.as_str(),
                ],
            )
            .await?;

        Ok(rows > 0)
    }
}

impl TransactionContext<'_> {
    pub async fn request_submission_finalization(
        &self,
        work: &PendingSubmissionUploadWork,
    ) -> Result<bool, Error> {
        let updated = self
            .transaction
            .execute(
                r#"
UPDATE evidence_submissions
SET upload_status = 'finalizing'
WHERE id = $1
  AND object_key = $2
  AND upload_status = 'pending'
"#,
                &[&Uuid::from(work.evidence_submission_id), &work.object_key],
            )
            .await?;

        Ok(updated > 0)
    }
}

impl WorkspaceTransactionContext<'_> {
    pub async fn create_evidence_submission(
        &self,
        evidence_id: EvidenceId,
        coverage: CoverageWindow,
        payload: &CreateEvidenceSubmissionPayload,
    ) -> Result<Option<EvidenceSubmission>, Error> {
        let agent_connection_id = self.credential.agent_connection_uuid();
        let rows = self
            .transaction
            .query(
                r#"
WITH inserted AS (
    INSERT INTO evidence_submissions (
        evidence_id,
        submitted_by_agent_connection_id,
        valid_from,
        valid_until,
        filename,
        content_type,
        content_length,
        object_key,
        checksum_sha256,
        checksum_crc32c,
        upload_status
    )
    SELECT e.id, $2, $3, $4, $5, $6, $7, $8, $9, $10, 'pending'
    FROM evidence e
    WHERE e.id = $1
      AND e.workspace_id = $11
    RETURNING
        id,
        evidence_id,
        submitted_by_agent_connection_id,
        received_at,
        valid_from,
        valid_until,
        filename,
        content_type,
        content_length,
        object_key,
        checksum_sha256,
        checksum_crc32c,
        upload_status,
        archived
)
SELECT
    s.id,
    s.evidence_id,
    s.submitted_by_agent_connection_id,
    c.user_id AS submitted_by_user_id,
    s.received_at,
    s.valid_from,
    s.valid_until,
    s.filename,
    s.content_type,
    s.content_length,
    s.object_key,
    s.checksum_sha256,
    s.checksum_crc32c,
    s.upload_status,
    s.archived
FROM inserted s
LEFT JOIN agent_connections c ON c.id = s.submitted_by_agent_connection_id
"#,
                &[
                    &Uuid::from(evidence_id),
                    &agent_connection_id,
                    &coverage.valid_from,
                    &coverage.valid_until,
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

        rows.into_iter()
            .next()
            .map(|row| evidence_submission_from_row(&row))
            .transpose()
    }

    pub async fn archive_evidence_submission(
        &self,
        evidence_submission_id: EvidenceSubmissionId,
    ) -> Result<ArchiveSubmissionResult, Error> {
        let Some(row) = self
            .transaction
            .query_opt(
                r#"
SELECT s.upload_status, s.archived
FROM evidence_submissions s
JOIN evidence e ON e.id = s.evidence_id
WHERE e.workspace_id = $1
  AND s.id = $2
FOR UPDATE OF s
"#,
                &[
                    &Uuid::from(self.workspace_id),
                    &Uuid::from(evidence_submission_id),
                ],
            )
            .await?
        else {
            return Ok(ArchiveSubmissionResult::NotFound);
        };

        let archived: bool = row.try_get("archived")?;
        let upload_status = row
            .try_get::<_, String>("upload_status")?
            .parse::<SubmissionUploadStatus>()?;

        if !archived && !upload_status.is_terminal() {
            return Ok(ArchiveSubmissionResult::NotTerminal);
        }

        self.transaction
            .execute(
                "UPDATE evidence_submissions SET archived = true WHERE id = $1",
                &[&Uuid::from(evidence_submission_id)],
            )
            .await?;

        Ok(ArchiveSubmissionResult::Archived)
    }
}

impl WorkspaceReadContext {
    pub async fn get_submission_for_download_grant(
        &self,
        evidence_submission_id: EvidenceSubmissionId,
    ) -> Result<Option<SubmissionDownloadCandidate>, Error> {
        let rows = self
            .client
            .query(
                &format!(
                    r#"
SELECT
    e.workspace_id,
    {SUBMISSION_COLUMNS}
FROM evidence_submissions s
JOIN evidence e ON e.id = s.evidence_id
LEFT JOIN agent_connections c ON c.id = s.submitted_by_agent_connection_id
WHERE e.workspace_id = $1
  AND s.id = $2
  AND s.archived = false
"#
                ),
                &[
                    &Uuid::from(self.workspace_id),
                    &Uuid::from(evidence_submission_id),
                ],
            )
            .await?;

        rows.first()
            .map(submission_download_candidate_from_row)
            .transpose()
    }

    pub async fn get_evidence_submission(
        &self,
        id: EvidenceSubmissionId,
    ) -> Result<Option<EvidenceSubmission>, Error> {
        let rows = self
            .client
            .query(
                &format!(
                    r#"
SELECT
    {SUBMISSION_COLUMNS}
FROM evidence_submissions s
JOIN evidence e ON e.id = s.evidence_id
LEFT JOIN agent_connections c ON c.id = s.submitted_by_agent_connection_id
WHERE s.id = $1
  AND e.workspace_id = $2
  AND s.archived = false
"#
                ),
                &[&Uuid::from(id), &Uuid::from(self.workspace_id)],
            )
            .await?;

        rows.first().map(evidence_submission_from_row).transpose()
    }

    pub async fn list_evidence_submissions(
        &self,
        evidence_id: EvidenceId,
    ) -> Result<Vec<EvidenceSubmission>, Error> {
        let rows = self
            .client
            .query(
                &format!(
                    r#"
SELECT
    {SUBMISSION_COLUMNS}
FROM evidence_submissions s
JOIN evidence e ON e.id = s.evidence_id
LEFT JOIN agent_connections c ON c.id = s.submitted_by_agent_connection_id
WHERE s.evidence_id = $1
  AND e.workspace_id = $2
  AND s.archived = false
ORDER BY s.received_at DESC, s.id DESC
"#
                ),
                &[&Uuid::from(evidence_id), &Uuid::from(self.workspace_id)],
            )
            .await?;

        rows.iter().map(evidence_submission_from_row).collect()
    }

    /// The submissions an upload session may manage: everything filed against
    /// the session's evidence for the exact window the grant was issued for.
    pub async fn list_evidence_submissions_for_coverage(
        &self,
        evidence_id: EvidenceId,
        coverage: CoverageWindow,
    ) -> Result<Vec<EvidenceSubmission>, Error> {
        let rows = self
            .client
            .query(
                &format!(
                    r#"
SELECT
    {SUBMISSION_COLUMNS}
FROM evidence_submissions s
JOIN evidence e ON e.id = s.evidence_id
LEFT JOIN agent_connections c ON c.id = s.submitted_by_agent_connection_id
WHERE s.evidence_id = $1
  AND e.workspace_id = $2
  AND s.valid_from = $3
  AND s.valid_until = $4
  AND s.archived = false
ORDER BY s.received_at DESC, s.id DESC
"#
                ),
                &[
                    &Uuid::from(evidence_id),
                    &Uuid::from(self.workspace_id),
                    &coverage.valid_from,
                    &coverage.valid_until,
                ],
            )
            .await?;

        rows.iter().map(evidence_submission_from_row).collect()
    }

    pub async fn evidence_exists(&self, id: EvidenceId) -> Result<bool, Error> {
        let rows = self
            .client
            .query(
                r#"
SELECT 1
FROM evidence
WHERE id = $1
  AND workspace_id = $2
LIMIT 1
"#,
                &[&Uuid::from(id), &Uuid::from(self.workspace_id)],
            )
            .await?;

        Ok(!rows.is_empty())
    }
}

fn evidence_submission_from_row(row: &Row) -> Result<EvidenceSubmission, Error> {
    Ok(EvidenceSubmission {
        id: EvidenceSubmissionId::from(row.try_get::<_, Uuid>("id")?),
        evidence_id: EvidenceId::from(row.try_get::<_, Uuid>("evidence_id")?),
        submitted_by: evidence_submitter_from_row(row)?,
        received_at: row.try_get("received_at")?,
        valid_from: row.try_get("valid_from")?,
        valid_until: row.try_get("valid_until")?,
        filename: row.try_get("filename")?,
        content_type: row.try_get("content_type")?,
        content_length: row.try_get("content_length")?,
        object_key: row.try_get("object_key")?,
        checksum_sha256: row.try_get("checksum_sha256")?,
        checksum_crc32c: row.try_get("checksum_crc32c")?,
        upload_status: row
            .try_get::<_, String>("upload_status")?
            .parse::<SubmissionUploadStatus>()?,
        archived: row.try_get("archived")?,
    })
}

fn evidence_submitter_from_row(row: &Row) -> Result<EvidenceSubmitter, Error> {
    let user_id = row.try_get::<_, Uuid>("submitted_by_user_id")?.into();
    let agent_connection_id = row.try_get::<_, Option<Uuid>>("submitted_by_agent_connection_id")?;

    match agent_connection_id {
        Some(agent_connection_id) => Ok(EvidenceSubmitter::AgentConnection {
            agent_connection_id: agent_connection_id.into(),
            user_id,
        }),
        None => Err(Error::InvariantViolation(
            "evidence submission must have an agent connection submitter",
        )),
    }
}

fn pending_submission_upload_work_from_row(
    row: &Row,
) -> Result<PendingSubmissionUploadWork, Error> {
    let upload_status = row
        .try_get::<_, String>("upload_status")?
        .parse::<SubmissionUploadStatus>()?;

    Ok(PendingSubmissionUploadWork {
        workspace_id: WorkspaceId::from(row.try_get::<_, Uuid>("workspace_id")?),
        evidence_id: EvidenceId::from(row.try_get::<_, Uuid>("evidence_id")?),
        evidence_submission_id: EvidenceSubmissionId::from(row.try_get::<_, Uuid>("id")?),
        filename: row.try_get("filename")?,
        content_type: row.try_get("content_type")?,
        content_length: row.try_get("content_length")?,
        object_key: row.try_get("object_key")?,
        checksum_sha256: row.try_get("checksum_sha256")?,
        upload_status,
    })
}

fn submission_download_candidate_from_row(row: &Row) -> Result<SubmissionDownloadCandidate, Error> {
    Ok(SubmissionDownloadCandidate {
        workspace_id: WorkspaceId::from(row.try_get::<_, Uuid>("workspace_id")?),
        submission: evidence_submission_from_row(row)?,
    })
}
