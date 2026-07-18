use std::collections::HashMap;

use chrono::{DateTime, Utc};
use tokio_postgres::Row;
use uuid::Uuid;

use crate::{
    domain::{
        AuditorPortalControl, AuditorPortalDocument, AuditorPortalEvidenceRequest,
        AuditorPortalSubmission, Control, ControlId, DocumentUploadStatus, EvidenceRequest,
        EvidenceRequestCadence, EvidenceRequestId, EvidenceRequestStatus, EvidenceSubmission,
        EvidenceSubmissionId, EvidenceSubmitter, WorkspaceId,
    },
    repository::WorkspaceReadContext,
};

use super::Error;

impl WorkspaceReadContext {
    pub async fn auditor_portal_controls(&self) -> Result<Vec<AuditorPortalControl>, Error> {
        let mut controls = self
            .list_controls()
            .await?
            .into_iter()
            .map(AuditorPortalControl::from)
            .collect::<Vec<_>>();
        let control_indices = controls
            .iter()
            .enumerate()
            .map(|(index, control)| (control.id, index))
            .collect::<HashMap<_, _>>();
        let submissions_by_request = self.auditor_portal_submissions_by_request().await?;
        let mappings = self.auditor_portal_request_mappings().await?;

        for mapping in mappings {
            let Some(control) = control_indices
                .get(&mapping.control_id)
                .and_then(|index| controls.get_mut(*index))
            else {
                continue;
            };

            let submissions = submissions_by_request
                .get(&mapping.request.id)
                .cloned()
                .unwrap_or_default();
            control
                .evidence_requests
                .push(AuditorPortalEvidenceRequest {
                    mapping_rationale: mapping.rationale,
                    mapping_created_at: mapping.created_at,
                    request: mapping.request,
                    submissions,
                });
        }

        Ok(controls)
    }

    async fn auditor_portal_request_mappings(
        &self,
    ) -> Result<Vec<AuditorPortalRequestMapping>, Error> {
        let rows = self
            .client
            .query(
                r#"
SELECT
    c.id AS control_id,
    m.rationale AS mapping_rationale,
    m.created_at AS mapping_created_at,
    er.id,
    er.workspace_id,
    er.title,
    er.description,
    er.collection_instructions,
    er.cadence,
    er.due_at,
    er.schedule_anchor_at,
    er.freshness_window_days,
    er.status,
    er.created_at,
    er.updated_at
FROM evidence_request_control_mappings m
JOIN controls c ON c.id = m.control_id
JOIN evidence_requests er ON er.id = m.evidence_request_id
WHERE c.workspace_id = $1
  AND er.workspace_id = $1
ORDER BY c.code, c.id, er.due_at, er.title, er.id
"#,
                &[&Uuid::from(self.workspace_id)],
            )
            .await?;

        rows.into_iter()
            .map(auditor_portal_request_mapping_from_row)
            .collect()
    }

    async fn auditor_portal_submissions_by_request(
        &self,
    ) -> Result<HashMap<EvidenceRequestId, Vec<AuditorPortalSubmission>>, Error> {
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
    a.checksum_sha256,
    a.checksum_crc32c,
    a.created_by_user_id,
    a.upload_status
FROM evidence_submissions s
JOIN evidence_requests er ON er.id = s.evidence_request_id
LEFT JOIN agent_connections c ON c.id = s.submitted_by_agent_connection_id
LEFT JOIN documents a ON a.owner_id = s.id
    AND a.owner_type = 'evidence_submission'
    AND a.workspace_id = er.workspace_id
    AND a.archived = false
WHERE er.workspace_id = $1
ORDER BY s.evidence_request_id, s.received_at DESC, s.id DESC, a.filename, a.id
"#,
                &[&Uuid::from(self.workspace_id)],
            )
            .await?;

        let mut submissions_by_request = HashMap::new();
        let mut current_submission_id = None;
        let mut current_request_id = None;

