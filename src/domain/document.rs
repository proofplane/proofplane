use std::{fmt, str::FromStr};

use chrono::{DateTime, Utc};
use uuid::Uuid;

use super::{ids::uuid_id, DomainError, EvidenceSubmissionId, PolicyId, UserId};

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
    pub created_by_user_id: UserId,
    pub filename: String,
    pub content_type: String,
    pub content_length: i64,
    pub object_key: String,
    pub checksum_sha256: String,
    pub checksum_crc32c: String,
    pub upload_status: DocumentUploadStatus,
    pub archived: bool,
    pub created_at: DateTime<Utc>,
}

impl Document {
    pub fn id(&self) -> DocumentId {
        self.identity.document_id()
    }

    pub fn owner(&self) -> DocumentOwner {
        self.identity.owner()
    }

    #[allow(clippy::too_many_arguments)]
    pub fn create(
        identity: DocumentIdentity,
        created_by_user_id: UserId,
        payload: CreateDocumentPayload,
        created_at: DateTime<Utc>,
    ) -> Result<(Self, DocumentTransition), DocumentLifecycleError> {
        if payload.owner != identity.owner() {
            return Err(DocumentLifecycleError::OwnerMismatch);
        }
        if payload.content_length < 0 {
            return Err(DocumentLifecycleError::NegativeContentLength);
        }
        let document = Self {
            identity,
            created_by_user_id,
            filename: payload.filename,
            content_type: payload.content_type,
            content_length: payload.content_length,
            object_key: payload.object_key,
            checksum_sha256: payload.checksum_sha256,
            checksum_crc32c: payload.checksum_crc32c,
            upload_status: DocumentUploadStatus::PendingUpload,
            archived: false,
            created_at,
        };
        let transition = DocumentTransition::applied(
            DocumentTransitionOutcome::Created,
            Some(DocumentEvent::ScanRequested {
                identity: document.identity,
                object_key: document.object_key.clone(),
            }),
        );
        Ok((document, transition))
    }

    pub fn scan_clean(&mut self) -> DocumentTransition {
        self.transition(
            DocumentUploadStatus::PendingUpload,
            DocumentUploadStatus::Finalizing,
            Some(DocumentEvent::FinalizationRequested {
                identity: self.identity,
                object_key: self.object_key.clone(),
            }),
        )
    }

    pub fn scan_malicious(&mut self) -> DocumentTransition {
        self.transition(
            DocumentUploadStatus::PendingUpload,
            DocumentUploadStatus::ContainsVirus,
            None,
        )
    }

    pub fn scan_failed(&mut self) -> DocumentTransition {
        self.transition(
            DocumentUploadStatus::PendingUpload,
            DocumentUploadStatus::FailedUpload,
            None,
        )
    }

    pub fn finalize_uploaded(&mut self, final_object_key: String) -> DocumentTransition {
        if self.archived || self.upload_status != DocumentUploadStatus::Finalizing {
            return DocumentTransition::ignored();
        }
        self.object_key = final_object_key;
        self.upload_status = DocumentUploadStatus::Uploaded;
        DocumentTransition::applied(DocumentTransitionOutcome::Finalized, None)
    }

    pub fn archive(&mut self) -> DocumentTransition {
        if self.archived {
            return DocumentTransition::ignored();
        }
        if !matches!(
            self.upload_status,
            DocumentUploadStatus::Uploaded
                | DocumentUploadStatus::ContainsVirus
                | DocumentUploadStatus::FailedUpload
        ) {
            return DocumentTransition::rejected();
        }
        self.archived = true;
        DocumentTransition::applied(DocumentTransitionOutcome::Archived, None)
    }

    fn transition(
        &mut self,
        expected: DocumentUploadStatus,
        next: DocumentUploadStatus,
        event: Option<DocumentEvent>,
    ) -> DocumentTransition {
        if self.archived || self.upload_status != expected {
            return DocumentTransition::ignored();
        }
        self.upload_status = next;
        DocumentTransition::applied(DocumentTransitionOutcome::Scanned, event)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DocumentEvent {
    ScanRequested {
        identity: DocumentIdentity,
        object_key: String,
    },
    FinalizationRequested {
        identity: DocumentIdentity,
        object_key: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentTransitionOutcome {
    Created,
    Scanned,
    Finalized,
    Archived,
    Ignored,
    Rejected,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentTransition {
    pub outcome: DocumentTransitionOutcome,
    pub event: Option<DocumentEvent>,
}

impl DocumentTransition {
    fn applied(outcome: DocumentTransitionOutcome, event: Option<DocumentEvent>) -> Self {
        Self { outcome, event }
    }
    fn ignored() -> Self {
        Self::applied(DocumentTransitionOutcome::Ignored, None)
    }
    fn rejected() -> Self {
        Self::applied(DocumentTransitionOutcome::Rejected, None)
    }
    pub fn changed(&self) -> bool {
        !matches!(
            self.outcome,
            DocumentTransitionOutcome::Ignored | DocumentTransitionOutcome::Rejected
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum DocumentLifecycleError {
    #[error("document owner does not match its identity")]
    OwnerMismatch,
    #[error("document content length cannot be negative")]
    NegativeContentLength,
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

    #[test]
    fn transitions_emit_only_work_that_has_a_consumer() {
        let identity = DocumentIdentity::Evidence {
            evidence_submission_id: Uuid::new_v4().into(),
            document_id: Uuid::new_v4().into(),
        };
        let (mut document, created) = Document::create(
            identity,
            Uuid::new_v4().into(),
            CreateDocumentPayload {
                owner: identity.owner(),
                filename: "evidence.pdf".to_owned(),
                content_type: "application/pdf".to_owned(),
                content_length: 1,
                object_key: "quarantine/x".to_owned(),
                checksum_sha256: "sha".to_owned(),
                checksum_crc32c: "crc".to_owned(),
            },
            Utc::now(),
        )
        .unwrap();
        assert!(matches!(
            created.event,
            Some(DocumentEvent::ScanRequested { .. })
        ));
        let transition = document.scan_clean();
        assert!(matches!(
            transition.event,
            Some(DocumentEvent::FinalizationRequested { .. })
        ));
        assert_eq!(
            document.scan_clean().outcome,
            DocumentTransitionOutcome::Ignored
        );
        assert!(document.scan_malicious().event.is_none());
    }
}
