use std::{collections::HashMap, str::FromStr};

use chrono::{DateTime, Utc};
use tokio_postgres::Row;
use uuid::Uuid;

use crate::{
    domain::{
        AuditorPortalControl, AuditorPortalDocument, AuditorPortalEvidence, AuditorPortalPolicy,
        AuditorPortalPolicyDocument, AuditorPortalSubmission, Control, ControlId, ControlSummary,
        DocumentUploadStatus, Evidence, EvidenceId, EvidenceStatus, EvidenceSubmission,
        EvidenceSubmissionId, EvidenceSubmitter, PolicyId, WorkspaceId,
    },
    repository::WorkspaceReadContext,
};

use super::Error;

impl WorkspaceReadContext {
    pub async fn auditor_portal_policies(&self) -> Result<Vec<AuditorPortalPolicy>, Error> {
        let rows = self
            .client
            .query(
                r#"
SELECT
    p.id AS policy_id,
    p.name AS policy_name,
    p.description AS policy_description,
    p.created_at AS policy_created_at,
    p.updated_at AS policy_updated_at,
    d.id AS document_id,
    d.created_by_user_id AS document_created_by_user_id,
    d.filename AS document_filename,
    d.content_type AS document_content_type,
    d.content_length AS document_content_length,
    d.checksum_sha256 AS document_checksum_sha256,
    d.checksum_crc32c AS document_checksum_crc32c,
    d.upload_status AS document_upload_status,
    d.created_at AS document_created_at,
    c.id AS control_id,
    c.code AS control_code,
    c.title AS control_title,
    c.description AS control_description
FROM policies p
LEFT JOIN documents d ON d.owner_id = p.id
    AND d.owner_type = 'policy'
    AND d.workspace_id = p.workspace_id
    AND d.archived = false
LEFT JOIN policy_control_mappings m ON m.policy_id = p.id
LEFT JOIN controls c ON c.id = m.control_id
    AND c.workspace_id = p.workspace_id
WHERE p.workspace_id = $1
  AND p.archived_at IS NULL
ORDER BY lower(p.name), p.id, lower(c.code), c.id
"#,
                &[&Uuid::from(self.workspace_id)],
            )
            .await?;

        auditor_portal_policies_from_rows(rows)
    }

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
    a.id AS document_id,
    a.filename,
    a.upload_status
FROM evidence_submissions s
JOIN evidence e ON e.id = s.evidence_id
LEFT JOIN agent_connections c ON c.id = s.submitted_by_agent_connection_id
JOIN documents a ON a.owner_id = s.id
    AND a.owner_type = 'evidence_submission'
    AND a.workspace_id = e.workspace_id
    AND a.archived = false
WHERE e.workspace_id = $1
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
            let document = auditor_portal_document_from_row(&row)?;
            submissions_by_evidence
                .entry(evidence_id)
                .or_default()
                .push(AuditorPortalSubmission {
                    submission,
                    document,
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
            policies: Vec::new(),
        }
    }
}

fn auditor_portal_policies_from_rows(rows: Vec<Row>) -> Result<Vec<AuditorPortalPolicy>, Error> {
    let mut policies = Vec::new();
    let mut current_policy_id = None;

    for row in rows {
        let policy_id = PolicyId::from(row.try_get::<_, Uuid>("policy_id")?);
        if current_policy_id != Some(policy_id) {
            policies.push(AuditorPortalPolicy {
                id: policy_id,
                name: row.try_get("policy_name")?,
                description: row.try_get("policy_description")?,
                controls: Vec::new(),
                document: auditor_portal_policy_document_from_row(&row, policy_id)?,
                created_at: row.try_get("policy_created_at")?,
                updated_at: row.try_get("policy_updated_at")?,
            });
            current_policy_id = Some(policy_id);
        }

        let Some(control_id) = row.try_get::<_, Option<Uuid>>("control_id")? else {
            continue;
        };
        if let Some(policy) = policies.last_mut() {
            policy.controls.push(ControlSummary {
                id: control_id.into(),
                code: row.try_get("control_code")?,
                title: row.try_get("control_title")?,
                description: row.try_get("control_description")?,
            });
        }
    }

    Ok(policies)
}

fn auditor_portal_policy_document_from_row(
    row: &Row,
    policy_id: PolicyId,
) -> Result<Option<AuditorPortalPolicyDocument>, Error> {
    let Some(document_id) = row.try_get::<_, Option<Uuid>>("document_id")? else {
        return Ok(None);
    };
    let upload_status =
        DocumentUploadStatus::from_str(&row.try_get::<_, String>("document_upload_status")?)?;

    Ok(Some(AuditorPortalPolicyDocument {
        id: document_id.into(),
        policy_id,
        created_by_user_id: row
            .try_get::<_, Uuid>("document_created_by_user_id")?
            .into(),
        filename: row.try_get("document_filename")?,
        content_type: row.try_get("document_content_type")?,
        content_length: row.try_get("document_content_length")?,
        checksum_sha256: row.try_get("document_checksum_sha256")?,
        checksum_crc32c: row.try_get("document_checksum_crc32c")?,
        upload_status,
        created_at: row.try_get("document_created_at")?,
        download_eligible: upload_status == DocumentUploadStatus::Uploaded,
    }))
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

fn auditor_portal_document_from_row(row: &Row) -> Result<AuditorPortalDocument, Error> {
    let upload_status = row
        .try_get::<_, String>("upload_status")?
        .parse::<DocumentUploadStatus>()?;

    Ok(AuditorPortalDocument {
        document_id: row.try_get::<_, Uuid>("document_id")?.into(),
        filename: row.try_get("filename")?,
        download_eligible: upload_status == DocumentUploadStatus::Uploaded,
    })
}
