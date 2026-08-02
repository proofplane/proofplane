use chrono::{DateTime, Utc};

use super::{
    ids::uuid_id, AgentConnectionId, DeclaredUploadFile, DeclaredUploadFileError, DocumentId,
    PolicyId, UserId, WorkspaceId,
};

uuid_id!(AgentPolicyDocumentUploadGrantId);

pub type AgentPolicyDocumentUploadDeclaration = DeclaredUploadFile;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AgentPolicyDocumentUploadAuthority {
    upload_id: AgentPolicyDocumentUploadGrantId,
    workspace_id: WorkspaceId,
    policy_id: PolicyId,
    issued_by_user_id: UserId,
    issued_via_agent_connection_id: AgentConnectionId,
    expires_at: DateTime<Utc>,
}

impl AgentPolicyDocumentUploadAuthority {
    pub fn new(
        upload_id: AgentPolicyDocumentUploadGrantId,
        workspace_id: WorkspaceId,
        policy_id: PolicyId,
        issued_by_user_id: UserId,
        issued_via_agent_connection_id: AgentConnectionId,
        expires_at: DateTime<Utc>,
    ) -> Self {
        Self {
            upload_id,
            workspace_id,
            policy_id,
            issued_by_user_id,
            issued_via_agent_connection_id,
            expires_at,
        }
    }

    pub fn upload_id(&self) -> AgentPolicyDocumentUploadGrantId {
        self.upload_id
    }

