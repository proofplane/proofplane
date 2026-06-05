use tokio_postgres::Row;
use uuid::Uuid;

use crate::{
    domain::{
        ActorId, AttachmentScanStatus, CreateEvidenceAttachmentPayload,
        CreateEvidenceSubmissionPayload, EvidenceAttachment, EvidenceAttachmentId,
        EvidenceAttachmentScan, EvidenceAttachmentWithScan, EvidenceRequestId, EvidenceSubmission,
        EvidenceSubmissionDetail, EvidenceSubmissionId,
    },
    services::{ReadServiceContext, ServiceContext},
};

use super::Error;

impl ServiceContext<'_> {
    pub async fn create_evidence_submission(
        &self,
        payload: &CreateEvidenceSubmissionPayload,
    ) -> Result<Option<EvidenceSubmission>, Error> {
        let rows = self
            .transaction
            .query(
                r#"
INSERT INTO evidence_submissions (
    evidence_request_id,
    submitted_by,
    coverage_start_at,
    coverage_end_at,
    source_system,
    collection_method
)
SELECT er.id, $2, $3, $4, $5, $6
FROM evidence_requests er
WHERE er.id = $1
  AND er.workspace_id = $7
RETURNING
    id,
    evidence_request_id,
    submitted_by,
    received_at,
    coverage_start_at,
    coverage_end_at,
    source_system,
    collection_method
"#,
                &[
                    &Uuid::from(payload.evidence_request_id),
                    &Uuid::from(self.actor_id),
                    &payload.coverage_start_at,
                    &payload.coverage_end_at,
                    &payload.source_system,
                    &payload.collection_method,
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

impl ReadServiceContext {
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
    s.submitted_by,
    s.received_at,
    s.coverage_start_at,
    s.coverage_end_at,
    s.source_system,
    s.collection_method,
    a.id AS attachment_id,
    a.evidence_submission_id AS attachment_submission_id,
    a.filename,
    a.content_type,
    a.content_length,
    a.object_key,
    a.checksum_sha256,
    a.checksum_crc32c,
    scan.evidence_attachment_id AS scan_attachment_id,
    scan.scan_status,
    scan.scanner_name,
    scan.scanner_version,
    scan.scanned_at,
    scan.scan_failure_reason,
    scan.updated_at AS scan_updated_at
FROM evidence_submissions s
JOIN evidence_requests er ON er.id = s.evidence_request_id
LEFT JOIN evidence_attachments a ON a.evidence_submission_id = s.id
LEFT JOIN evidence_attachment_scans scan ON scan.evidence_attachment_id = a.id
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
    s.submitted_by,
    s.received_at,
    s.coverage_start_at,
    s.coverage_end_at,
    s.source_system,
    s.collection_method,
    a.id AS attachment_id,
    a.evidence_submission_id AS attachment_submission_id,
    a.filename,
    a.content_type,
    a.content_length,
    a.object_key,
    a.checksum_sha256,
    a.checksum_crc32c,
    scan.evidence_attachment_id AS scan_attachment_id,
    scan.scan_status,
    scan.scanner_name,
    scan.scanner_version,
    scan.scanned_at,
    scan.scan_failure_reason,
    scan.updated_at AS scan_updated_at
FROM latest_submission latest
JOIN evidence_submissions s ON s.id = latest.id
LEFT JOIN evidence_attachments a ON a.evidence_submission_id = s.id
LEFT JOIN evidence_attachment_scans scan ON scan.evidence_attachment_id = a.id
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

impl ServiceContext<'_> {
    pub async fn create_evidence_attachment(
        &self,
        payload: &CreateEvidenceAttachmentPayload,
    ) -> Result<EvidenceAttachmentWithScan, Error> {
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
    checksum_crc32c
)
SELECT s.id, $2, $3, $4, $5, $6, $7
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
    checksum_crc32c
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
        let attachment = evidence_attachment_from_row(&row)?;
        let scan = self.create_pending_attachment_scan(attachment.id).await?;

        Ok(EvidenceAttachmentWithScan { attachment, scan })
    }

    async fn create_pending_attachment_scan(
        &self,
        evidence_attachment_id: EvidenceAttachmentId,
    ) -> Result<EvidenceAttachmentScan, Error> {
        let rows = self
            .transaction
            .query(
                r#"
INSERT INTO evidence_attachment_scans (evidence_attachment_id, scan_status)
VALUES ($1, 'pending')
RETURNING
    evidence_attachment_id AS scan_attachment_id,
    scan_status,
    scanner_name,
    scanner_version,
    scanned_at,
    scan_failure_reason,
    updated_at AS scan_updated_at
"#,
                &[&Uuid::from(evidence_attachment_id)],
            )
            .await?;

        let Some(row) = rows.into_iter().next() else {
            return Err(Error::InvariantViolation(
                "pending attachment scan insert returned no row",
            ));
        };

        evidence_attachment_scan_from_row(&row)
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
        .filter_map(evidence_attachment_with_scan_from_row)
        .collect::<Result<Vec<_>, _>>()?;

    Ok(Some(EvidenceSubmissionDetail {
        submission,
        attachments,
    }))
}

fn evidence_attachment_with_scan_from_row(
    row: &Row,
) -> Option<Result<EvidenceAttachmentWithScan, Error>> {
    match row.try_get::<_, Option<Uuid>>("attachment_id") {
        Ok(Some(_)) => {}
        Ok(None) => return None,
        Err(error) => return Some(Err(Error::Database(error))),
    }

    let attachment = evidence_attachment_from_row(row);
    let scan = evidence_attachment_scan_from_row(row);

    Some(match (attachment, scan) {
        (Ok(attachment), Ok(scan)) => Ok(EvidenceAttachmentWithScan { attachment, scan }),
        (Err(error), _) | (_, Err(error)) => Err(error),
    })
}

fn evidence_submission_from_row(row: &Row) -> Result<EvidenceSubmission, Error> {
    Ok(EvidenceSubmission {
        id: EvidenceSubmissionId::from(row.try_get::<_, Uuid>("id")?),
        evidence_request_id: EvidenceRequestId::from(
            row.try_get::<_, Uuid>("evidence_request_id")?,
        ),
        submitted_by: ActorId::from(row.try_get::<_, Uuid>("submitted_by")?),
        received_at: row.try_get("received_at")?,
        coverage_start_at: row.try_get("coverage_start_at")?,
        coverage_end_at: row.try_get("coverage_end_at")?,
        source_system: row.try_get("source_system")?,
        collection_method: row.try_get("collection_method")?,
    })
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
    })
}

fn evidence_attachment_scan_from_row(row: &Row) -> Result<EvidenceAttachmentScan, Error> {
    let scan_status = row
        .try_get::<_, String>("scan_status")?
        .parse::<AttachmentScanStatus>()?;

    Ok(EvidenceAttachmentScan {
        evidence_attachment_id: EvidenceAttachmentId::from(
            row.try_get::<_, Uuid>("scan_attachment_id")?,
        ),
        scan_status,
        scanner_name: row.try_get("scanner_name")?,
        scanner_version: row.try_get("scanner_version")?,
        scanned_at: row.try_get("scanned_at")?,
        scan_failure_reason: row.try_get("scan_failure_reason")?,
        updated_at: row.try_get("scan_updated_at")?,
    })
}
