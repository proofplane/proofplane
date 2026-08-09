use chrono::{DateTime, Utc};
use deadpool_postgres::GenericClient;
use tokio_postgres::Row;
use uuid::Uuid;

use crate::{
    domain::{
        CoverageWindow, CreateDocumentPayload, CreateEvidenceSubmissionPayload, Document,
        DocumentId, DocumentIdentity, DocumentOwner, DocumentUploadStatus, EvidenceId,
        EvidenceSubmission, EvidenceSubmissionDetail, EvidenceSubmissionId, EvidenceSubmitter,
        WorkspaceId,
    },
    repository::{WorkspaceReadContext, WorkspaceTransactionContext},
};

use super::{documents::document_from_row, Error, Postgres, TransactionContext};

/// Complete-snapshot repository for the submission provenance aggregate.
pub struct EvidenceSubmissionRepository<'a> {
    context: &'a WorkspaceTransactionContext<'a>,
}

impl<'a> WorkspaceTransactionContext<'a> {
    pub fn evidence_submissions(&'a self) -> EvidenceSubmissionRepository<'a> {
        EvidenceSubmissionRepository { context: self }
    }
}

impl EvidenceSubmissionRepository<'_> {
    pub async fn get(&self, id: EvidenceSubmissionId) -> Result<Option<EvidenceSubmission>, Error> {
        self.context
            .transaction
            .query_opt(
                r#"SELECT s.id, s.evidence_id, s.submitted_by_agent_connection_id,
 c.user_id AS submitted_by_user_id, s.received_at, s.valid_from, s.valid_until
FROM evidence_submissions s
JOIN evidence e ON e.id = s.evidence_id
JOIN agent_connections c ON c.id = s.submitted_by_agent_connection_id
WHERE s.id = $1 AND e.workspace_id = $2
FOR UPDATE OF s"#,
                &[&Uuid::from(id), &Uuid::from(self.context.workspace_id)],
            )
            .await?
            .map(|row| evidence_submission_from_row(&row))
            .transpose()
    }

