use chrono::{DateTime, Utc};

use super::{ids::uuid_id, AgentConnectionId, Document, DomainError, EvidenceId, UserId};

uuid_id!(EvidenceSubmissionId);
uuid_id!(DocumentUploadGrantId);
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoverageWindow {
    pub valid_from: DateTime<Utc>,
    pub valid_until: DateTime<Utc>,
}

impl CoverageWindow {
    pub fn new(valid_from: DateTime<Utc>, valid_until: DateTime<Utc>) -> Result<Self, DomainError> {
        if valid_until < valid_from {
            return Err(DomainError::InvalidCoverageWindow);
        }

        Ok(Self {
            valid_from,
            valid_until,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvidenceSubmission {
    pub id: EvidenceSubmissionId,
    pub evidence_id: EvidenceId,
    pub submitted_by: EvidenceSubmitter,
    pub received_at: DateTime<Utc>,
    pub valid_from: DateTime<Utc>,
    pub valid_until: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvidenceSubmissionTransitionOutcome {
    Created,
    Replayed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EvidenceSubmissionTransition {
    pub outcome: EvidenceSubmissionTransitionOutcome,
}

impl EvidenceSubmission {
    pub fn create(
        id: EvidenceSubmissionId,
        evidence_id: EvidenceId,
        submitted_by: EvidenceSubmitter,
        coverage: CoverageWindow,
        received_at: DateTime<Utc>,
    ) -> (Self, EvidenceSubmissionTransition) {
        (
            Self {
                id,
                evidence_id,
                submitted_by,
                received_at,
                valid_from: coverage.valid_from,
                valid_until: coverage.valid_until,
            },
            EvidenceSubmissionTransition {
                outcome: EvidenceSubmissionTransitionOutcome::Created,
            },
        )
    }

    pub fn coverage(&self) -> CoverageWindow {
        CoverageWindow {
            valid_from: self.valid_from,
            valid_until: self.valid_until,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvidenceSubmitter {
    AgentConnection {
        agent_connection_id: AgentConnectionId,
        user_id: UserId,
    },
}

impl EvidenceSubmitter {
    pub fn user_id(self) -> UserId {
        match self {
            Self::AgentConnection { user_id, .. } => user_id,
        }
    }

    pub fn agent_connection_id(self) -> Option<AgentConnectionId> {
        match self {
            Self::AgentConnection {
                agent_connection_id,
                ..
            } => Some(agent_connection_id),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CreateEvidenceSubmissionPayload {
    pub id: EvidenceSubmissionId,
    pub evidence_id: EvidenceId,
    pub coverage: CoverageWindow,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvidenceSubmissionDetail {
    pub submission: EvidenceSubmission,
    pub document: Document,
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};
    use uuid::Uuid;

    use super::{CoverageWindow, EvidenceSubmissionId};
    use crate::domain::DomainError;

    #[test]
    fn evidence_submission_id_wraps_uuid() {
        let uuid = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440003").unwrap();
        let id = EvidenceSubmissionId::from(uuid);

        assert_eq!(Uuid::from(id), uuid);
    }

    #[test]
    fn coverage_window_accepts_ordered_and_instant_windows() {
        let start = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        let end = Utc.with_ymd_and_hms(2026, 3, 31, 0, 0, 0).unwrap();

        assert_eq!(CoverageWindow::new(start, end).unwrap().valid_until, end);
        assert_eq!(CoverageWindow::new(start, start).unwrap().valid_from, start);
    }

    #[test]
    fn coverage_window_rejects_end_before_start() {
        let start = Utc.with_ymd_and_hms(2026, 3, 31, 0, 0, 0).unwrap();
        let end = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();

        assert_eq!(
            CoverageWindow::new(start, end).unwrap_err(),
            DomainError::InvalidCoverageWindow
        );
    }
}
