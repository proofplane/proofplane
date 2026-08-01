use std::fmt;

use chrono::{DateTime, Utc};
use http::HeaderValue;

use crate::validation::Validation;

use super::{
    ids::uuid_id, validate_document_filename, AgentConnectionId, CoverageWindow, DocumentId,
    DomainError, EvidenceId, EvidenceSubmissionId, Sha256Digest, UserId, WorkspaceId,
};

uuid_id!(AgentEvidenceUploadGrantId);

#[derive(Clone, PartialEq, Eq)]
pub struct AgentEvidenceUploadDeclaration {
    filename: String,
    content_type: String,
    expected_content_length: u64,
    expected_sha256: Option<Sha256Digest>,
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
            Some(value) if is_lowercase_sha256(&value) => hex::decode(value)
                .ok()
                .and_then(|bytes| bytes.try_into().ok())
                .map(Sha256Digest::from_bytes)
                .map(Some),
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

    pub(crate) fn rehydrate(
        filename: String,
        content_type: String,
        expected_content_length: u64,
        expected_sha256: Option<Sha256Digest>,
    ) -> Result<Self, AgentEvidenceUploadGrantError> {
        Self::new(
            filename,
            content_type,
            expected_content_length,
            expected_sha256.map(|digest| hex::encode(digest.as_bytes())),
            i64::MAX as u64,
        )
        .into_result()
        .map_err(|_| AgentEvidenceUploadGrantError::InvalidRehydration)
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
pub struct AgentEvidenceUploadAuthority {
    upload_id: AgentEvidenceUploadGrantId,
    workspace_id: WorkspaceId,
    evidence_id: EvidenceId,
    submission_id: EvidenceSubmissionId,
    issued_by_user_id: UserId,
    issued_via_agent_connection_id: AgentConnectionId,
    expires_at: DateTime<Utc>,
}

impl AgentEvidenceUploadAuthority {
    pub fn new(
        upload_id: AgentEvidenceUploadGrantId,
        workspace_id: WorkspaceId,
        evidence_id: EvidenceId,
        submission_id: EvidenceSubmissionId,
        issued_by_user_id: UserId,
        issued_via_agent_connection_id: AgentConnectionId,
        expires_at: DateTime<Utc>,
    ) -> Self {
        Self {
            upload_id,
            workspace_id,
            evidence_id,
            submission_id,
            issued_by_user_id,
            issued_via_agent_connection_id,
            expires_at,
        }
    }

    pub fn upload_id(&self) -> AgentEvidenceUploadGrantId {
        self.upload_id
    }

    pub fn workspace_id(&self) -> WorkspaceId {
        self.workspace_id
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AgentEvidenceUploadGrantLifecycle {
    Pending,
    Completed {
        document_id: DocumentId,
        completed_at: DateTime<Utc>,
    },
}

#[derive(Debug, PartialEq, Eq)]
pub struct AgentEvidenceUploadGrant {
    id: AgentEvidenceUploadGrantId,
    submission_id: EvidenceSubmissionId,
    workspace_id: WorkspaceId,
    evidence_id: EvidenceId,
    coverage: CoverageWindow,
    declaration: AgentEvidenceUploadDeclaration,
    issued_by_user_id: UserId,
    issued_via_agent_connection_id: AgentConnectionId,
    issued_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
    lifecycle: AgentEvidenceUploadGrantLifecycle,
}

impl AgentEvidenceUploadGrant {
    #[allow(clippy::too_many_arguments)]
    pub fn issue(
        id: AgentEvidenceUploadGrantId,
        submission_id: EvidenceSubmissionId,
        workspace_id: WorkspaceId,
        evidence_id: EvidenceId,
        coverage: CoverageWindow,
        declaration: AgentEvidenceUploadDeclaration,
        issued_by_user_id: UserId,
        issued_via_agent_connection_id: AgentConnectionId,
        issued_at: DateTime<Utc>,
        expires_at: DateTime<Utc>,
    ) -> Result<Self, AgentEvidenceUploadGrantError> {
        if expires_at <= issued_at {
            return Err(AgentEvidenceUploadGrantError::InvalidIssuance);
        }
        Ok(Self {
            id,
            submission_id,
            workspace_id,
            evidence_id,
            coverage,
            declaration,
            issued_by_user_id,
            issued_via_agent_connection_id,
            issued_at,
            expires_at,
            lifecycle: AgentEvidenceUploadGrantLifecycle::Pending,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn rehydrate(
        id: AgentEvidenceUploadGrantId,
        submission_id: EvidenceSubmissionId,
        workspace_id: WorkspaceId,
        evidence_id: EvidenceId,
        coverage: CoverageWindow,
        declaration: AgentEvidenceUploadDeclaration,
        issued_by_user_id: UserId,
        issued_via_agent_connection_id: AgentConnectionId,
        issued_at: DateTime<Utc>,
        expires_at: DateTime<Utc>,
        completed_at: Option<DateTime<Utc>>,
        document_id: Option<DocumentId>,
    ) -> Result<Self, AgentEvidenceUploadGrantError> {
        let mut grant = Self::issue(
            id,
            submission_id,
            workspace_id,
            evidence_id,
            coverage,
            declaration,
            issued_by_user_id,
            issued_via_agent_connection_id,
            issued_at,
            expires_at,
        )
        .map_err(|_| AgentEvidenceUploadGrantError::InvalidRehydration)?;
        grant.lifecycle = match (completed_at, document_id) {
            (None, None) => AgentEvidenceUploadGrantLifecycle::Pending,
            (Some(completed_at), Some(document_id)) if completed_at >= issued_at => {
                AgentEvidenceUploadGrantLifecycle::Completed {
                    document_id,
                    completed_at,
                }
            }
            _ => return Err(AgentEvidenceUploadGrantError::InvalidRehydration),
        };
        Ok(grant)
    }

    pub fn matches_authority(
        &self,
        authority: &AgentEvidenceUploadAuthority,
    ) -> Result<(), AgentEvidenceUploadGrantError> {
        if self.id == authority.upload_id
            && self.workspace_id == authority.workspace_id
            && self.evidence_id == authority.evidence_id
            && self.submission_id == authority.submission_id
            && self.issued_by_user_id == authority.issued_by_user_id
            && self.issued_via_agent_connection_id == authority.issued_via_agent_connection_id
            && self.expires_at == authority.expires_at
        {
            Ok(())
        } else {
            Err(AgentEvidenceUploadGrantError::AuthorityMismatch)
        }
    }

    pub fn ensure_pending_at(
        &self,
        now: DateTime<Utc>,
    ) -> Result<(), AgentEvidenceUploadGrantError> {
        if self.lifecycle != AgentEvidenceUploadGrantLifecycle::Pending {
            return Err(AgentEvidenceUploadGrantError::AlreadyCompleted);
        }
        if now >= self.expires_at {
            return Err(AgentEvidenceUploadGrantError::Expired);
        }
        Ok(())
    }

    pub fn validate_declared_file(
        &self,
        content_type: &str,
        content_length: u64,
    ) -> Result<(), AgentEvidenceUploadGrantError> {
        if content_type != self.declaration.content_type {
            return Err(AgentEvidenceUploadGrantError::ContentTypeMismatch);
        }
        if content_length != self.declaration.expected_content_length {
            return Err(AgentEvidenceUploadGrantError::DeclaredContentLengthMismatch);
        }
        Ok(())
    }

    pub fn validate_staged_file(
        &self,
        content_length: i64,
        checksum_sha256: &str,
    ) -> Result<(), AgentEvidenceUploadGrantError> {
        let content_length = u64::try_from(content_length)
            .map_err(|_| AgentEvidenceUploadGrantError::ReceivedContentLengthMismatch)?;
        if content_length != self.declaration.expected_content_length {
            return Err(AgentEvidenceUploadGrantError::ReceivedContentLengthMismatch);
        }
        if self
            .declaration
            .expected_sha256
            .is_some_and(|expected| hex::encode(expected.as_bytes()) != checksum_sha256)
        {
            return Err(AgentEvidenceUploadGrantError::ChecksumMismatch);
        }
        Ok(())
    }

    pub fn complete(
        &mut self,
        document_id: DocumentId,
        completed_at: DateTime<Utc>,
    ) -> Result<(), AgentEvidenceUploadGrantError> {
        self.ensure_pending_at(completed_at)?;
        if completed_at < self.issued_at {
            return Err(AgentEvidenceUploadGrantError::InvalidCompletion);
        }
        self.lifecycle = AgentEvidenceUploadGrantLifecycle::Completed {
            document_id,
            completed_at,
        };
        Ok(())
    }

    pub fn id(&self) -> AgentEvidenceUploadGrantId {
        self.id
    }
    pub fn submission_id(&self) -> EvidenceSubmissionId {
        self.submission_id
    }
    pub fn workspace_id(&self) -> WorkspaceId {
        self.workspace_id
    }
    pub fn evidence_id(&self) -> EvidenceId {
        self.evidence_id
    }
    pub fn coverage(&self) -> CoverageWindow {
        self.coverage
    }
    pub fn declaration(&self) -> &AgentEvidenceUploadDeclaration {
        &self.declaration
    }
    pub fn issued_by_user_id(&self) -> UserId {
        self.issued_by_user_id
    }
    pub fn issued_via_agent_connection_id(&self) -> AgentConnectionId {
        self.issued_via_agent_connection_id
    }
    pub fn issued_at(&self) -> DateTime<Utc> {
        self.issued_at
    }
    pub fn expires_at(&self) -> DateTime<Utc> {
        self.expires_at
    }
    pub fn completed_at(&self) -> Option<DateTime<Utc>> {
        match self.lifecycle {
            AgentEvidenceUploadGrantLifecycle::Pending => None,
            AgentEvidenceUploadGrantLifecycle::Completed { completed_at, .. } => Some(completed_at),
        }
    }
    pub fn document_id(&self) -> Option<DocumentId> {
        match self.lifecycle {
            AgentEvidenceUploadGrantLifecycle::Pending => None,
            AgentEvidenceUploadGrantLifecycle::Completed { document_id, .. } => Some(document_id),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum AgentEvidenceUploadGrantError {
    #[error("upload grant issuance is invalid")]
    InvalidIssuance,
    #[error("persisted upload grant is inconsistent")]
    InvalidRehydration,
    #[error("upload authority does not match the grant")]
    AuthorityMismatch,
    #[error("upload grant has expired")]
    Expired,
    #[error("upload grant is already completed")]
    AlreadyCompleted,
    #[error("upload grant completion is invalid")]
    InvalidCompletion,
    #[error("declared content type does not match the grant")]
    ContentTypeMismatch,
    #[error("declared content length does not match the grant")]
    DeclaredContentLengthMismatch,
    #[error("received content length does not match the grant")]
    ReceivedContentLengthMismatch,
    #[error("received checksum does not match the grant")]
    ChecksumMismatch,
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, TimeZone, Utc};
    use uuid::Uuid;

    use super::*;
    use crate::domain::DomainError;

    fn pending() -> AgentEvidenceUploadGrant {
        let issued_at = Utc.with_ymd_and_hms(2026, 7, 31, 12, 0, 0).unwrap();
        AgentEvidenceUploadGrant::issue(
            Uuid::new_v4().into(),
            Uuid::new_v4().into(),
            Uuid::new_v4().into(),
            Uuid::new_v4().into(),
            CoverageWindow::new(issued_at, issued_at + Duration::days(1)).unwrap(),
            AgentEvidenceUploadDeclaration::new(
                "evidence.pdf".to_owned(),
                "application/pdf".to_owned(),
                3,
                Some(hex::encode(Sha256Digest::digest(b"abc").as_bytes())),
                100,
            )
            .into_result()
            .unwrap(),
            Uuid::new_v4().into(),
            Uuid::new_v4().into(),
            issued_at,
            issued_at + Duration::minutes(5),
        )
        .unwrap()
    }

    fn authority(grant: &AgentEvidenceUploadGrant) -> AgentEvidenceUploadAuthority {
        AgentEvidenceUploadAuthority::new(
            grant.id(),
            grant.workspace_id(),
            grant.evidence_id(),
            grant.submission_id(),
            grant.issued_by_user_id(),
            grant.issued_via_agent_connection_id(),
            grant.expires_at(),
        )
    }

    #[test]
    fn declaration_accepts_valid_metadata() {
        let declaration = AgentEvidenceUploadDeclaration::new(
            "access-review.pdf".to_owned(),
            "application/pdf".to_owned(),
            483_920,
            Some("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad".to_owned()),
            25 * 1024 * 1024,
        )
        .into_result()
        .unwrap();

        assert_eq!(declaration.filename(), "access-review.pdf");
        assert_eq!(declaration.content_type(), "application/pdf");
        assert_eq!(declaration.expected_content_length(), 483_920);
        assert!(declaration.expected_sha256().is_some());
    }

    #[test]
    fn declaration_rejects_invalid_metadata() {
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
    fn declaration_debug_redacts_file_metadata() {
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

    #[test]
    fn issuance_creates_a_pending_grant_with_immutable_identity() {
        let grant = pending();
        assert!(grant.completed_at().is_none());
        assert!(grant.document_id().is_none());
        assert!(grant.ensure_pending_at(grant.issued_at()).is_ok());
    }

    #[test]
    fn authority_binding_requires_every_value_to_match_exactly() {
        let grant = pending();
        assert!(grant.matches_authority(&authority(&grant)).is_ok());
        let wrong = AgentEvidenceUploadAuthority::new(
            grant.id(),
            Uuid::new_v4().into(),
            grant.evidence_id(),
            grant.submission_id(),
            grant.issued_by_user_id(),
            grant.issued_via_agent_connection_id(),
            grant.expires_at(),
        );
        assert_eq!(
            grant.matches_authority(&wrong),
            Err(AgentEvidenceUploadGrantError::AuthorityMismatch)
        );
    }

    #[test]
    fn pending_eligibility_expires_at_the_boundary() {
        let grant = pending();
        assert!(grant
            .ensure_pending_at(grant.expires_at() - Duration::milliseconds(1))
            .is_ok());
        assert_eq!(
            grant.ensure_pending_at(grant.expires_at()),
            Err(AgentEvidenceUploadGrantError::Expired)
        );
    }

    #[test]
    fn declared_and_received_file_rules_are_owned_by_the_grant() {
        let grant = pending();
        assert!(grant.validate_declared_file("application/pdf", 3).is_ok());
        assert_eq!(
            grant.validate_declared_file("text/plain", 3),
            Err(AgentEvidenceUploadGrantError::ContentTypeMismatch)
        );
        assert_eq!(
            grant.validate_declared_file("application/pdf", 2),
            Err(AgentEvidenceUploadGrantError::DeclaredContentLengthMismatch)
        );
        assert!(grant
            .validate_staged_file(3, &hex::encode(Sha256Digest::digest(b"abc").as_bytes()))
            .is_ok());
        assert_eq!(
            grant.validate_staged_file(2, "ignored"),
            Err(AgentEvidenceUploadGrantError::ReceivedContentLengthMismatch)
        );
        assert_eq!(
            grant.validate_staged_file(3, &hex::encode(Sha256Digest::digest(b"xyz").as_bytes())),
            Err(AgentEvidenceUploadGrantError::ChecksumMismatch)
        );
    }

    #[test]
    fn completion_is_one_way_and_records_the_document_and_time() {
        let mut grant = pending();
        let document_id = DocumentId::from(Uuid::new_v4());
        let completed_at = grant.issued_at() + Duration::minutes(1);
        grant.complete(document_id, completed_at).unwrap();
        assert_eq!(grant.document_id(), Some(document_id));
        assert_eq!(grant.completed_at(), Some(completed_at));
        assert_eq!(
            grant.complete(Uuid::new_v4().into(), completed_at),
            Err(AgentEvidenceUploadGrantError::AlreadyCompleted)
        );
    }

    #[test]
    fn rehydration_rejects_invalid_expiry_and_partial_completion() {
        let grant = pending();
        let invalid_expiry = AgentEvidenceUploadGrant::rehydrate(
            grant.id(),
            grant.submission_id(),
            grant.workspace_id(),
            grant.evidence_id(),
            grant.coverage(),
            grant.declaration().clone(),
            grant.issued_by_user_id(),
            grant.issued_via_agent_connection_id(),
            grant.issued_at(),
            grant.issued_at(),
            None,
            None,
        );
        assert_eq!(
            invalid_expiry,
            Err(AgentEvidenceUploadGrantError::InvalidRehydration)
        );
        let partial = AgentEvidenceUploadGrant::rehydrate(
            grant.id(),
            grant.submission_id(),
            grant.workspace_id(),
            grant.evidence_id(),
            grant.coverage(),
            grant.declaration().clone(),
            grant.issued_by_user_id(),
            grant.issued_via_agent_connection_id(),
            grant.issued_at(),
            grant.expires_at(),
            Some(grant.issued_at()),
            None,
        );
        assert_eq!(
            partial,
            Err(AgentEvidenceUploadGrantError::InvalidRehydration)
        );
    }
}
