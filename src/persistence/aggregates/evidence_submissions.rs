use deadpool_postgres::GenericClient;
use tokio_postgres::Row;
use uuid::Uuid;

use crate::{
    domain::{EvidenceId, EvidenceSubmission, EvidenceSubmissionId, EvidenceSubmitter},
    persistence::WorkspaceUnitOfWork,
};

use super::Error;

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
            .query_opt(
                r#"SELECT s.id, s.evidence_id, s.submitted_by_agent_connection_id,
 c.user_id AS submitted_by_user_id, s.received_at, s.valid_from, s.valid_until
FROM evidence_submissions s
JOIN evidence e ON e.id = s.evidence_id
JOIN agent_connections c ON c.id = s.submitted_by_agent_connection_id
WHERE s.id = $1 AND e.workspace_id = $2
FOR UPDATE OF s"#,
                &[&Uuid::from(id), &Uuid::from(self.workspace.workspace_id)],
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
        let changed = self.workspace.transaction.execute(
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
              &Uuid::from(self.workspace.workspace_id)],
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
