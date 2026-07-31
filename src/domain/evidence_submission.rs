use std::fmt;

use chrono::{DateTime, Utc};
use http::HeaderValue;

use crate::validation::Validation;

use super::{
    ids::uuid_id, validate_document_filename, AgentConnectionId, Document, DomainError, EvidenceId,
    Sha256Digest, UserId,
};

uuid_id!(EvidenceSubmissionId);
uuid_id!(DocumentUploadGrantId);
uuid_id!(AgentEvidenceUploadGrantId);

#[derive(Clone, PartialEq, Eq)]
pub struct AgentEvidenceUploadDeclaration {
    pub(crate) filename: String,
    pub(crate) content_type: String,
    pub(crate) expected_content_length: u64,
    pub(crate) expected_sha256: Option<Sha256Digest>,
}

impl fmt::Debug for AgentEvidenceUploadDeclaration {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("AgentEvidenceUploadDeclaration([redacted])")
    }
}

impl AgentEvidenceUploadDeclaration {
    pub fn new(
        filename: String,
        content_type: String,
        expected_content_length: u64,
        expected_sha256: Option<String>,
        max_bytes: u64,
    ) -> Validation<Self, DomainError> {
        let mut errors = Vec::new();
        let filename = match validate_document_filename(filename).into_result() {
            Ok(filename) => Some(filename),
            Err(mut filename_errors) => {
                errors.append(&mut filename_errors);
                None
            }
        };
        let valid_content_type = !content_type.is_empty()
            && content_type.trim() == content_type
            && content_type.parse::<mime::Mime>().is_ok()
            && HeaderValue::from_str(&content_type).is_ok();
        if !valid_content_type {
            errors.push(DomainError::InvalidDocumentContentType);
        }
        let maximum = max_bytes.min(i64::MAX as u64);
        if expected_content_length > maximum {
            errors.push(DomainError::DocumentContentLengthTooLarge { maximum });
        }
        let expected_sha256 = match expected_sha256 {
            None => Some(None),
            Some(value) if is_lowercase_sha256(&value) => {
                let bytes = hex::decode(value)
                    .ok()
                    .and_then(|bytes| bytes.try_into().ok());
                bytes.map(Sha256Digest::from_bytes).map(Some)
            }
            Some(_) => None,
        };
        if expected_sha256.is_none() {
            errors.push(DomainError::InvalidDocumentSha256Checksum);
        }

        match (filename, valid_content_type, expected_sha256) {
            (Some(filename), true, Some(expected_sha256)) if errors.is_empty() => {
                Validation::valid(Self {
                    filename,
                    content_type,
                    expected_content_length,
                    expected_sha256,
                })
            }
            _ => Validation::invalid_many(errors),
        }
    }

    pub fn filename(&self) -> &str {
        &self.filename
    }

    pub fn content_type(&self) -> &str {
        &self.content_type
    }

    pub fn expected_content_length(&self) -> u64 {
        self.expected_content_length
    }

    pub fn expected_sha256(&self) -> Option<&Sha256Digest> {
        self.expected_sha256.as_ref()
    }
}

fn is_lowercase_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

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

    use super::{AgentEvidenceUploadDeclaration, CoverageWindow, EvidenceSubmissionId};
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

    #[test]
    fn machine_upload_declaration_accepts_valid_metadata() {
        let declaration = AgentEvidenceUploadDeclaration::new(
            "access-review.pdf".to_owned(),
            "application/pdf".to_owned(),
            483_920,
            Some("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad".to_owned()),
            25 * 1024 * 1024,
        )
        .into_result()
        .unwrap();

        assert_eq!(declaration.filename, "access-review.pdf");
        assert_eq!(declaration.content_type, "application/pdf");
        assert_eq!(declaration.expected_content_length, 483_920);
        assert!(declaration.expected_sha256.is_some());
    }

    #[test]
    fn machine_upload_declaration_rejects_invalid_metadata() {
        let errors = AgentEvidenceUploadDeclaration::new(
            "path/report.pdf".to_owned(),
            "not a media type".to_owned(),
            101,
            Some("ABC".to_owned()),
            100,
        )
        .into_result()
        .unwrap_err();

        assert_eq!(
            errors,
            vec![
                DomainError::InvalidDocumentFilenameCharacters,
                DomainError::InvalidDocumentContentType,
                DomainError::DocumentContentLengthTooLarge { maximum: 100 },
                DomainError::InvalidDocumentSha256Checksum,
            ]
        );
    }

    #[test]
    fn machine_upload_declaration_debug_redacts_file_metadata() {
        let declaration = AgentEvidenceUploadDeclaration::new(
            "secret-report.pdf".to_owned(),
            "application/secret".to_owned(),
            42,
            Some("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad".to_owned()),
            100,
        )
        .into_result()
        .unwrap();

        let debug = format!("{declaration:?}");
        assert_eq!(debug, "AgentEvidenceUploadDeclaration([redacted])");
        assert!(!debug.contains("secret-report.pdf"));
        assert!(!debug.contains("application/secret"));
    }
}
