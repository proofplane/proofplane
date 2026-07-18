use deadpool_postgres::GenericClient;
use tokio_postgres::Row;
use uuid::Uuid;

use crate::{
    domain::{
        CreateDocumentPayload, CreateEvidenceSubmissionPayload, Document, DocumentId,
        DocumentIdentity, DocumentOwner, DocumentUploadStatus, EvidenceRequestId,
        EvidenceSubmission, EvidenceSubmissionDetail, EvidenceSubmissionId, EvidenceSubmitter,
        WorkspaceId,
    },
    repository::{WorkspaceReadContext, WorkspaceTransactionContext},
};

use super::{documents::document_from_row, Error, Postgres, TransactionContext};

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
	        evidence_request_id,
	        submitted_by_agent_connection_id,
	        coverage_start_at,
	        coverage_end_at,
	        source_system,
        collection_method,
	        summary,
	        description
	    )
	    SELECT er.id, $2, $3, $4, $5, $6, $7, $8
	    FROM evidence_requests er
	    WHERE er.id = $1
	      AND er.workspace_id = $9
	    RETURNING
	        id,
	        evidence_request_id,
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
	    inserted.submitted_by_agent_connection_id,
	    c.user_id AS submitted_by_user_id,
	    inserted.received_at,
    inserted.coverage_start_at,
    inserted.coverage_end_at,
    inserted.source_system,
    inserted.collection_method,
	    inserted.summary,
	    inserted.description
	FROM inserted
	LEFT JOIN agent_connections c ON c.id = inserted.submitted_by_agent_connection_id
	"#,
                &[
                    &Uuid::from(payload.evidence_request_id),
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
	    s.submitted_by_agent_connection_id,
	    c.user_id AS submitted_by_user_id,
    s.received_at,
    s.coverage_start_at,
    s.coverage_end_at,
    s.source_system,
    s.collection_method,
    s.summary,
    s.description,
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
	FROM evidence_submissions s
	JOIN evidence_requests er ON er.id = s.evidence_request_id
	LEFT JOIN agent_connections c ON c.id = s.submitted_by_agent_connection_id
LEFT JOIN documents a ON a.owner_id = s.id
    AND a.owner_type = 'evidence_submission'
    AND a.workspace_id = er.workspace_id
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
	    s.submitted_by_agent_connection_id,
	    c.user_id AS submitted_by_user_id,
    s.received_at,
    s.coverage_start_at,
    s.coverage_end_at,
    s.source_system,
    s.collection_method,
    s.summary,
    s.description,
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
	FROM latest_submission latest
	JOIN evidence_submissions s ON s.id = latest.id
	LEFT JOIN agent_connections c ON c.id = s.submitted_by_agent_connection_id
LEFT JOIN documents a ON a.owner_id = s.id
    AND a.owner_type = 'evidence_submission'
    AND a.workspace_id = $2
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
JOIN evidence_requests er ON er.id = s.evidence_request_id
WHERE s.id = $1
  AND er.workspace_id = $8
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

    pub async fn create_first_evidence_document(
        &self,
        payload: &CreateDocumentPayload,
    ) -> Result<Option<Document>, Error> {
        let DocumentOwner::EvidenceSubmission(evidence_submission_id) = payload.owner else {
            return Err(Error::InvariantViolation(
                "evidence document creation requires an evidence submission owner",
            ));
        };
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
                    &Uuid::from(evidence_submission_id),
                    &Uuid::from(self.workspace_id),
                ],
            )
            .await?;

        if locked_submission.is_none() {
            return Err(Error::InvariantViolation(
                "document insert requires an existing workspace-scoped submission",
            ));
        }

        let row = self
            .transaction
            .query_opt(
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
SELECT $8, 'evidence_submission', $1, $2, $3, $4, $5, $6, $7, $9, 'pending'
WHERE NOT EXISTS (
    SELECT 1
    FROM documents
    WHERE owner_type = 'evidence_submission'
      AND owner_id = $1
)
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

        row.map(|row| evidence_document_from_row(&row)).transpose()
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

fn evidence_submission_detail_from_rows(
    rows: Vec<Row>,
) -> Result<Option<EvidenceSubmissionDetail>, Error> {
    let Some(first_row) = rows.first() else {
        return Ok(None);
    };

    let submission = evidence_submission_from_row(first_row)?;
    let documents = rows
        .iter()
        .filter_map(evidence_document_from_optional_row)
        .collect::<Result<Vec<_>, _>>()?;

    Ok(Some(EvidenceSubmissionDetail {
        submission,
        documents,
    }))
}

fn evidence_document_from_optional_row(row: &Row) -> Option<Result<Document, Error>> {
    match row.try_get::<_, Option<Uuid>>("document_id") {
        Ok(Some(_)) => {}
        Ok(None) => return None,
        Err(error) => return Some(Err(Error::Database(error))),
    }

    Some(evidence_document_from_row(row))
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
