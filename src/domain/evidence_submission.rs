use std::{fmt, str::FromStr};

use chrono::{DateTime, Utc};

use super::{ids::uuid_id, AgentConnectionId, DomainError, EvidenceId, UserId};

uuid_id!(EvidenceSubmissionId);
uuid_id!(EvidenceUploadGrantId);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubmissionUploadStatus {
    PendingUpload,
    Finalizing,
    Uploaded,
    ContainsVirus,
    FailedUpload,
}

impl SubmissionUploadStatus {
    /// Whether the upload lifecycle has reached a terminal state — the point at
    /// which a submission may be archived.
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Uploaded | Self::ContainsVirus | Self::FailedUpload
        )
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::PendingUpload => "pending",
            Self::Finalizing => "finalizing",
            Self::Uploaded => "uploaded",
            Self::ContainsVirus => "contains_virus",
            Self::FailedUpload => "failed",
        }
    }
}

impl fmt::Display for SubmissionUploadStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for SubmissionUploadStatus {
    type Err = DomainError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "pending" => Ok(Self::PendingUpload),
            "finalizing" => Ok(Self::Finalizing),
            "uploaded" => Ok(Self::Uploaded),
            "contains_virus" => Ok(Self::ContainsVirus),
            "failed" => Ok(Self::FailedUpload),
            _ => Err(DomainError::InvalidEnumValue {
                field: "upload_status",
                value: value.to_owned(),
            }),
        }
    }
}

/**
 * An EvidenceSubmission is one file offered as proof for a piece of
 * evidence, over the coverage window the submitter states. Several
 * submissions may share a coverage window when one file is not enough
 * to cover the period. To replace proof, archive a submission and
 * upload another.
 */
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvidenceSubmission {
    pub id: EvidenceSubmissionId,
    pub evidence_id: EvidenceId,
    pub submitted_by: EvidenceSubmitter,
    pub received_at: DateTime<Utc>,
    pub valid_from: DateTime<Utc>,
    pub valid_until: DateTime<Utc>,
    pub filename: String,
    pub content_type: String,
    pub content_length: i64,
    pub object_key: String,
    pub checksum_sha256: String,
    pub checksum_crc32c: String,
    pub upload_status: SubmissionUploadStatus,
    pub archived: bool,
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

/// The file half of a submission. Coverage window and provenance come from
/// the upload session, not the uploader, so they are not part of this payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateEvidenceSubmissionPayload {
    pub filename: String,
    pub content_type: String,
    pub content_length: i64,
    pub object_key: String,
    pub checksum_sha256: String,
    pub checksum_crc32c: String,
}

/// The coverage window an upload session stamps onto every file uploaded
/// through it.
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

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use chrono::{TimeZone, Utc};
    use uuid::Uuid;

    use super::{CoverageWindow, EvidenceSubmissionId, SubmissionUploadStatus};
    use crate::domain::DomainError;

    #[test]
    fn evidence_submission_id_wraps_uuid() {
        let uuid = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440003").unwrap();
        let id = EvidenceSubmissionId::from(uuid);

        assert_eq!(Uuid::from(id), uuid);
    }

    #[test]
    fn upload_status_parses_allowed_values() {
        assert_eq!(
            SubmissionUploadStatus::from_str("pending").unwrap(),
            SubmissionUploadStatus::PendingUpload
        );
        assert_eq!(
            SubmissionUploadStatus::from_str("finalizing").unwrap(),
            SubmissionUploadStatus::Finalizing
        );
        assert_eq!(
            SubmissionUploadStatus::from_str("uploaded").unwrap(),
            SubmissionUploadStatus::Uploaded
        );
        assert_eq!(
            SubmissionUploadStatus::from_str("contains_virus").unwrap(),
            SubmissionUploadStatus::ContainsVirus
        );
        assert_eq!(
            SubmissionUploadStatus::from_str("failed").unwrap(),
            SubmissionUploadStatus::FailedUpload
        );
    }

    #[test]
    fn upload_status_rejects_invalid_values() {
        assert_eq!(
            SubmissionUploadStatus::from_str("skipped").unwrap_err(),
            DomainError::InvalidEnumValue {
                field: "upload_status",
                value: "skipped".to_owned()
            }
        );
    }

    #[test]
    fn upload_status_terminal_states_are_archivable() {
        assert!(SubmissionUploadStatus::Uploaded.is_terminal());
        assert!(SubmissionUploadStatus::ContainsVirus.is_terminal());
        assert!(SubmissionUploadStatus::FailedUpload.is_terminal());
        assert!(!SubmissionUploadStatus::PendingUpload.is_terminal());
        assert!(!SubmissionUploadStatus::Finalizing.is_terminal());
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