    pub fn workspace_id(&self) -> WorkspaceId {
        self.workspace_id
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Lifecycle {
    Pending,
    Completed {
        document_id: DocumentId,
        completed_at: DateTime<Utc>,
    },
}

#[derive(Debug, PartialEq, Eq)]
pub struct AgentPolicyDocumentUploadGrant {
    id: AgentPolicyDocumentUploadGrantId,
    workspace_id: WorkspaceId,
    policy_id: PolicyId,
    declaration: AgentPolicyDocumentUploadDeclaration,
    issued_by_user_id: UserId,
    issued_via_agent_connection_id: AgentConnectionId,
    issued_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
    lifecycle: Lifecycle,
}

impl AgentPolicyDocumentUploadGrant {
    #[allow(clippy::too_many_arguments)]
    pub fn issue(
        id: AgentPolicyDocumentUploadGrantId,
        workspace_id: WorkspaceId,
        policy_id: PolicyId,
        declaration: AgentPolicyDocumentUploadDeclaration,
        issued_by_user_id: UserId,
        issued_via_agent_connection_id: AgentConnectionId,
        issued_at: DateTime<Utc>,
        expires_at: DateTime<Utc>,
    ) -> Result<Self, AgentPolicyDocumentUploadGrantError> {
        if expires_at <= issued_at {
            return Err(AgentPolicyDocumentUploadGrantError::InvalidIssuance);
        }
        Ok(Self {
            id,
            workspace_id,
            policy_id,
            declaration,
            issued_by_user_id,
            issued_via_agent_connection_id,
            issued_at,
            expires_at,
            lifecycle: Lifecycle::Pending,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn rehydrate(
        id: AgentPolicyDocumentUploadGrantId,
        workspace_id: WorkspaceId,
        policy_id: PolicyId,
        declaration: AgentPolicyDocumentUploadDeclaration,
        issued_by_user_id: UserId,
        issued_via_agent_connection_id: AgentConnectionId,
        issued_at: DateTime<Utc>,
        expires_at: DateTime<Utc>,
        completed_at: Option<DateTime<Utc>>,
        document_id: Option<DocumentId>,
    ) -> Result<Self, AgentPolicyDocumentUploadGrantError> {
        let mut grant = Self::issue(
            id,
            workspace_id,
            policy_id,
            declaration,
            issued_by_user_id,
            issued_via_agent_connection_id,
            issued_at,
            expires_at,
        )
        .map_err(|_| AgentPolicyDocumentUploadGrantError::InvalidRehydration)?;
        grant.lifecycle = match (completed_at, document_id) {
            (None, None) => Lifecycle::Pending,
            (Some(completed_at), Some(document_id))
                if completed_at >= issued_at && completed_at < expires_at =>
            {
                Lifecycle::Completed {
                    document_id,
                    completed_at,
                }
            }
            _ => return Err(AgentPolicyDocumentUploadGrantError::InvalidRehydration),
        };
        Ok(grant)
    }

    pub fn matches_authority(
        &self,
        authority: &AgentPolicyDocumentUploadAuthority,
    ) -> Result<(), AgentPolicyDocumentUploadGrantError> {
        if self.id == authority.upload_id
            && self.workspace_id == authority.workspace_id
            && self.policy_id == authority.policy_id
            && self.issued_by_user_id == authority.issued_by_user_id
            && self.issued_via_agent_connection_id == authority.issued_via_agent_connection_id
            && self.expires_at == authority.expires_at
        {
            Ok(())
        } else {
            Err(AgentPolicyDocumentUploadGrantError::AuthorityMismatch)
        }
    }

    pub fn ensure_pending_at(
        &self,
        now: DateTime<Utc>,
    ) -> Result<(), AgentPolicyDocumentUploadGrantError> {
        if self.completed_document_at(now)?.is_some() {
            Err(AgentPolicyDocumentUploadGrantError::AlreadyCompleted)
        } else {
            Ok(())
        }
    }

    pub fn completed_document_at(
        &self,
        now: DateTime<Utc>,
    ) -> Result<Option<DocumentId>, AgentPolicyDocumentUploadGrantError> {
        if now >= self.expires_at {
            return Err(AgentPolicyDocumentUploadGrantError::Expired);
        }
        Ok(self.document_id())
    }

    pub fn validate_declared_file(
        &self,
        content_type: &str,
        content_length: u64,
    ) -> Result<(), AgentPolicyDocumentUploadGrantError> {
        self.declaration
            .validate_declared(content_type, content_length)
            .map_err(AgentPolicyDocumentUploadGrantError::from)
    }

    pub fn validate_staged_file(
        &self,
        content_length: i64,
        checksum_sha256: &str,
    ) -> Result<(), AgentPolicyDocumentUploadGrantError> {
        self.declaration
            .validate_staged(content_length, checksum_sha256)
            .map_err(AgentPolicyDocumentUploadGrantError::from)
    }

    pub fn complete(
        &mut self,
        document_id: DocumentId,
        completed_at: DateTime<Utc>,
    ) -> Result<(), AgentPolicyDocumentUploadGrantError> {
        self.ensure_pending_at(completed_at)?;
        if completed_at < self.issued_at {
            return Err(AgentPolicyDocumentUploadGrantError::InvalidCompletion);
        }
        self.lifecycle = Lifecycle::Completed {
            document_id,
            completed_at,
        };
        Ok(())
    }

    pub fn id(&self) -> AgentPolicyDocumentUploadGrantId {
        self.id
    }
    pub fn workspace_id(&self) -> WorkspaceId {
        self.workspace_id
    }
    pub fn policy_id(&self) -> PolicyId {
        self.policy_id
    }
    pub fn declaration(&self) -> &AgentPolicyDocumentUploadDeclaration {
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
            Lifecycle::Pending => None,
            Lifecycle::Completed { completed_at, .. } => Some(completed_at),
        }
    }
    pub fn document_id(&self) -> Option<DocumentId> {
        match self.lifecycle {
            Lifecycle::Pending => None,
            Lifecycle::Completed { document_id, .. } => Some(document_id),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum AgentPolicyDocumentUploadGrantError {
    #[error("policy document upload grant issuance is invalid")]
    InvalidIssuance,
    #[error("persisted policy document upload grant is inconsistent")]
    InvalidRehydration,
    #[error("policy document upload authority does not match the grant")]
    AuthorityMismatch,
    #[error("policy document upload grant has expired")]
    Expired,
    #[error("policy document upload grant is already completed")]
    AlreadyCompleted,
    #[error("policy document upload grant completion is invalid")]
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

impl From<DeclaredUploadFileError> for AgentPolicyDocumentUploadGrantError {
    fn from(error: DeclaredUploadFileError) -> Self {
        match error {
            DeclaredUploadFileError::InvalidRehydration => Self::InvalidRehydration,
            DeclaredUploadFileError::ContentTypeMismatch => Self::ContentTypeMismatch,
            DeclaredUploadFileError::DeclaredContentLengthMismatch => {
                Self::DeclaredContentLengthMismatch
            }
            DeclaredUploadFileError::ReceivedContentLengthMismatch => {
                Self::ReceivedContentLengthMismatch
            }
            DeclaredUploadFileError::ChecksumMismatch => Self::ChecksumMismatch,
        }
    }
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, TimeZone, Utc};
    use uuid::Uuid;

    use super::*;
    use crate::domain::{AgentConnectionId, DeclaredUploadFile, PolicyId, UserId, WorkspaceId};

    #[test]
    fn policy_machine_grant_issues_for_one_declared_file() {
        let issued_at = Utc.with_ymd_and_hms(2026, 8, 1, 12, 0, 0).unwrap();
        let declaration = DeclaredUploadFile::new(
            "policy.pdf".to_owned(),
            "application/pdf".to_owned(),
            42,
            None,
            1024,
        )
        .into_result()
        .unwrap();
        let grant = AgentPolicyDocumentUploadGrant::issue(
            Uuid::new_v4().into(),
            WorkspaceId::from(Uuid::new_v4()),
            PolicyId::from(Uuid::new_v4()),
            declaration.clone(),
            UserId::from(Uuid::new_v4()),
            AgentConnectionId::from(Uuid::new_v4()),
            issued_at,
            issued_at + Duration::minutes(5),
        )
        .unwrap();

        assert_eq!(grant.declaration(), &declaration);
        assert!(grant.completed_at().is_none());
        assert!(grant.document_id().is_none());
    }

    #[test]
    fn policy_machine_grant_requires_exact_authority_binding() {
        let grant = pending();
        let authority = AgentPolicyDocumentUploadAuthority::new(
            grant.id(),
            grant.workspace_id(),
            grant.policy_id(),
            grant.issued_by_user_id(),
            grant.issued_via_agent_connection_id(),
            grant.expires_at(),
        );
        assert_eq!(grant.matches_authority(&authority), Ok(()));

        let wrong_authorities = [
            AgentPolicyDocumentUploadAuthority::new(
                Uuid::new_v4().into(),
                grant.workspace_id(),
                grant.policy_id(),
                grant.issued_by_user_id(),
                grant.issued_via_agent_connection_id(),
                grant.expires_at(),
            ),
            AgentPolicyDocumentUploadAuthority::new(
                grant.id(),
                Uuid::new_v4().into(),
                grant.policy_id(),
                grant.issued_by_user_id(),
                grant.issued_via_agent_connection_id(),
                grant.expires_at(),
            ),
            AgentPolicyDocumentUploadAuthority::new(
                grant.id(),
                grant.workspace_id(),
                Uuid::new_v4().into(),
                grant.issued_by_user_id(),
                grant.issued_via_agent_connection_id(),
                grant.expires_at(),
            ),
            AgentPolicyDocumentUploadAuthority::new(
                grant.id(),
                grant.workspace_id(),
                grant.policy_id(),
                Uuid::new_v4().into(),
                grant.issued_via_agent_connection_id(),
                grant.expires_at(),
            ),
            AgentPolicyDocumentUploadAuthority::new(
                grant.id(),
                grant.workspace_id(),
                grant.policy_id(),
                grant.issued_by_user_id(),
                Uuid::new_v4().into(),
                grant.expires_at(),
            ),
            AgentPolicyDocumentUploadAuthority::new(
                grant.id(),
                grant.workspace_id(),
                grant.policy_id(),
                grant.issued_by_user_id(),
                grant.issued_via_agent_connection_id(),
                grant.expires_at() + Duration::seconds(1),
            ),
        ];
        for wrong in wrong_authorities {
            assert_eq!(
                grant.matches_authority(&wrong),
                Err(AgentPolicyDocumentUploadGrantError::AuthorityMismatch)
            );
        }
    }

    #[test]
    fn policy_machine_grant_expires_and_completes_only_once() {
        let mut grant = pending();
        let before_expiry = grant.expires_at() - Duration::seconds(1);
        let document_id = DocumentId::from(Uuid::new_v4());

        assert_eq!(grant.completed_document_at(before_expiry), Ok(None));
        assert_eq!(grant.complete(document_id, before_expiry), Ok(()));
        assert_eq!(
            grant.completed_document_at(before_expiry),
            Ok(Some(document_id))
        );
        assert_eq!(
            grant.complete(Uuid::new_v4().into(), before_expiry),
            Err(AgentPolicyDocumentUploadGrantError::AlreadyCompleted)
        );
        assert_eq!(
            grant.completed_document_at(grant.expires_at()),
            Err(AgentPolicyDocumentUploadGrantError::Expired)
        );
    }

    #[test]
    fn policy_machine_grant_owns_declared_and_received_file_matching() {
        let declaration = DeclaredUploadFile::new(
            "policy.pdf".to_owned(),
            "application/pdf".to_owned(),
            3,
            Some("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad".to_owned()),
            1024,
        )
        .into_result()
        .unwrap();
        let grant = pending_with(declaration);

        assert_eq!(
            grant.validate_declared_file("text/plain", 3),
            Err(AgentPolicyDocumentUploadGrantError::ContentTypeMismatch)
        );
        assert_eq!(
            grant.validate_declared_file("application/pdf", 4),
            Err(AgentPolicyDocumentUploadGrantError::DeclaredContentLengthMismatch)
        );
        assert_eq!(
            grant.validate_staged_file(
                4,
                "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
            ),
            Err(AgentPolicyDocumentUploadGrantError::ReceivedContentLengthMismatch)
        );
        assert_eq!(
            grant.validate_staged_file(
                3,
                "3608bca1e44ea6c4d268eb6db02260269892c0b42b86bbf1e77a6fa16c3c9282",
            ),
            Err(AgentPolicyDocumentUploadGrantError::ChecksumMismatch)
        );
    }

    #[test]
    fn policy_machine_grant_rehydration_rejects_inconsistent_state() {
        let grant = pending();
        assert_eq!(
            AgentPolicyDocumentUploadGrant::rehydrate(
                grant.id(),
                grant.workspace_id(),
                grant.policy_id(),
                grant.declaration().clone(),
                grant.issued_by_user_id(),
                grant.issued_via_agent_connection_id(),
                grant.issued_at(),
                grant.expires_at(),
                Some(grant.issued_at()),
                None,
            ),
            Err(AgentPolicyDocumentUploadGrantError::InvalidRehydration)
        );
        assert_eq!(
            AgentPolicyDocumentUploadGrant::rehydrate(
                grant.id(),
                grant.workspace_id(),
                grant.policy_id(),
                grant.declaration().clone(),
                grant.issued_by_user_id(),
                grant.issued_via_agent_connection_id(),
                grant.issued_at(),
                grant.issued_at(),
                None,
                None,
            ),
            Err(AgentPolicyDocumentUploadGrantError::InvalidRehydration)
        );
        for completed_at in [
            grant.issued_at() - Duration::milliseconds(1),
            grant.expires_at(),
            grant.expires_at() + Duration::milliseconds(1),
        ] {
            assert_eq!(
                AgentPolicyDocumentUploadGrant::rehydrate(
                    grant.id(),
                    grant.workspace_id(),
                    grant.policy_id(),
                    grant.declaration().clone(),
                    grant.issued_by_user_id(),
                    grant.issued_via_agent_connection_id(),
                    grant.issued_at(),
                    grant.expires_at(),
                    Some(completed_at),
                    Some(Uuid::new_v4().into()),
                ),
                Err(AgentPolicyDocumentUploadGrantError::InvalidRehydration)
            );
        }
    }

    fn pending() -> AgentPolicyDocumentUploadGrant {
        pending_with(
            DeclaredUploadFile::new(
                "policy.pdf".to_owned(),
                "application/pdf".to_owned(),
                42,
                None,
                1024,
            )
            .into_result()
            .unwrap(),
        )
    }

    fn pending_with(declaration: DeclaredUploadFile) -> AgentPolicyDocumentUploadGrant {
        let issued_at = Utc.with_ymd_and_hms(2026, 8, 1, 12, 0, 0).unwrap();
        AgentPolicyDocumentUploadGrant::issue(
            Uuid::new_v4().into(),
            WorkspaceId::from(Uuid::new_v4()),
            PolicyId::from(Uuid::new_v4()),
            declaration,
            UserId::from(Uuid::new_v4()),
            AgentConnectionId::from(Uuid::new_v4()),
            issued_at,
            issued_at + Duration::minutes(5),
        )
        .unwrap()
    }
}
