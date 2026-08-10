use chrono::{DateTime, Utc};

use super::{
    AgentConnectionId, CoverageWindow, DocumentUploadGrantId, EvidenceId, UserId, WorkspaceId,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EvidenceDocumentUploadGrantAuthority {
    id: DocumentUploadGrantId,
    workspace_id: WorkspaceId,
    evidence_id: EvidenceId,
    coverage: CoverageWindow,
    issued_by_user_id: UserId,
    issued_via_agent_connection_id: AgentConnectionId,
    expires_at: DateTime<Utc>,
}

impl EvidenceDocumentUploadGrantAuthority {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: DocumentUploadGrantId,
        workspace_id: WorkspaceId,
        evidence_id: EvidenceId,
        coverage: CoverageWindow,
        issued_by_user_id: UserId,
        issued_via_agent_connection_id: AgentConnectionId,
        expires_at: DateTime<Utc>,
    ) -> Self {
        Self {
            id,
            workspace_id,
            evidence_id,
            coverage,
            issued_by_user_id,
            issued_via_agent_connection_id,
            expires_at,
        }
    }

    pub fn id(self) -> DocumentUploadGrantId {
        self.id
    }

    pub fn workspace_id(self) -> WorkspaceId {
        self.workspace_id
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EvidenceDocumentUploadGrant {
    id: DocumentUploadGrantId,
    workspace_id: WorkspaceId,
    evidence_id: EvidenceId,
    coverage: CoverageWindow,
    issued_by_user_id: UserId,
    issued_via_agent_connection_id: AgentConnectionId,
    issued_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
    redeemed_at: Option<DateTime<Utc>>,
}

impl EvidenceDocumentUploadGrant {
    #[allow(clippy::too_many_arguments)]
    pub fn issue(
        id: DocumentUploadGrantId,
        workspace_id: WorkspaceId,
        evidence_id: EvidenceId,
        coverage: CoverageWindow,
        issued_by_user_id: UserId,
        issued_via_agent_connection_id: AgentConnectionId,
        issued_at: DateTime<Utc>,
        expires_at: DateTime<Utc>,
    ) -> Result<Self, EvidenceDocumentUploadGrantError> {
        if expires_at <= issued_at {
            return Err(EvidenceDocumentUploadGrantError::InvalidIssuance);
        }
        Ok(Self {
            id,
            workspace_id,
            evidence_id,
            coverage,
            issued_by_user_id,
            issued_via_agent_connection_id,
            issued_at,
            expires_at,
            redeemed_at: None,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn rehydrate(
        id: DocumentUploadGrantId,
        workspace_id: WorkspaceId,
        evidence_id: EvidenceId,
        coverage: CoverageWindow,
        issued_by_user_id: UserId,
        issued_via_agent_connection_id: AgentConnectionId,
        issued_at: DateTime<Utc>,
        expires_at: DateTime<Utc>,
        redeemed_at: Option<DateTime<Utc>>,
    ) -> Result<Self, EvidenceDocumentUploadGrantError> {
        let mut grant = Self::issue(
            id,
            workspace_id,
            evidence_id,
            coverage,
            issued_by_user_id,
            issued_via_agent_connection_id,
            issued_at,
            expires_at,
        )
        .map_err(|_| EvidenceDocumentUploadGrantError::InvalidRehydration)?;
        if redeemed_at.is_some_and(|redeemed_at| {
            redeemed_at < grant.issued_at || redeemed_at >= grant.expires_at
        }) {
            return Err(EvidenceDocumentUploadGrantError::InvalidRehydration);
        }
        grant.redeemed_at = redeemed_at;
        Ok(grant)
    }

    pub fn redeem(
        &mut self,
        authority: EvidenceDocumentUploadGrantAuthority,
        redeemed_at: DateTime<Utc>,
    ) -> Result<(), EvidenceDocumentUploadGrantError> {
        self.matches_authority(authority)?;
        if self.redeemed_at.is_some() {
            return Err(EvidenceDocumentUploadGrantError::AlreadyRedeemed);
        }
        if redeemed_at < self.issued_at || redeemed_at >= self.expires_at {
            return Err(EvidenceDocumentUploadGrantError::Expired);
        }
        self.redeemed_at = Some(redeemed_at);
        Ok(())
    }

    fn matches_authority(
        &self,
        authority: EvidenceDocumentUploadGrantAuthority,
    ) -> Result<(), EvidenceDocumentUploadGrantError> {
        if self.id == authority.id
            && self.workspace_id == authority.workspace_id
            && self.evidence_id == authority.evidence_id
            && self.coverage == authority.coverage
            && self.issued_by_user_id == authority.issued_by_user_id
            && self.issued_via_agent_connection_id == authority.issued_via_agent_connection_id
            && self.expires_at == authority.expires_at
        {
            Ok(())
        } else {
            Err(EvidenceDocumentUploadGrantError::AuthorityMismatch)
        }
    }

    pub fn id(self) -> DocumentUploadGrantId {
        self.id
    }
    pub fn workspace_id(self) -> WorkspaceId {
        self.workspace_id
    }
    pub fn evidence_id(self) -> EvidenceId {
        self.evidence_id
    }
    pub fn coverage(self) -> CoverageWindow {
        self.coverage
    }
    pub fn issued_by_user_id(self) -> UserId {
        self.issued_by_user_id
    }
    pub fn issued_via_agent_connection_id(self) -> AgentConnectionId {
        self.issued_via_agent_connection_id
    }
    pub fn issued_at(self) -> DateTime<Utc> {
        self.issued_at
    }
    pub fn expires_at(self) -> DateTime<Utc> {
        self.expires_at
    }
    pub fn redeemed_at(self) -> Option<DateTime<Utc>> {
        self.redeemed_at
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum EvidenceDocumentUploadGrantError {
    #[error("evidence document upload grant issuance is invalid")]
    InvalidIssuance,
    #[error("persisted evidence document upload grant is inconsistent")]
    InvalidRehydration,
    #[error("evidence document upload authority does not match the grant")]
    AuthorityMismatch,
    #[error("evidence document upload grant has expired")]
    Expired,
    #[error("evidence document upload grant is already redeemed")]
    AlreadyRedeemed,
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, TimeZone, Utc};
    use uuid::Uuid;

    use super::*;

    fn pending() -> EvidenceDocumentUploadGrant {
        let issued_at = Utc.with_ymd_and_hms(2026, 8, 3, 12, 0, 0).unwrap();
        EvidenceDocumentUploadGrant::issue(
            Uuid::new_v4().into(),
            Uuid::new_v4().into(),
            Uuid::new_v4().into(),
            CoverageWindow::new(issued_at, issued_at + Duration::days(30)).unwrap(),
            Uuid::new_v4().into(),
            Uuid::new_v4().into(),
            issued_at,
            issued_at + Duration::minutes(5),
        )
        .unwrap()
    }

    fn authority(grant: EvidenceDocumentUploadGrant) -> EvidenceDocumentUploadGrantAuthority {
        EvidenceDocumentUploadGrantAuthority::new(
            grant.id(),
            grant.workspace_id(),
            grant.evidence_id(),
            grant.coverage(),
            grant.issued_by_user_id(),
            grant.issued_via_agent_connection_id(),
            grant.expires_at(),
        )
    }

    #[test]
    fn evidence_human_grant_redeems_once_before_expiry() {
        let mut grant = pending();
        let authority = authority(grant);
        let redeemed_at = grant.issued_at() + Duration::seconds(1);

        assert_eq!(grant.redeem(authority, redeemed_at), Ok(()));
        assert_eq!(grant.redeemed_at(), Some(redeemed_at));
        assert_eq!(
            grant.redeem(authority, redeemed_at + Duration::seconds(1)),
            Err(EvidenceDocumentUploadGrantError::AlreadyRedeemed)
        );
        assert_eq!(grant.redeemed_at(), Some(redeemed_at));
    }

    #[test]
    fn evidence_human_grant_rejects_mismatch_and_expiry_without_transition() {
        let mut grant = pending();
        let wrong = EvidenceDocumentUploadGrantAuthority::new(
            Uuid::new_v4().into(),
            grant.workspace_id(),
            grant.evidence_id(),
            grant.coverage(),
            grant.issued_by_user_id(),
            grant.issued_via_agent_connection_id(),
            grant.expires_at(),
        );
        assert_eq!(
            grant.redeem(wrong, grant.issued_at()),
            Err(EvidenceDocumentUploadGrantError::AuthorityMismatch)
        );
        assert_eq!(
            grant.redeem(authority(grant), grant.expires_at()),
            Err(EvidenceDocumentUploadGrantError::Expired)
        );
        assert_eq!(grant.redeemed_at(), None);
    }

    #[test]
    fn evidence_human_grant_rehydration_enforces_lifecycle_boundaries() {
        let grant = pending();
        for redeemed_at in [grant.issued_at() - Duration::seconds(1), grant.expires_at()] {
            assert_eq!(
                EvidenceDocumentUploadGrant::rehydrate(
                    grant.id(),
                    grant.workspace_id(),
                    grant.evidence_id(),
                    grant.coverage(),
                    grant.issued_by_user_id(),
                    grant.issued_via_agent_connection_id(),
                    grant.issued_at(),
                    grant.expires_at(),
                    Some(redeemed_at),
                ),
                Err(EvidenceDocumentUploadGrantError::InvalidRehydration)
            );
        }
    }
}
