use tokio_postgres::Row;
use uuid::Uuid;

use crate::{
    domain::{EvidenceId, EvidenceSubmission, EvidenceSubmissionId, EvidenceSubmitter},
    persistence::WorkspaceUnitOfWork,
};

use super::params::param;
use super::{
    snapshot::{save_snapshot, snapshot_record},
    Error,
};

/// Complete-snapshot repository for the submission provenance aggregate.
pub struct EvidenceSubmissionRepository<'a> {
    workspace: &'a WorkspaceUnitOfWork<'a>,
}

impl<'a> WorkspaceUnitOfWork<'a> {
    pub fn evidence_submissions(&'a self) -> EvidenceSubmissionRepository<'a> {
        EvidenceSubmissionRepository { workspace: self }
    }
}

impl EvidenceSubmissionRepository<'_> {
    pub async fn get(&self, id: EvidenceSubmissionId) -> Result<Option<EvidenceSubmission>, Error> {
        self.workspace
            .transaction
            .query_typed_opt(
                r#"SELECT s.id, s.evidence_id, s.submitted_by_agent_connection_id,
 c.user_id AS submitted_by_user_id, s.received_at, s.valid_from, s.valid_until
FROM evidence_submissions s
JOIN evidence e ON e.id = s.evidence_id
JOIN agent_connections c ON c.id = s.submitted_by_agent_connection_id
WHERE s.id = $1 AND e.workspace_id = $2
FOR UPDATE OF s"#,
                &[
                    param(&Uuid::from(id)),
                    param(&Uuid::from(self.workspace.workspace_id)),
                ],
            )
            .await?
            .map(|row| EvidenceSubmissionRecord::try_from_row(&row)?.into_domain(&row))
            .transpose()
    }

    /// Persists the entire submission snapshot; evidence eligibility is checked
    /// by the command handler before this boundary is called.
    pub async fn save(&self, submission: &EvidenceSubmission) -> Result<(), Error> {
        let record = EvidenceSubmissionRecord::from_domain(submission)?;
        save_snapshot(self.workspace.transaction, record.as_snapshot()).await
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveDocumentResult {
    Archived,
    NotFound,
    NotTerminal,
}

snapshot_record! {
    struct EvidenceSubmissionRecord {
        id: Uuid,
        evidence_id: Uuid,
        submitted_by_agent_connection_id: Uuid,
        received_at: chrono::DateTime<chrono::Utc>,
        valid_from: chrono::DateTime<chrono::Utc>,
        valid_until: chrono::DateTime<chrono::Utc>,
    }
    table: evidence_submissions,
    conflict: id,
}

impl EvidenceSubmissionRecord {
    fn try_from_row(row: &Row) -> Result<Self, Error> {
        Ok(Self {
            id: row.try_get("id")?,
            evidence_id: row.try_get("evidence_id")?,
            submitted_by_agent_connection_id: row.try_get("submitted_by_agent_connection_id")?,
            received_at: row.try_get("received_at")?,
            valid_from: row.try_get("valid_from")?,
            valid_until: row.try_get("valid_until")?,
        })
    }

    fn from_domain(submission: &EvidenceSubmission) -> Result<Self, Error> {
        let EvidenceSubmitter::AgentConnection {
            agent_connection_id,
            ..
        } = submission.submitted_by;
        Ok(Self {
            id: submission.id.into(),
            evidence_id: submission.evidence_id.into(),
            submitted_by_agent_connection_id: agent_connection_id.into(),
            received_at: submission.received_at,
            valid_from: submission.valid_from,
            valid_until: submission.valid_until,
        })
    }

    fn into_domain(self, row: &Row) -> Result<EvidenceSubmission, Error> {
        Ok(EvidenceSubmission {
            id: EvidenceSubmissionId::from(self.id),
            evidence_id: EvidenceId::from(self.evidence_id),
            submitted_by: EvidenceSubmitter::AgentConnection {
                agent_connection_id: self.submitted_by_agent_connection_id.into(),
                user_id: row.try_get::<_, Uuid>("submitted_by_user_id")?.into(),
            },
            received_at: self.received_at,
            valid_from: self.valid_from,
            valid_until: self.valid_until,
        })
    }
}
