use chrono::{DateTime, Utc};
use tokio_postgres::Row;
use uuid::Uuid;

use crate::{
    domain::{
        CoverageWindow, Document, DocumentId, DocumentIdentity, EvidenceId, EvidenceSubmission,
        EvidenceSubmissionId, EvidenceSubmitter, WorkspaceId,
    },
    persistence::Error,
    read_models::{DocumentDownloadCandidate, EvidenceSubmissionDetail},
};

use super::{documents::document_from_row, param, ReadExecutor, TransactionalReadExecutor};

pub(crate) struct EvidenceSubmissionReads<'a, E> {
    executor: &'a E,
    workspace_id: WorkspaceId,
}
impl<'a, E> EvidenceSubmissionReads<'a, E> {
    pub(crate) fn new(executor: &'a E, workspace_id: WorkspaceId) -> Self {
        Self {
            executor,
            workspace_id,
        }
    }
}

impl<E: ReadExecutor> EvidenceSubmissionReads<'_, E> {
    pub async fn get_agent_upload_document(
        &self,
        submission_id: EvidenceSubmissionId,
        document_id: DocumentId,
    ) -> Result<Option<Document>, Error> {
        self.executor
            .query_opt(
                DOCUMENT_SQL,
                &[
                    param(&Uuid::from(submission_id)),
                    param(&Uuid::from(document_id)),
                    param(&Uuid::from(self.workspace_id)),
                ],
            )
            .await?
            .as_ref()
            .map(evidence_document_from_row)
            .transpose()
    }
    pub async fn get_document_for_download(
        &self,
        submission_id: EvidenceSubmissionId,
        document_id: DocumentId,
    ) -> Result<Option<DocumentDownloadCandidate>, Error> {
        self.download(submission_id, document_id, None).await
    }
    pub async fn get_document_for_download_within_period(
        &self,
        submission_id: EvidenceSubmissionId,
        document_id: DocumentId,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<Option<DocumentDownloadCandidate>, Error> {
        self.download(submission_id, document_id, Some((start, end)))
            .await
    }
    async fn download(
        &self,
        submission_id: EvidenceSubmissionId,
        document_id: DocumentId,
        period: Option<(DateTime<Utc>, DateTime<Utc>)>,
    ) -> Result<Option<DocumentDownloadCandidate>, Error> {
        let sql = format!(
            "{DOWNLOAD_SQL}{}",
            if period.is_some() {
                " AND s.valid_from <= $5 AND s.valid_until >= $4"
            } else {
                ""
            }
        );
        let row = match period {
            Some((start, end)) => {
                self.executor
                    .query_opt(
                        &sql,
                        &[
                            param(&Uuid::from(self.workspace_id)),
                            param(&Uuid::from(submission_id)),
                            param(&Uuid::from(document_id)),
                            param(&start),
                            param(&end),
                        ],
                    )
                    .await?
            }
            None => {
                self.executor
                    .query_opt(
                        &sql,
                        &[
                            param(&Uuid::from(self.workspace_id)),
                            param(&Uuid::from(submission_id)),
                            param(&Uuid::from(document_id)),
                        ],
                    )
                    .await?
            }
        };
        row.as_ref()
            .map(document_download_candidate_from_row)
            .transpose()
    }
    pub async fn get(
        &self,
        id: EvidenceSubmissionId,
    ) -> Result<Option<EvidenceSubmissionDetail>, Error> {
        let sql = format!("SELECT{SUBMISSION_DETAIL_COLUMNS} {DETAIL_FROM} WHERE s.id = $1 AND e.workspace_id = $2");
        self.executor
            .query_opt(
                &sql,
                &[
                    param(&Uuid::from(id)),
                    param(&Uuid::from(self.workspace_id)),
                ],
            )
            .await?
            .as_ref()
            .map(evidence_submission_detail_from_row)
            .transpose()
    }
    pub async fn list_for_evidence(
        &self,
        id: EvidenceId,
    ) -> Result<Vec<EvidenceSubmissionDetail>, Error> {
        self.list(id, None).await
    }
    pub async fn list_for_coverage(
        &self,
        id: EvidenceId,
        coverage: CoverageWindow,
    ) -> Result<Vec<EvidenceSubmissionDetail>, Error> {
        self.list(id, Some(coverage)).await
    }
    async fn list(
        &self,
        id: EvidenceId,
        coverage: Option<CoverageWindow>,
    ) -> Result<Vec<EvidenceSubmissionDetail>, Error> {
        let sql = format!("SELECT{SUBMISSION_DETAIL_COLUMNS} {DETAIL_FROM} WHERE s.evidence_id = $1 AND e.workspace_id = $2{} ORDER BY s.received_at DESC, s.id DESC", if coverage.is_some() { " AND s.valid_from = $3 AND s.valid_until = $4" } else { "" });
        let rows = match coverage {
            Some(c) => {
                self.executor
                    .query(
                        &sql,
                        &[
                            param(&Uuid::from(id)),
                            param(&Uuid::from(self.workspace_id)),
                            param(&c.valid_from),
                            param(&c.valid_until),
                        ],
                    )
                    .await?
            }
            None => {
                self.executor
                    .query(
                        &sql,
                        &[
                            param(&Uuid::from(id)),
                            param(&Uuid::from(self.workspace_id)),
                        ],
                    )
                    .await?
            }
        };
        rows.iter()
            .map(evidence_submission_detail_from_row)
            .collect()
    }
    pub async fn latest_for_evidence(
        &self,
        id: EvidenceId,
    ) -> Result<Option<EvidenceSubmissionDetail>, Error> {
        let rows = self.list_for_evidence(id).await?;
        Ok(rows.into_iter().next())
    }
}

