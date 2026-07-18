use std::{fmt, str::FromStr};

use chrono::{DateTime, Utc};
use uuid::Uuid;

use super::{ids::uuid_id, DomainError, EvidenceSubmissionId, PolicyId};

uuid_id!(DocumentId);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentUploadStatus {
    PendingUpload,
    Finalizing,
    Uploaded,
    ContainsVirus,
    FailedUpload,
}

impl DocumentUploadStatus {
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

impl fmt::Display for DocumentUploadStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for DocumentUploadStatus {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentOwner {
    EvidenceSubmission(EvidenceSubmissionId),
    Policy(PolicyId),
}

impl DocumentOwner {
    pub fn owner_type(self) -> &'static str {
        match self {
            Self::EvidenceSubmission(_) => "evidence_submission",
            Self::Policy(_) => "policy",
        }
    }

    pub fn owner_uuid(self) -> Uuid {
        match self {
            Self::EvidenceSubmission(id) => id.into(),
            Self::Policy(id) => id.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentIdentity {
    Evidence {
        evidence_submission_id: EvidenceSubmissionId,
        document_id: DocumentId,
    },
    Policy {
        policy_id: PolicyId,
        document_id: DocumentId,
    },
}

impl DocumentIdentity {
    pub fn document_id(self) -> DocumentId {
        match self {
            Self::Evidence { document_id, .. } | Self::Policy { document_id, .. } => document_id,
        }
    }

    pub fn document_uuid(self) -> Uuid {
        match self {
            Self::Evidence { document_id, .. } => document_id.into(),
            Self::Policy { document_id, .. } => document_id.into(),
        }
    }

    pub fn owner(self) -> DocumentOwner {
        match self {
            Self::Evidence {
                evidence_submission_id,
                ..
            } => DocumentOwner::EvidenceSubmission(evidence_submission_id),
            Self::Policy { policy_id, .. } => DocumentOwner::Policy(policy_id),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Document {
    pub identity: DocumentIdentity,
    pub filename: String,
    pub content_type: String,
    pub content_length: i64,
    pub object_key: String,
    pub checksum_sha256: String,
    pub checksum_crc32c: String,
    pub upload_status: DocumentUploadStatus,
    pub created_at: DateTime<Utc>,
}

impl Document {
    pub fn id(&self) -> DocumentId {
        self.identity.document_id()
    }

    pub fn owner(&self) -> DocumentOwner {
        self.identity.owner()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateDocumentPayload {
    pub owner: DocumentOwner,
    pub filename: String,
    pub content_type: String,
    pub content_length: i64,
    pub object_key: String,
    pub checksum_sha256: String,
    pub checksum_crc32c: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn document_identity_couples_a_document_with_one_typed_owner() {
        let document_id = Uuid::new_v4();

        assert_eq!(
            DocumentIdentity::Evidence {
                evidence_submission_id: Uuid::new_v4().into(),
                document_id: document_id.into(),
            }
            .document_uuid(),
            document_id
        );
        assert_eq!(
            DocumentOwner::Policy(Uuid::new_v4().into()).owner_type(),
            "policy"
        );
    }
}
