use std::{fmt, str::FromStr};

use chrono::{DateTime, Utc};
use serde_json::Value;

use super::{ids::uuid_id, ActorId, DomainError, EvidenceRequestId};

uuid_id!(EvidenceSubmissionId);
uuid_id!(EvidenceAttachmentId);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttachmentScanStatus {
    Pending,
    Clean,
    Malicious,
    Failed,
}

impl AttachmentScanStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Clean => "clean",
            Self::Malicious => "malicious",
            Self::Failed => "failed",
        }
    }
}

impl fmt::Display for AttachmentScanStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for AttachmentScanStatus {
    type Err = DomainError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "pending" => Ok(Self::Pending),
            "clean" => Ok(Self::Clean),
            "malicious" => Ok(Self::Malicious),
            "failed" => Ok(Self::Failed),
            _ => Err(DomainError::InvalidEnumValue {
                field: "scan_status",
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
    pub submitted_by: ActorId,
    pub received_at: DateTime<Utc>,
    pub coverage_start_at: DateTime<Utc>,
    pub coverage_end_at: DateTime<Utc>,
    pub source_system: String,
    pub collection_method: String,
    // Integration-specific receipt metadata, such as external run IDs,
    // source URLs, export timestamps, or webhook delivery IDs.
    pub provenance: Value,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateEvidenceSubmissionPayload {
    pub evidence_request_id: EvidenceRequestId,
    pub coverage_start_at: DateTime<Utc>,
    pub coverage_end_at: DateTime<Utc>,
    pub source_system: String,
    pub collection_method: String,
    pub provenance: Value,
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

/**
 * EvidenceAttachmentScan is a record of a virus scan done to an uploaded
 * evidence attachment.
 */
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvidenceAttachmentScan {
    pub evidence_attachment_id: EvidenceAttachmentId,
    pub scan_status: AttachmentScanStatus,
    pub scanner_name: Option<String>,
    pub scanner_version: Option<String>,
    pub scanned_at: Option<DateTime<Utc>>,
    pub scan_failure_reason: Option<String>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvidenceAttachmentWithScan {
    pub attachment: EvidenceAttachment,
    pub scan: EvidenceAttachmentScan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvidenceSubmissionDetail {
    pub submission: EvidenceSubmission,
    pub attachments: Vec<EvidenceAttachmentWithScan>,
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use uuid::Uuid;

    use super::{AttachmentScanStatus, EvidenceAttachmentId, EvidenceSubmissionId};
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
    fn scan_status_parses_allowed_values() {
        assert_eq!(
            AttachmentScanStatus::from_str("pending").unwrap(),
            AttachmentScanStatus::Pending
        );
        assert_eq!(
            AttachmentScanStatus::from_str("clean").unwrap(),
            AttachmentScanStatus::Clean
        );
        assert_eq!(
            AttachmentScanStatus::from_str("malicious").unwrap(),
            AttachmentScanStatus::Malicious
        );
        assert_eq!(
            AttachmentScanStatus::from_str("failed").unwrap(),
            AttachmentScanStatus::Failed
        );
    }

    #[test]
    fn scan_status_rejects_invalid_values() {
        assert_eq!(
            AttachmentScanStatus::from_str("skipped").unwrap_err(),
            DomainError::InvalidEnumValue {
                field: "scan_status",
                value: "skipped".to_owned()
            }
        );
    }
}
