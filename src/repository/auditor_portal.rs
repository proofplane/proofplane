use std::collections::HashMap;

use chrono::{DateTime, Utc};
use tokio_postgres::Row;
use uuid::Uuid;

use crate::{
    domain::{
        AuditorPortalControl, AuditorPortalEvidence, AuditorPortalSubmission, Control, ControlId,
        Evidence, EvidenceId, EvidenceStatus, EvidenceSubmission, EvidenceSubmissionId,
        EvidenceSubmitter, SubmissionUploadStatus, WorkspaceId,
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
        let submissions_by_evidence = self.auditor_portal_submissions_by_evidence().await?;
        let mappings = self.auditor_portal_evidence_mappings().await?;

        for mapping in mappings {
            let Some(control) = control_indices
                .get(&mapping.control_id)
                .and_then(|index| controls.get_mut(*index))
            else {
                continue;
            };

            let submissions = submissions_by_evidence
                .get(&mapping.evidence.id)
                .cloned()
                .unwrap_or_default();
            control.evidence.push(AuditorPortalEvidence {
                mapping_rationale: mapping.rationale,
                mapping_created_at: mapping.created_at,
                evidence: mapping.evidence,
                submissions,
            });
        }

        Ok(controls)
    }

    async fn auditor_portal_evidence_mappings(
        &self,
    ) -> Result<Vec<AuditorPortalEvidenceMapping>, Error> {
        let rows = self
            .client
            .query(
                r#"
SELECT
    c.id AS control_id,
    m.rationale AS mapping_rationale,
    m.created_at AS mapping_created_at,
    e.id,
    e.workspace_id,
    e.title,
    e.description,
    e.collection_instructions,
    e.status,
    e.created_at,
    e.updated_at
FROM evidence_control_mappings m
JOIN controls c ON c.id = m.control_id
JOIN evidence e ON e.id = m.evidence_id
WHERE c.workspace_id = $1
  AND e.workspace_id = $1
ORDER BY c.code, c.id, e.title, e.id
"#,
                &[&Uuid::from(self.workspace_id)],
            )
            .await?;

        rows.into_iter()
            .map(auditor_portal_evidence_mapping_from_row)
            .collect()
    }

    async fn auditor_portal_submissions_by_evidence(
        &self,
    ) -> Result<HashMap<EvidenceId, Vec<AuditorPortalSubmission>>, Error> {
        let rows = self
            .client
            .query(
                r#"
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
FROM evidence_submissions s
JOIN evidence e ON e.id = s.evidence_id
LEFT JOIN agent_connections c ON c.id = s.submitted_by_agent_connection_id
WHERE e.workspace_id = $1
  AND s.archived = false
ORDER BY s.evidence_id, s.received_at DESC, s.id DESC
"#,
                &[&Uuid::from(self.workspace_id)],
            )
            .await?;

        let mut submissions_by_evidence: HashMap<EvidenceId, Vec<AuditorPortalSubmission>> =
            HashMap::new();

        for row in rows {
            let evidence_id = EvidenceId::from(row.try_get::<_, Uuid>("evidence_id")?);
            let submission = evidence_submission_from_row(&row)?;
            let download_eligible = submission.upload_status == SubmissionUploadStatus::Uploaded;

            submissions_by_evidence
                .entry(evidence_id)
                .or_default()
                .push(AuditorPortalSubmission {
                    submission,
                    download_eligible,
                });
        }

        Ok(submissions_by_evidence)
    }
}

struct AuditorPortalEvidenceMapping {
    control_id: ControlId,
    rationale: String,
    created_at: DateTime<Utc>,
    evidence: Evidence,
}

impl From<Control> for AuditorPortalControl {
    fn from(control: Control) -> Self {
        Self {
            id: control.id,
            code: control.code,
            title: control.title,
            description: control.description,
            framework_requirements: control.framework_requirements,
            evidence: Vec::new(),
        }
    }
}

fn auditor_portal_evidence_mapping_from_row(
    row: Row,
) -> Result<AuditorPortalEvidenceMapping, Error> {
    Ok(AuditorPortalEvidenceMapping {
        control_id: ControlId::from(row.try_get::<_, Uuid>("control_id")?),
        rationale: row.try_get("mapping_rationale")?,
        created_at: row.try_get("mapping_created_at")?,
        evidence: evidence_from_row(&row)?,
    })
}

fn evidence_from_row(row: &Row) -> Result<Evidence, Error> {
    Ok(Evidence {
        id: EvidenceId::from(row.try_get::<_, Uuid>("id")?),
        workspace_id: WorkspaceId::from(row.try_get::<_, Uuid>("workspace_id")?),
        title: row.try_get("title")?,
        description: row.try_get("description")?,
        collection_instructions: row.try_get("collection_instructions")?,
        status: row
            .try_get::<_, String>("status")?
            .parse::<EvidenceStatus>()?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
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
            "auditor portal submission must have an agent connection submitter",
        )),
    }
}