impl EvidenceSubmissionReads<'_, TransactionalReadExecutor<'_>> {
    pub async fn exists(&self, id: EvidenceSubmissionId) -> Result<bool, Error> {
        Ok(self.executor.query_opt("SELECT 1 FROM evidence_submissions s JOIN evidence e ON e.id = s.evidence_id WHERE s.id = $1 AND e.workspace_id = $2 FOR KEY SHARE OF s", &[param(&Uuid::from(id)), param(&Uuid::from(self.workspace_id))]).await?.is_some())
    }
}

const DETAIL_FROM: &str = "FROM evidence_submissions s JOIN evidence e ON e.id = s.evidence_id LEFT JOIN agent_connections c ON c.id = s.submitted_by_agent_connection_id JOIN documents a ON a.owner_id = s.id AND a.owner_type = 'evidence_submission' AND a.workspace_id = e.workspace_id AND a.archived = false";
const DOCUMENT_SQL: &str = "SELECT a.workspace_id, a.id AS document_id, a.owner_id AS document_submission_id, a.filename, a.content_type, a.content_length, a.object_key, a.checksum_sha256, a.checksum_crc32c, a.created_by_user_id, a.upload_status, a.created_at FROM documents a JOIN evidence_submissions s ON s.id = a.owner_id AND a.owner_type = 'evidence_submission' JOIN evidence e ON e.id = s.evidence_id WHERE s.id = $1 AND a.id = $2 AND e.workspace_id = $3 AND a.workspace_id = e.workspace_id";
const DOWNLOAD_SQL: &str = "SELECT a.workspace_id, a.id AS document_id, a.owner_id AS document_submission_id, a.filename, a.content_type, a.content_length, a.object_key, a.checksum_sha256, a.checksum_crc32c, a.created_by_user_id, a.upload_status, a.created_at FROM documents a JOIN evidence_submissions s ON s.id = a.owner_id WHERE a.workspace_id = $1 AND a.owner_type = 'evidence_submission' AND s.id = $2 AND a.id = $3 AND a.archived = false";

const SUBMISSION_DETAIL_COLUMNS: &str = r#"
    s.id,
    s.evidence_id,
    s.submitted_by_agent_connection_id,
    c.user_id AS submitted_by_user_id,
    s.received_at,
    s.valid_from,
    s.valid_until,
    a.id AS document_id,
    a.workspace_id,
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
