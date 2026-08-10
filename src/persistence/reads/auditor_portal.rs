use std::{collections::HashMap, str::FromStr};

use chrono::{DateTime, Utc};
use tokio_postgres::Row;
use uuid::Uuid;

use crate::{
    domain::{
        ControlId, DocumentUploadStatus, EvidenceId, EvidenceStatus, EvidenceSubmission,
        EvidenceSubmissionId, EvidenceSubmitter, PolicyId, WorkspaceId,
    },
    persistence::Error,
    read_models::{
        AuditorPortalControl, AuditorPortalDocument, AuditorPortalEvidence, AuditorPortalPolicy,
        AuditorPortalPolicyDocument, AuditorPortalSubmission, ControlSummary, EvidenceDetail,
    },
};

use super::{ControlReads, ReadExecutor};

pub(crate) struct AuditorPortalReads<'a, E> {
    executor: &'a E,
    workspace_id: WorkspaceId,
}
impl<'a, E> AuditorPortalReads<'a, E> {
    pub(crate) fn new(executor: &'a E, workspace_id: WorkspaceId) -> Self {
        Self {
            executor,
            workspace_id,
        }
    }
}

impl<E: ReadExecutor> AuditorPortalReads<'_, E> {
    pub async fn policies(&self) -> Result<Vec<AuditorPortalPolicy>, Error> {
        policies_from_rows(
            self.executor
                .query(POLICIES_SQL, &[&Uuid::from(self.workspace_id)])
                .await?,
        )
    }
    pub async fn controls(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<Vec<AuditorPortalControl>, Error> {
        let mut controls = ControlReads::new(self.executor, self.workspace_id)
            .list()
            .await?
            .into_iter()
            .map(|control| AuditorPortalControl {
                id: control.id,
                code: control.code,
                title: control.title,
                description: control.description,
                framework_requirements: control.framework_requirements,
                evidence: Vec::new(),
                policies: Vec::new(),
            })
            .collect::<Vec<_>>();
        let indices = controls
            .iter()
            .enumerate()
            .map(|(i, c)| (c.id, i))
            .collect::<HashMap<_, _>>();
        let submissions = self.submissions(start, end).await?;
        for mapping in self.mappings().await? {
            let Some(control) = indices
                .get(&mapping.control_id)
                .and_then(|i| controls.get_mut(*i))
            else {
                continue;
            };
            control.evidence.push(AuditorPortalEvidence {
                mapping_rationale: mapping.rationale,
                mapping_created_at: mapping.created_at,
                submissions: submissions
                    .get(&mapping.evidence.id)
                    .cloned()
                    .unwrap_or_default(),
                evidence: mapping.evidence,
            });
        }
        Ok(controls)
    }
    async fn mappings(&self) -> Result<Vec<Mapping>, Error> {
        self.executor
            .query(MAPPINGS_SQL, &[&Uuid::from(self.workspace_id)])
            .await?
            .into_iter()
            .map(mapping_from_row)
            .collect()
    }
    async fn submissions(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<HashMap<EvidenceId, Vec<AuditorPortalSubmission>>, Error> {
        let mut result = HashMap::new();
        for row in self
            .executor
            .query(
                SUBMISSIONS_SQL,
                &[&Uuid::from(self.workspace_id), &start, &end],
            )
            .await?
        {
            let evidence_id = row.try_get::<_, Uuid>("evidence_id")?.into();
            result
                .entry(evidence_id)
                .or_insert_with(Vec::new)
                .push(AuditorPortalSubmission {
                    submission: submission_from_row(&row)?,
                    document: AuditorPortalDocument {
                        document_id: row.try_get::<_, Uuid>("document_id")?.into(),
                        filename: row.try_get("filename")?,
                        download_eligible: row
                            .try_get::<_, String>("upload_status")?
                            .parse::<DocumentUploadStatus>()?
                            == DocumentUploadStatus::Uploaded,
                    },
                });
        }
        Ok(result)
    }
}

struct Mapping {
    control_id: ControlId,
    rationale: String,
    created_at: DateTime<Utc>,
    evidence: EvidenceDetail,
}
const POLICIES_SQL: &str = "SELECT p.id AS policy_id, p.name AS policy_name, p.description AS policy_description, p.created_at AS policy_created_at, p.updated_at AS policy_updated_at, d.id AS document_id, d.created_by_user_id AS document_created_by_user_id, d.filename AS document_filename, d.content_type AS document_content_type, d.content_length AS document_content_length, d.checksum_sha256 AS document_checksum_sha256, d.checksum_crc32c AS document_checksum_crc32c, d.upload_status AS document_upload_status, d.created_at AS document_created_at, c.id AS control_id, c.code AS control_code, c.title AS control_title, c.description AS control_description FROM policies p LEFT JOIN documents d ON d.owner_id = p.id AND d.owner_type = 'policy' AND d.workspace_id = p.workspace_id AND d.archived = false LEFT JOIN policy_control_mappings m ON m.policy_id = p.id LEFT JOIN controls c ON c.id = m.control_id AND c.workspace_id = p.workspace_id WHERE p.workspace_id = $1 AND p.archived_at IS NULL ORDER BY lower(p.name), p.id, lower(c.code), c.id";
const MAPPINGS_SQL: &str = "SELECT c.id AS control_id, m.rationale AS mapping_rationale, m.created_at AS mapping_created_at, e.id, e.workspace_id, e.title, e.description, e.collection_instructions, e.status, e.created_at, e.updated_at FROM evidence_control_mappings m JOIN controls c ON c.id = m.control_id JOIN evidence e ON e.id = m.evidence_id WHERE c.workspace_id = $1 AND e.workspace_id = $1 ORDER BY c.code, c.id, e.title, e.id";
const SUBMISSIONS_SQL: &str = "SELECT s.id, s.evidence_id, s.submitted_by_agent_connection_id, c.user_id AS submitted_by_user_id, s.received_at, s.valid_from, s.valid_until, d.id AS document_id, d.filename, d.upload_status FROM evidence_submissions s JOIN evidence e ON e.id = s.evidence_id LEFT JOIN agent_connections c ON c.id = s.submitted_by_agent_connection_id JOIN documents d ON d.owner_id = s.id AND d.owner_type = 'evidence_submission' AND d.workspace_id = e.workspace_id AND d.archived = false WHERE e.workspace_id = $1 AND s.valid_from <= $3 AND s.valid_until >= $2 ORDER BY s.evidence_id, s.received_at DESC, s.id DESC";

fn policies_from_rows(rows: Vec<Row>) -> Result<Vec<AuditorPortalPolicy>, Error> {
    let mut result = Vec::new();
    let mut current = None;
    for row in rows {
        let id = PolicyId::from(row.try_get::<_, Uuid>("policy_id")?);
        if current != Some(id) {
            result.push(AuditorPortalPolicy {
                id,
                name: row.try_get("policy_name")?,
                description: row.try_get("policy_description")?,
                controls: vec![],
                document: policy_document(&row, id)?,
                created_at: row.try_get("policy_created_at")?,
                updated_at: row.try_get("policy_updated_at")?,
            });
            current = Some(id);
        }
        if let Some(control_id) = row.try_get::<_, Option<Uuid>>("control_id")? {
            if let Some(policy) = result.last_mut() {
                policy.controls.push(ControlSummary {
                    id: control_id.into(),
                    code: row.try_get("control_code")?,
                    title: row.try_get("control_title")?,
                    description: row.try_get("control_description")?,
                });
            }
        }
    }
    Ok(result)
}
fn policy_document(
    row: &Row,
    policy_id: PolicyId,
) -> Result<Option<AuditorPortalPolicyDocument>, Error> {
    let Some(id) = row.try_get::<_, Option<Uuid>>("document_id")? else {
        return Ok(None);
    };
    let status =
        DocumentUploadStatus::from_str(&row.try_get::<_, String>("document_upload_status")?)?;
    Ok(Some(AuditorPortalPolicyDocument {
        id: id.into(),
        policy_id,
        created_by_user_id: row
            .try_get::<_, Uuid>("document_created_by_user_id")?
            .into(),
        filename: row.try_get("document_filename")?,
        content_type: row.try_get("document_content_type")?,
        content_length: row.try_get("document_content_length")?,
        checksum_sha256: row.try_get("document_checksum_sha256")?,
        checksum_crc32c: row.try_get("document_checksum_crc32c")?,
        upload_status: status,
        created_at: row.try_get("document_created_at")?,
        download_eligible: status == DocumentUploadStatus::Uploaded,
    }))
}
fn mapping_from_row(row: Row) -> Result<Mapping, Error> {
    Ok(Mapping {
        control_id: row.try_get::<_, Uuid>("control_id")?.into(),
        rationale: row.try_get("mapping_rationale")?,
        created_at: row.try_get("mapping_created_at")?,
        evidence: EvidenceDetail {
            id: row.try_get::<_, Uuid>("id")?.into(),
            workspace_id: row.try_get::<_, Uuid>("workspace_id")?.into(),
            title: row.try_get("title")?,
            description: row.try_get("description")?,
            collection_instructions: row.try_get("collection_instructions")?,
            status: row
                .try_get::<_, String>("status")?
                .parse::<EvidenceStatus>()?,
            created_at: row.try_get("created_at")?,
            updated_at: row.try_get("updated_at")?,
        },
    })
}
fn submission_from_row(row: &Row) -> Result<EvidenceSubmission, Error> {
    let user_id = row.try_get::<_, Uuid>("submitted_by_user_id")?.into();
    let Some(connection_id) = row.try_get::<_, Option<Uuid>>("submitted_by_agent_connection_id")?
    else {
        return Err(Error::InvariantViolation(
            "auditor portal submission must have an agent connection submitter",
        ));
    };
    Ok(EvidenceSubmission {
        id: EvidenceSubmissionId::from(row.try_get::<_, Uuid>("id")?),
        evidence_id: EvidenceId::from(row.try_get::<_, Uuid>("evidence_id")?),
        submitted_by: EvidenceSubmitter::AgentConnection {
            agent_connection_id: connection_id.into(),
            user_id,
        },
        received_at: row.try_get("received_at")?,
        valid_from: row.try_get("valid_from")?,
        valid_until: row.try_get("valid_until")?,
    })
}
