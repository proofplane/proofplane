use chrono::{DateTime, Utc};
use deadpool_postgres::GenericClient;
use tokio_postgres::Row;
use uuid::Uuid;

use crate::{
    domain::{
        CoverageWindow, Document, DocumentId, DocumentIdentity, EvidenceId, EvidenceSubmission,
        EvidenceSubmissionId, EvidenceSubmitter, WorkspaceId,
    },
    projections::{DocumentDownloadCandidate, EvidenceSubmissionDetail},
    repository::{WorkspaceClient, WorkspaceRepositories},
};

use super::{documents::document_from_row, Error};

/// Complete-snapshot repository for the submission provenance aggregate.
pub struct EvidenceSubmissionRepository<'a> {
    context: &'a WorkspaceRepositories<'a>,
}

impl<'a> WorkspaceRepositories<'a> {
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

impl WorkspaceClient {
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

    pub(super) async fn load_evidence_submission_detail(
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

    pub(super) async fn load_evidence_submission_details(
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

    pub(super) async fn load_evidence_submission_details_for_coverage(
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

    pub(super) async fn load_latest_evidence_submission_detail(
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

fn document_download_candidate_from_row(row: &Row) -> Result<DocumentDownloadCandidate, Error> {
    Ok(DocumentDownloadCandidate {
        workspace_id: WorkspaceId::from(row.try_get::<_, Uuid>("workspace_id")?),
        document: evidence_document_from_row(row)?,
    })
}
