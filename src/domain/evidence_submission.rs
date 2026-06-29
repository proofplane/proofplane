use std::{fmt, str::FromStr};

use chrono::{DateTime, Utc};

use super::{ids::uuid_id, ApiTokenId, DomainError, EvidenceRequestId, UserId};

uuid_id!(EvidenceSubmissionId);
uuid_id!(EvidenceAttachmentId);
uuid_id!(AttachmentUploadGrantId);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttachmentUploadStatus {
    PendingUpload,
    Finalizing,
    Uploaded,
    ContainsVirus,
    FailedUpload,
}

impl AttachmentUploadStatus {
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

impl fmt::Display for AttachmentUploadStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for AttachmentUploadStatus {
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
pub struct EvidenceSubmitter {
    pub api_token_id: ApiTokenId,
    pub user_id: UserId,
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

/**
 * The EvidenceAttachment is the actual thing that's being presented as
 * evidence. It can be a screenshot of config, a JSON response from a
 * configuration management API, or anything else.
 */
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvidenceAttachment {
    pub id: EvidenceAttachmentId,
    pub evidence_submission_id: EvidenceSubmissionId,
    pub filename: String,
    pub content_type: String,
    pub content_length: i64,
    pub object_key: String,
    pub checksum_sha256: String,
    pub checksum_crc32c: String,
    pub upload_status: AttachmentUploadStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateEvidenceAttachmentPayload {
    pub evidence_submission_id: EvidenceSubmissionId,
    pub filename: String,
    pub content_type: String,
    pub content_length: i64,
    pub object_key: String,
    pub checksum_sha256: String,
    pub checksum_crc32c: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvidenceSubmissionDetail {
    pub submission: EvidenceSubmission,
    pub attachments: Vec<EvidenceAttachment>,
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use uuid::Uuid;

    use super::{AttachmentUploadStatus, EvidenceAttachmentId, EvidenceSubmissionId};
    use crate::domain::DomainError;

    #[test]
    fn evidence_submission_id_wraps_uuid() {
        let uuid = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440003").unwrap();
        let id = EvidenceSubmissionId::from(uuid);

        assert_eq!(Uuid::from(id), uuid);
    }

    #[test]
    fn evidence_attachment_id_wraps_uuid() {
        let uuid = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440004").unwrap();
        let id = EvidenceAttachmentId::from(uuid);

        assert_eq!(Uuid::from(id), uuid);
    }

    #[test]
    fn upload_status_parses_allowed_values() {
        assert_eq!(
            AttachmentUploadStatus::from_str("pending").unwrap(),
            AttachmentUploadStatus::PendingUpload
        );
        assert_eq!(
            AttachmentUploadStatus::from_str("finalizing").unwrap(),
            AttachmentUploadStatus::Finalizing
        );
        assert_eq!(
            AttachmentUploadStatus::from_str("uploaded").unwrap(),
            AttachmentUploadStatus::Uploaded
        );
        assert_eq!(
            AttachmentUploadStatus::from_str("contains_virus").unwrap(),
            AttachmentUploadStatus::ContainsVirus
        );
        assert_eq!(
            AttachmentUploadStatus::from_str("failed").unwrap(),
            AttachmentUploadStatus::FailedUpload
        );
    }

    #[test]
    fn upload_status_rejects_invalid_values() {
        assert_eq!(
            AttachmentUploadStatus::from_str("skipped").unwrap_err(),
            DomainError::InvalidEnumValue {
                field: "upload_status",
                value: "skipped".to_owned()
            }
        );
    }
}
