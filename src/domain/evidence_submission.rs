use chrono::{DateTime, Utc};

use super::{ids::uuid_id, AgentConnectionId, Document, EvidenceRequestId, UserId};

uuid_id!(EvidenceSubmissionId);
uuid_id!(DocumentUploadGrantId);

/**
 * An EvidenceSubmission represents a particular piece of evidence that
 * is submitted to satisfy a request for evidence for a certain period.
 */
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvidenceSubmission {
    pub id: EvidenceSubmissionId,
    pub evidence_request_id: EvidenceRequestId,
    pub submitted_by: EvidenceSubmitter,
    pub received_at: DateTime<Utc>,
    pub coverage_start_at: DateTime<Utc>,
    pub coverage_end_at: DateTime<Utc>,
    pub source_system: String,
    pub collection_method: String,
    pub summary: Option<String>,
    pub description: Option<String>,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateEvidenceSubmissionPayload {
    pub evidence_request_id: EvidenceRequestId,
    pub coverage_start_at: DateTime<Utc>,
    pub coverage_end_at: DateTime<Utc>,
    pub source_system: String,
    pub collection_method: String,
    pub summary: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvidenceSubmissionDetail {
    pub submission: EvidenceSubmission,
    pub documents: Vec<Document>,
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use uuid::Uuid;

    use super::EvidenceSubmissionId;
    use crate::domain::DomainError;
    use crate::domain::{DocumentId, DocumentUploadStatus};

    #[test]
    fn evidence_submission_id_wraps_uuid() {
        let uuid = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440003").unwrap();
        let id = EvidenceSubmissionId::from(uuid);

        assert_eq!(Uuid::from(id), uuid);
    }

    #[test]
    fn document_id_wraps_uuid() {
        let uuid = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440004").unwrap();
        let id = DocumentId::from(uuid);

        assert_eq!(Uuid::from(id), uuid);
    }

    #[test]
    fn upload_status_parses_allowed_values() {
        assert_eq!(
            DocumentUploadStatus::from_str("pending").unwrap(),
            DocumentUploadStatus::PendingUpload
        );
        assert_eq!(
            DocumentUploadStatus::from_str("finalizing").unwrap(),
            DocumentUploadStatus::Finalizing
        );
        assert_eq!(
            DocumentUploadStatus::from_str("uploaded").unwrap(),
            DocumentUploadStatus::Uploaded
        );
        assert_eq!(
            DocumentUploadStatus::from_str("contains_virus").unwrap(),
            DocumentUploadStatus::ContainsVirus
        );
        assert_eq!(
            DocumentUploadStatus::from_str("failed").unwrap(),
            DocumentUploadStatus::FailedUpload
        );
    }

    #[test]
    fn upload_status_rejects_invalid_values() {
        assert_eq!(
            DocumentUploadStatus::from_str("skipped").unwrap_err(),
            DomainError::InvalidEnumValue {
                field: "upload_status",
                value: "skipped".to_owned()
            }
        );
    }
}
