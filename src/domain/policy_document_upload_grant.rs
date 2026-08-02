use chrono::{DateTime, Utc};

use super::{AgentConnectionId, PolicyDocumentUploadGrantId, PolicyId, UserId, WorkspaceId};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PolicyDocumentUploadGrantAuthority {
    id: PolicyDocumentUploadGrantId,
    workspace_id: WorkspaceId,
    policy_id: PolicyId,
    issued_by_user_id: UserId,
    issued_via_agent_connection_id: AgentConnectionId,
    expires_at: DateTime<Utc>,
}

impl PolicyDocumentUploadGrantAuthority {
    pub fn new(
        id: PolicyDocumentUploadGrantId,
        workspace_id: WorkspaceId,
        policy_id: PolicyId,
        issued_by_user_id: UserId,
        issued_via_agent_connection_id: AgentConnectionId,
        expires_at: DateTime<Utc>,
    ) -> Self {
        Self {
            id,
            workspace_id,
            policy_id,
            issued_by_user_id,
            issued_via_agent_connection_id,
            expires_at,
        }
    }

    pub fn id(self) -> PolicyDocumentUploadGrantId {
        self.id
    }
    pub fn workspace_id(self) -> WorkspaceId {
        self.workspace_id
    }
    pub fn policy_id(self) -> PolicyId {
        self.policy_id
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PolicyDocumentUploadGrant {
    id: PolicyDocumentUploadGrantId,
    workspace_id: WorkspaceId,
    policy_id: PolicyId,
    issued_by_user_id: UserId,
    issued_via_agent_connection_id: AgentConnectionId,
    issued_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
    redeemed_at: Option<DateTime<Utc>>,
}

impl PolicyDocumentUploadGrant {
    #[allow(clippy::too_many_arguments)]
    pub fn issue(
        id: PolicyDocumentUploadGrantId,
        workspace_id: WorkspaceId,
        policy_id: PolicyId,
        issued_by_user_id: UserId,
        issued_via_agent_connection_id: AgentConnectionId,
        issued_at: DateTime<Utc>,
        expires_at: DateTime<Utc>,
    ) -> Result<Self, PolicyDocumentUploadGrantError> {
        if expires_at <= issued_at {
            return Err(PolicyDocumentUploadGrantError::InvalidIssuance);
        }
        Ok(Self {
            id,
            workspace_id,
            policy_id,
            issued_by_user_id,
            issued_via_agent_connection_id,
            issued_at,
            expires_at,
            redeemed_at: None,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn rehydrate(
        id: PolicyDocumentUploadGrantId,
        workspace_id: WorkspaceId,
        policy_id: PolicyId,
        issued_by_user_id: UserId,
        issued_via_agent_connection_id: AgentConnectionId,
        issued_at: DateTime<Utc>,
        expires_at: DateTime<Utc>,
        redeemed_at: Option<DateTime<Utc>>,
    ) -> Result<Self, PolicyDocumentUploadGrantError> {
        let mut grant = Self::issue(
            id,
            workspace_id,
            policy_id,
            issued_by_user_id,
            issued_via_agent_connection_id,
            issued_at,
            expires_at,
        )
        .map_err(|_| PolicyDocumentUploadGrantError::InvalidRehydration)?;
        if redeemed_at.is_some_and(|redeemed_at| {
            redeemed_at < grant.issued_at || redeemed_at >= grant.expires_at
        }) {
            return Err(PolicyDocumentUploadGrantError::InvalidRehydration);
        }
        grant.redeemed_at = redeemed_at;
        Ok(grant)
    }

    pub fn redeem(
        &mut self,
        authority: PolicyDocumentUploadGrantAuthority,
        redeemed_at: DateTime<Utc>,
    ) -> Result<(), PolicyDocumentUploadGrantError> {
        self.matches_authority(authority)?;
        if self.redeemed_at.is_some() {
            return Err(PolicyDocumentUploadGrantError::AlreadyRedeemed);
        }
        if redeemed_at < self.issued_at || redeemed_at >= self.expires_at {
            return Err(PolicyDocumentUploadGrantError::Expired);
        }
        self.redeemed_at = Some(redeemed_at);
        Ok(())
    }

    fn matches_authority(
        &self,
        authority: PolicyDocumentUploadGrantAuthority,
    ) -> Result<(), PolicyDocumentUploadGrantError> {
        if self.id == authority.id
            && self.workspace_id == authority.workspace_id
            && self.policy_id == authority.policy_id
            && self.issued_by_user_id == authority.issued_by_user_id
            && self.issued_via_agent_connection_id == authority.issued_via_agent_connection_id
            && self.expires_at == authority.expires_at
        {
            Ok(())
        } else {
            Err(PolicyDocumentUploadGrantError::AuthorityMismatch)
        }
    }

    pub fn id(self) -> PolicyDocumentUploadGrantId {
        self.id
    }
    pub fn workspace_id(self) -> WorkspaceId {
        self.workspace_id
    }
    pub fn policy_id(self) -> PolicyId {
        self.policy_id
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
pub enum PolicyDocumentUploadGrantError {
    #[error("policy document upload grant issuance is invalid")]
    InvalidIssuance,
    #[error("persisted policy document upload grant is inconsistent")]
    InvalidRehydration,
    #[error("policy document upload authority does not match the grant")]
    AuthorityMismatch,
    #[error("policy document upload grant has expired")]
    Expired,
    #[error("policy document upload grant is already redeemed")]
    AlreadyRedeemed,
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, TimeZone, Utc};
    use uuid::Uuid;

    use super::*;

    fn pending() -> PolicyDocumentUploadGrant {
        let issued_at = Utc.with_ymd_and_hms(2026, 8, 3, 12, 0, 0).unwrap();
        PolicyDocumentUploadGrant::issue(
            Uuid::new_v4().into(),
            Uuid::new_v4().into(),
            Uuid::new_v4().into(),
            Uuid::new_v4().into(),
            Uuid::new_v4().into(),
            issued_at,
            issued_at + Duration::minutes(5),
        )
        .unwrap()
    }

    fn authority(grant: PolicyDocumentUploadGrant) -> PolicyDocumentUploadGrantAuthority {
        PolicyDocumentUploadGrantAuthority::new(
            grant.id(),
            grant.workspace_id(),
            grant.policy_id(),
            grant.issued_by_user_id(),
            grant.issued_via_agent_connection_id(),
            grant.expires_at(),
        )
    }

    #[test]
    fn policy_human_grant_redeems_once_before_expiry() {
        let mut grant = pending();
        let authority = authority(grant);
        let redeemed_at = grant.issued_at() + Duration::seconds(1);

        assert_eq!(grant.redeem(authority, redeemed_at), Ok(()));
        assert_eq!(grant.redeemed_at(), Some(redeemed_at));
        assert_eq!(
            grant.redeem(authority, redeemed_at + Duration::seconds(1)),
            Err(PolicyDocumentUploadGrantError::AlreadyRedeemed)
        );
        assert_eq!(grant.redeemed_at(), Some(redeemed_at));
    }

    #[test]
    fn policy_human_grant_rejects_mismatch_and_expiry_without_transition() {
        let mut grant = pending();
        let wrong = PolicyDocumentUploadGrantAuthority::new(
            Uuid::new_v4().into(),
            grant.workspace_id(),
            grant.policy_id(),
            grant.issued_by_user_id(),
            grant.issued_via_agent_connection_id(),
            grant.expires_at(),
        );
        assert_eq!(
            grant.redeem(wrong, grant.issued_at()),
            Err(PolicyDocumentUploadGrantError::AuthorityMismatch)
        );
        assert_eq!(
            grant.redeem(authority(grant), grant.expires_at()),
            Err(PolicyDocumentUploadGrantError::Expired)
        );
        assert_eq!(grant.redeemed_at(), None);
    }

    #[test]
    fn policy_human_grant_rehydration_enforces_lifecycle_boundaries() {
        let grant = pending();
        for redeemed_at in [grant.issued_at() - Duration::seconds(1), grant.expires_at()] {
            assert_eq!(
                PolicyDocumentUploadGrant::rehydrate(
                    grant.id(),
                    grant.workspace_id(),
                    grant.policy_id(),
                    grant.issued_by_user_id(),
                    grant.issued_via_agent_connection_id(),
                    grant.issued_at(),
                    grant.expires_at(),
                    Some(redeemed_at),
                ),
                Err(PolicyDocumentUploadGrantError::InvalidRehydration)
            );
        }
    }
}