        for row in rows {
            let submission_id = EvidenceSubmissionId::from(row.try_get::<_, Uuid>("id")?);
            let request_id =
                EvidenceRequestId::from(row.try_get::<_, Uuid>("evidence_request_id")?);

            if current_submission_id != Some(submission_id) {
                submissions_by_request
                    .entry(request_id)
                    .or_insert_with(Vec::new)
                    .push(AuditorPortalSubmission {
                        submission: evidence_submission_from_row(&row)?,
                        documents: Vec::new(),
                    });
                current_submission_id = Some(submission_id);
                current_request_id = Some(request_id);
            }

            let Some(document) = auditor_portal_document_from_optional_row(&row) else {
                continue;
            };
            let Some(request_submissions) =
                current_request_id.and_then(|id| submissions_by_request.get_mut(&id))
            else {
                continue;
            };
            if let Some(submission) = request_submissions.last_mut() {
                submission.documents.push(document?);
            }
        }

        Ok(submissions_by_request)
    }
}

struct AuditorPortalRequestMapping {
    control_id: ControlId,
    rationale: String,
    created_at: DateTime<Utc>,
    request: EvidenceRequest,
}

impl From<Control> for AuditorPortalControl {
    fn from(control: Control) -> Self {
        Self {
            id: control.id,
            code: control.code,
            title: control.title,
            description: control.description,
            framework_requirements: control.framework_requirements,
            evidence_requests: Vec::new(),
        }
    }
}

fn auditor_portal_request_mapping_from_row(row: Row) -> Result<AuditorPortalRequestMapping, Error> {
    Ok(AuditorPortalRequestMapping {
        control_id: ControlId::from(row.try_get::<_, Uuid>("control_id")?),
        rationale: row.try_get("mapping_rationale")?,
        created_at: row.try_get("mapping_created_at")?,
        request: evidence_request_from_row(&row)?,
    })
}

fn evidence_request_from_row(row: &Row) -> Result<EvidenceRequest, Error> {
    Ok(EvidenceRequest {
        id: EvidenceRequestId::from(row.try_get::<_, Uuid>("id")?),
        workspace_id: WorkspaceId::from(row.try_get::<_, Uuid>("workspace_id")?),
        title: row.try_get("title")?,
        description: row.try_get("description")?,
        collection_instructions: row.try_get("collection_instructions")?,
        cadence: row
            .try_get::<_, String>("cadence")?
            .parse::<EvidenceRequestCadence>()?,
        due_at: row.try_get("due_at")?,
        schedule_anchor_at: row.try_get("schedule_anchor_at")?,
        freshness_window_days: row.try_get("freshness_window_days")?,
        status: row
            .try_get::<_, String>("status")?
            .parse::<EvidenceRequestStatus>()?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
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
            "auditor portal submission must have an agent connection submitter",
        )),
    }
}

fn auditor_portal_document_from_optional_row(
    row: &Row,
) -> Option<Result<AuditorPortalDocument, Error>> {
    match row.try_get::<_, Option<Uuid>>("document_id") {
        Ok(Some(_)) => {}
        Ok(None) => return None,
        Err(error) => return Some(Err(Error::Database(error))),
    }

    Some(auditor_portal_document_from_row(row))
}

fn auditor_portal_document_from_row(row: &Row) -> Result<AuditorPortalDocument, Error> {
    let upload_status = row
        .try_get::<_, String>("upload_status")?
        .parse::<DocumentUploadStatus>()?;

    Ok(AuditorPortalDocument {
        id: row.try_get::<_, Uuid>("document_id")?.into(),
        evidence_submission_id: row.try_get::<_, Uuid>("document_submission_id")?.into(),
        created_by_user_id: row.try_get::<_, Uuid>("created_by_user_id")?.into(),
        filename: row.try_get("filename")?,
        content_type: row.try_get("content_type")?,
        content_length: row.try_get("content_length")?,
        checksum_sha256: row.try_get("checksum_sha256")?,
        checksum_crc32c: row.try_get("checksum_crc32c")?,
        upload_status,
        download_eligible: upload_status == DocumentUploadStatus::Uploaded,
    })
}