    /// Persists the entire submission snapshot; evidence eligibility is checked
    /// by the command handler before this boundary is called.
    pub async fn save(&self, submission: &EvidenceSubmission) -> Result<(), Error> {
        let EvidenceSubmitter::AgentConnection {
            agent_connection_id,
            ..
        } = submission.submitted_by;
        let changed = self.context.transaction.execute(
            r#"INSERT INTO evidence_submissions (id, evidence_id, submitted_by_agent_connection_id, received_at, valid_from, valid_until)
VALUES ($1, $2, $3, $4, $5, $6)
ON CONFLICT (id) DO UPDATE SET evidence_id = EXCLUDED.evidence_id,
 submitted_by_agent_connection_id = EXCLUDED.submitted_by_agent_connection_id,
 received_at = EXCLUDED.received_at, valid_from = EXCLUDED.valid_from, valid_until = EXCLUDED.valid_until
WHERE EXISTS (
    SELECT 1 FROM evidence existing_evidence
    WHERE existing_evidence.id = evidence_submissions.evidence_id
      AND existing_evidence.workspace_id = $7
)"#,
            &[&Uuid::from(submission.id), &Uuid::from(submission.evidence_id), &Uuid::from(agent_connection_id),
              &submission.received_at, &submission.valid_from, &submission.valid_until,
              &Uuid::from(self.context.workspace_id)],
        ).await?;
        if changed != 1 {
            return Err(Error::InvariantViolation(
                "submission snapshot save must affect one row",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingDocumentUploadWork {
    pub workspace_id: WorkspaceId,
    pub evidence_submission_id: EvidenceSubmissionId,
    pub evidence_document_id: DocumentId,
    pub filename: String,
    pub content_type: String,
    pub content_length: i64,
    pub object_key: String,
    pub checksum_sha256: String,
    pub upload_status: DocumentUploadStatus,
}

pub type FinalizingDocumentUploadWork = PendingDocumentUploadWork;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentDownloadCandidate {
    pub workspace_id: WorkspaceId,
    pub document: Document,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveDocumentResult {
    Archived,
    NotFound,
    NotTerminal,
}

const SUBMISSION_DETAIL_COLUMNS: &str = r#"
    s.id,
    s.evidence_id,
    s.submitted_by_agent_connection_id,
    c.user_id AS submitted_by_user_id,
    s.received_at,
    s.valid_from,
    s.valid_until,
    a.id AS document_id,
    a.owner_id AS document_submission_id,
    a.filename,
    a.content_type,
    a.content_length,
    a.object_key,
    a.checksum_sha256,
    a.checksum_crc32c,
    a.created_by_user_id,
    a.upload_status,
    a.created_at
"#;

impl Postgres {
    pub async fn load_pending_document_upload_work(
        &self,
        evidence_document_id: DocumentId,
        quarantine_object_key: &str,
    ) -> Result<Option<PendingDocumentUploadWork>, Error> {
        let client = self.get().await?;
        let rows = client
            .query(
                r#"
SELECT
    a.workspace_id,
    a.owner_id AS evidence_submission_id,
    a.id AS document_id,
    a.filename,
    a.content_type,
    a.content_length,
    a.object_key,
    a.checksum_sha256,
    a.upload_status
FROM documents a
WHERE a.id = $1
  AND a.owner_type = 'evidence_submission'
  AND a.object_key = $2
  AND a.upload_status = 'pending'
"#,
                &[&Uuid::from(evidence_document_id), &quarantine_object_key],
            )
            .await?;

        rows.into_iter()
            .next()
            .map(|row| pending_document_upload_work_from_row(&row))
            .transpose()
    }

    pub async fn load_finalizing_document_upload_work(
        &self,
        evidence_document_id: DocumentId,
        evidence_submission_id: EvidenceSubmissionId,
        quarantine_object_key: &str,
    ) -> Result<Option<FinalizingDocumentUploadWork>, Error> {
        let client = self.get().await?;
        let rows = client
            .query(
                r#"
SELECT
    a.workspace_id,
    a.owner_id AS evidence_submission_id,
    a.id AS document_id,
    a.filename,
    a.content_type,
    a.content_length,
    a.object_key,
    a.checksum_sha256,
    a.upload_status
FROM documents a
WHERE a.id = $1
  AND a.owner_type = 'evidence_submission'
  AND a.owner_id = $2
  AND a.object_key = $3
  AND a.upload_status = 'finalizing'
"#,
                &[
                    &Uuid::from(evidence_document_id),
                    &Uuid::from(evidence_submission_id),
                    &quarantine_object_key,
                ],
            )
            .await?;

        rows.into_iter()
            .next()
            .map(|row| pending_document_upload_work_from_row(&row))
            .transpose()
    }

    pub async fn mark_document_uploaded(
        &self,
        evidence_document_id: DocumentId,
        quarantine_object_key: &str,
        final_object_key: &str,
    ) -> Result<bool, Error> {
        let client = self.get().await?;
        let rows = client
            .execute(
                r#"
UPDATE documents a
SET object_key = $3,
    upload_status = 'uploaded'
WHERE a.id = $1
  AND a.owner_type = 'evidence_submission'
  AND a.object_key = $2
  AND a.upload_status = 'finalizing'
"#,
                &[
                    &Uuid::from(evidence_document_id),
                    &quarantine_object_key,
                    &final_object_key,
                ],
            )
            .await?;

        Ok(rows > 0)
    }

    pub async fn mark_document_contains_virus(
        &self,
        evidence_document_id: DocumentId,
        quarantine_object_key: &str,
    ) -> Result<bool, Error> {
        self.mark_document_terminal_upload_status(
            evidence_document_id,
            quarantine_object_key,
            DocumentUploadStatus::ContainsVirus,
        )
        .await
    }

    pub async fn mark_document_upload_failed(
        &self,
        evidence_document_id: DocumentId,
        quarantine_object_key: &str,
    ) -> Result<bool, Error> {
        self.mark_document_terminal_upload_status(
            evidence_document_id,
            quarantine_object_key,
            DocumentUploadStatus::FailedUpload,
        )
        .await
    }

    async fn mark_document_terminal_upload_status(
        &self,
        evidence_document_id: DocumentId,
        quarantine_object_key: &str,
        status: DocumentUploadStatus,
    ) -> Result<bool, Error> {
        let client = self.get().await?;
        let rows = client
            .execute(
                r#"
UPDATE documents a
SET upload_status = $3
WHERE a.id = $1
  AND a.owner_type = 'evidence_submission'
  AND a.object_key = $2
  AND a.upload_status = 'pending'
"#,
                &[
                    &Uuid::from(evidence_document_id),
                    &quarantine_object_key,
                    &status.as_str(),
                ],
            )
            .await?;

        Ok(rows > 0)
    }
}

impl TransactionContext<'_> {
    pub async fn request_document_finalization(
        &self,
        work: &PendingDocumentUploadWork,
    ) -> Result<bool, Error> {
        let updated = self
            .transaction
            .execute(
                r#"
UPDATE documents
SET upload_status = 'finalizing'
WHERE id = $1
  AND owner_type = 'evidence_submission'
  AND owner_id = $2
  AND object_key = $3
  AND upload_status = 'pending'
"#,
                &[
                    &Uuid::from(work.evidence_document_id),
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
        let agent_connection_id = self.credential.agent_connection_uuid();
        let rows = self
            .transaction
            .query(
                r#"
WITH inserted AS (
    INSERT INTO evidence_submissions (
        id,
        evidence_id,
        submitted_by_agent_connection_id,
        valid_from,
        valid_until
    )
    SELECT $1, e.id, $3, $4, $5
    FROM evidence e
    WHERE e.id = $2
      AND e.workspace_id = $6
    RETURNING
        id,
        evidence_id,
        submitted_by_agent_connection_id,
        received_at,
        valid_from,
        valid_until
)
SELECT
    inserted.id,
    inserted.evidence_id,
    inserted.submitted_by_agent_connection_id,
    c.user_id AS submitted_by_user_id,
    inserted.received_at,
    inserted.valid_from,
    inserted.valid_until
FROM inserted
LEFT JOIN agent_connections c ON c.id = inserted.submitted_by_agent_connection_id
"#,
                &[
                    &Uuid::from(payload.id),
                    &Uuid::from(payload.evidence_id),
                    &agent_connection_id,
                    &payload.coverage.valid_from,
                    &payload.coverage.valid_until,
                    &Uuid::from(self.workspace_id),
                ],
            )
            .await?;

        rows.into_iter()
            .next()
            .map(|row| evidence_submission_from_row(&row))
            .transpose()
    }

    pub async fn create_evidence_document(
        &self,
        payload: &CreateDocumentPayload,
    ) -> Result<Document, Error> {
        let DocumentOwner::EvidenceSubmission(evidence_submission_id) = payload.owner else {
            return Err(Error::InvariantViolation(
                "evidence document creation requires an evidence submission owner",
            ));
        };
        let rows = self
            .transaction
            .query(
                r#"
INSERT INTO documents (
    workspace_id,
    owner_type,
    owner_id,
    filename,
    content_type,
    content_length,
    object_key,
    checksum_sha256,
    checksum_crc32c,
    created_by_user_id,
    upload_status
)
SELECT $8, 'evidence_submission', s.id, $2, $3, $4, $5, $6, $7, $9, 'pending'
FROM evidence_submissions s
JOIN evidence e ON e.id = s.evidence_id
WHERE s.id = $1
  AND e.workspace_id = $8
RETURNING
    id,
    owner_id AS evidence_submission_id,
    filename,
    content_type,
    content_length,
    object_key,
    checksum_sha256,
    checksum_crc32c,
    created_by_user_id,
    upload_status,
    created_at
"#,
                &[
                    &Uuid::from(evidence_submission_id),
                    &payload.filename,
                    &payload.content_type,
                    &payload.content_length,
                    &payload.object_key,
                    &payload.checksum_sha256,
                    &payload.checksum_crc32c,
                    &Uuid::from(self.workspace_id),
                    &Uuid::from(self.user_id),
                ],
            )
            .await?;

        let Some(row) = rows.into_iter().next() else {
            return Err(Error::InvariantViolation(
                "document insert requires an existing workspace-scoped submission",
            ));
        };
        evidence_document_from_row(&row)
    }

    pub async fn archive_evidence_document(
        &self,
        evidence_submission_id: EvidenceSubmissionId,
        evidence_document_id: DocumentId,
    ) -> Result<ArchiveDocumentResult, Error> {
        let row = self
            .transaction
            .query_opt(
                r#"
WITH scoped AS (
    SELECT a.id, a.upload_status, a.archived
    FROM documents a
    WHERE a.workspace_id = $1
      AND a.owner_type = 'evidence_submission'
      AND a.owner_id = $2
      AND a.id = $3
),
updated AS (
    UPDATE documents a
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
                    &Uuid::from(evidence_document_id),
                ],
            )
            .await?
            .ok_or(Error::InvariantViolation("archive query must return one row"))?;

        match (
            row.try_get::<_, bool>("found")?,
            row.try_get::<_, bool>("archived")?,
        ) {
            (false, _) => Ok(ArchiveDocumentResult::NotFound),
            (true, true) => Ok(ArchiveDocumentResult::Archived),
            (true, false) => Ok(ArchiveDocumentResult::NotTerminal),
        }
    }
}

impl WorkspaceReadContext {
    pub(crate) async fn get_agent_upload_document(
        &self,
        submission_id: EvidenceSubmissionId,
        document_id: DocumentId,
    ) -> Result<Option<Document>, Error> {
        let rows = self
            .client
            .query(
                r#"
SELECT
    a.id AS document_id,
    a.owner_id AS document_submission_id,
    a.filename,
    a.content_type,
    a.content_length,
    a.object_key,
    a.checksum_sha256,
    a.checksum_crc32c,
    a.created_by_user_id,
    a.upload_status,
    a.created_at
FROM documents a
JOIN evidence_submissions s
  ON s.id = a.owner_id
 AND a.owner_type = 'evidence_submission'
JOIN evidence e ON e.id = s.evidence_id
WHERE s.id = $1
  AND a.id = $2
  AND e.workspace_id = $3
  AND a.workspace_id = e.workspace_id
"#,
                &[
                    &Uuid::from(submission_id),
                    &Uuid::from(document_id),
                    &Uuid::from(self.workspace_id),
                ],
            )
            .await?;

        rows.first().map(evidence_document_from_row).transpose()
    }

    pub async fn get_document_for_download_grant(
        &self,
        evidence_submission_id: EvidenceSubmissionId,
        evidence_document_id: DocumentId,
    ) -> Result<Option<DocumentDownloadCandidate>, Error> {
        let rows = &self
            .client
            .query(
                r#"
SELECT
    a.workspace_id,
    a.id AS document_id,
    a.owner_id AS document_submission_id,
    a.filename,
    a.content_type,
    a.content_length,
    a.object_key,
    a.checksum_sha256,
    a.checksum_crc32c,
    a.created_by_user_id,
    a.upload_status,
    a.created_at
FROM documents a
JOIN evidence_submissions s ON s.id = a.owner_id
WHERE a.workspace_id = $1
  AND a.owner_type = 'evidence_submission'
  AND s.id = $2
  AND a.id = $3
  AND a.archived = false
"#,
                &[
                    &Uuid::from(self.workspace_id),
                    &Uuid::from(evidence_submission_id),
                    &Uuid::from(evidence_document_id),
                ],
            )
            .await?;

        rows.iter()
            .next()
            .map(document_download_candidate_from_row)
            .transpose()
    }

    pub async fn get_document_for_download_grant_within_period(
        &self,
        evidence_submission_id: EvidenceSubmissionId,
        evidence_document_id: DocumentId,
        period_start: DateTime<Utc>,
        period_end: DateTime<Utc>,
    ) -> Result<Option<DocumentDownloadCandidate>, Error> {
        let rows = &self
            .client
            .query(
                r#"
SELECT
    a.workspace_id,
    a.id AS document_id,
    a.owner_id AS document_submission_id,
    a.filename,
    a.content_type,
    a.content_length,
    a.object_key,
    a.checksum_sha256,
    a.checksum_crc32c,
    a.created_by_user_id,
    a.upload_status,
    a.created_at
FROM documents a
JOIN evidence_submissions s ON s.id = a.owner_id
WHERE a.workspace_id = $1
  AND a.owner_type = 'evidence_submission'
  AND s.id = $2
  AND a.id = $3
  AND a.archived = false
  AND s.valid_from <= $5
  AND s.valid_until >= $4
"#,
                &[
                    &Uuid::from(self.workspace_id),
                    &Uuid::from(evidence_submission_id),
                    &Uuid::from(evidence_document_id),
                    &period_start,
                    &period_end,
                ],
            )
            .await?;

        rows.iter()
            .next()
            .map(document_download_candidate_from_row)
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
JOIN evidence e ON e.id = s.evidence_id
WHERE s.id = $1
  AND e.workspace_id = $2
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
                &format!(
                    r#"
SELECT{SUBMISSION_DETAIL_COLUMNS}
FROM evidence_submissions s
JOIN evidence e ON e.id = s.evidence_id
LEFT JOIN agent_connections c ON c.id = s.submitted_by_agent_connection_id
JOIN documents a ON a.owner_id = s.id
    AND a.owner_type = 'evidence_submission'
    AND a.workspace_id = e.workspace_id
    AND a.archived = false
WHERE s.id = $1
  AND e.workspace_id = $2
"#
                ),
                &[&Uuid::from(id), &Uuid::from(self.workspace_id)],
            )
            .await?;

        rows.first()
            .map(evidence_submission_detail_from_row)
            .transpose()
    }

    pub async fn list_evidence_submissions(
        &self,
        evidence_id: EvidenceId,
    ) -> Result<Vec<EvidenceSubmissionDetail>, Error> {
        let rows = self
            .client
            .query(
                &format!(
                    r#"
SELECT{SUBMISSION_DETAIL_COLUMNS}
FROM evidence_submissions s
JOIN evidence e ON e.id = s.evidence_id
LEFT JOIN agent_connections c ON c.id = s.submitted_by_agent_connection_id
JOIN documents a ON a.owner_id = s.id
    AND a.owner_type = 'evidence_submission'
    AND a.workspace_id = e.workspace_id
    AND a.archived = false
WHERE s.evidence_id = $1
  AND e.workspace_id = $2
ORDER BY s.received_at DESC, s.id DESC
"#
                ),
                &[&Uuid::from(evidence_id), &Uuid::from(self.workspace_id)],
            )
            .await?;

        rows.iter()
            .map(evidence_submission_detail_from_row)
            .collect()
    }

    pub async fn list_evidence_submissions_for_coverage(
        &self,
        evidence_id: EvidenceId,
        coverage: CoverageWindow,
    ) -> Result<Vec<EvidenceSubmissionDetail>, Error> {
        let rows = self
            .client
            .query(
                &format!(
                    r#"
SELECT{SUBMISSION_DETAIL_COLUMNS}
FROM evidence_submissions s
JOIN evidence e ON e.id = s.evidence_id
LEFT JOIN agent_connections c ON c.id = s.submitted_by_agent_connection_id
JOIN documents a ON a.owner_id = s.id
    AND a.owner_type = 'evidence_submission'
    AND a.workspace_id = e.workspace_id
    AND a.archived = false
WHERE s.evidence_id = $1
  AND e.workspace_id = $2
  AND s.valid_from = $3
  AND s.valid_until = $4
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

        rows.iter()
            .map(evidence_submission_detail_from_row)
            .collect()
    }

    pub async fn latest_evidence_submission_for_evidence(
        &self,
        evidence_id: EvidenceId,
    ) -> Result<Option<EvidenceSubmissionDetail>, Error> {
        let rows = self
            .client
            .query(
                &format!(
                    r#"
WITH latest_submission AS (
    SELECT s.id
    FROM evidence_submissions s
    JOIN evidence e ON e.id = s.evidence_id
    JOIN documents a ON a.owner_id = s.id
        AND a.owner_type = 'evidence_submission'
        AND a.workspace_id = e.workspace_id
        AND a.archived = false
    WHERE s.evidence_id = $1
      AND e.workspace_id = $2
    ORDER BY s.received_at DESC, s.id DESC
    LIMIT 1
)
SELECT{SUBMISSION_DETAIL_COLUMNS}
FROM latest_submission latest
JOIN evidence_submissions s ON s.id = latest.id
JOIN evidence e ON e.id = s.evidence_id
LEFT JOIN agent_connections c ON c.id = s.submitted_by_agent_connection_id
JOIN documents a ON a.owner_id = s.id
    AND a.owner_type = 'evidence_submission'
    AND a.workspace_id = e.workspace_id
    AND a.archived = false
"#
                ),
                &[&Uuid::from(evidence_id), &Uuid::from(self.workspace_id)],
            )
            .await?;

        rows.first()
            .map(evidence_submission_detail_from_row)
            .transpose()
    }
}

fn evidence_submission_detail_from_row(row: &Row) -> Result<EvidenceSubmissionDetail, Error> {
    Ok(EvidenceSubmissionDetail {
        submission: evidence_submission_from_row(row)?,
        document: evidence_document_from_row(row)?,
    })
}

fn evidence_submission_from_row(row: &Row) -> Result<EvidenceSubmission, Error> {
    Ok(EvidenceSubmission {
        id: EvidenceSubmissionId::from(row.try_get::<_, Uuid>("id")?),
        evidence_id: EvidenceId::from(row.try_get::<_, Uuid>("evidence_id")?),
        submitted_by: evidence_submitter_from_row(row)?,
        received_at: row.try_get("received_at")?,
        valid_from: row.try_get("valid_from")?,
        valid_until: row.try_get("valid_until")?,
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

fn evidence_document_from_row(row: &Row) -> Result<Document, Error> {
    let document_id = DocumentId::from(
        row.try_get::<_, Uuid>("document_id")
            .or_else(|_| row.try_get::<_, Uuid>("id"))?,
    );
    let evidence_submission_id = EvidenceSubmissionId::from(
        row.try_get::<_, Uuid>("document_submission_id")
            .or_else(|_| row.try_get::<_, Uuid>("evidence_submission_id"))?,
    );
    document_from_row(
        row,
        DocumentIdentity::Evidence {
            evidence_submission_id,
            document_id,
        },
    )
}

fn pending_document_upload_work_from_row(row: &Row) -> Result<PendingDocumentUploadWork, Error> {
    let upload_status = row
        .try_get::<_, String>("upload_status")?
        .parse::<DocumentUploadStatus>()?;

    Ok(PendingDocumentUploadWork {
        workspace_id: WorkspaceId::from(row.try_get::<_, Uuid>("workspace_id")?),
        evidence_submission_id: EvidenceSubmissionId::from(
            row.try_get::<_, Uuid>("evidence_submission_id")?,
        ),
        evidence_document_id: DocumentId::from(row.try_get::<_, Uuid>("document_id")?),
        filename: row.try_get("filename")?,
        content_type: row.try_get("content_type")?,
        content_length: row.try_get("content_length")?,
        object_key: row.try_get("object_key")?,
        checksum_sha256: row.try_get("checksum_sha256")?,
        upload_status,
    })
}

fn document_download_candidate_from_row(row: &Row) -> Result<DocumentDownloadCandidate, Error> {
    Ok(DocumentDownloadCandidate {
        workspace_id: WorkspaceId::from(row.try_get::<_, Uuid>("workspace_id")?),
        document: evidence_document_from_row(row)?,
    })
}
