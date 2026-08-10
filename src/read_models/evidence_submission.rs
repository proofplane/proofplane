use crate::domain::{Document, EvidenceSubmission};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvidenceSubmissionDetail {
    pub submission: EvidenceSubmission,
    pub document: Document,
}
