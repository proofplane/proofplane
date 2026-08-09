use chrono::{DateTime, Utc};

use super::{ids::uuid_id, Sha256Digest, UserId, WorkspaceId, WorkspacePermission};

uuid_id!(AgentConnectionId);
uuid_id!(AgentAuthorizationTransactionId);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentConnectionStatus {
    Pending,
    Authorized,
    Active,
    Revoked,
}

impl AgentConnectionStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Authorized => "authorized",
            Self::Active => "active",
            Self::Revoked => "revoked",
        }
    }
}

impl std::str::FromStr for AgentConnectionStatus {
    type Err = ();
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "pending" => Ok(Self::Pending),
            "authorized" => Ok(Self::Authorized),
            "active" => Ok(Self::Active),
            "revoked" => Ok(Self::Revoked),
            _ => Err(()),
        }
    }
}

/// The one-shot continuation is part of the connection snapshot. Its digests
/// are deliberately never exposed by application read models.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentAuthorizationTransaction {
    pub id: AgentAuthorizationTransactionId,
    continuation_digest: Sha256Digest,
    nonce_digest: Sha256Digest,
    pub consumed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentConnection {
    pub id: AgentConnectionId,
    pub user_id: UserId,
    pub workspace_id: WorkspaceId,
    pub auth0_subject: String,
    pub auth0_client_id: String,
    pub client_display_name: String,
    pub resource: String,
    pub status: AgentConnectionStatus,
    pub permissions: Vec<WorkspacePermission>,
    pub pending_expires_at: DateTime<Utc>,
    pub activated_at: Option<DateTime<Utc>>,
    pub last_used_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    authorization: AgentAuthorizationTransaction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentConnectionConsumption {
    Authorized,
    Unavailable,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentConnectionActivation {
    Activated,
    AlreadyActive,
    Unavailable,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentConnectionUse {
    Used,
    Unavailable,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentConnectionRevocation {
    Revoked,
    AlreadyRevoked,
}

#[allow(
    clippy::enum_variant_names,
    reason = "the invalidity category is part of the domain vocabulary"
)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum AgentConnectionLifecycleError {
    #[error("agent connection request is invalid")]
    InvalidRequest,
    #[error("persisted agent connection is inconsistent")]
    InvalidRehydration,
    #[error("agent connection lifecycle transition is invalid")]
    InvalidTransition,
}

impl AgentConnection {
    #[allow(clippy::too_many_arguments)]
    pub fn request(
        id: AgentConnectionId,
        transaction_id: AgentAuthorizationTransactionId,
        user_id: UserId,
        workspace_id: WorkspaceId,
        auth0_subject: String,
        auth0_client_id: String,
        client_display_name: String,
        resource: String,
        permissions: Vec<WorkspacePermission>,
        pending_expires_at: DateTime<Utc>,
        continuation_digest: Sha256Digest,
        nonce_digest: Sha256Digest,
        created_at: DateTime<Utc>,
    ) -> Result<Self, AgentConnectionLifecycleError> {
        if pending_expires_at <= created_at
            || permissions.is_empty()
            || auth0_subject.trim().is_empty()
            || auth0_client_id.trim().is_empty()
            || client_display_name.trim().is_empty()
            || resource.trim().is_empty()
        {
            return Err(AgentConnectionLifecycleError::InvalidRequest);
        }
        Ok(Self {
            id,
            user_id,
            workspace_id,
            auth0_subject,
            auth0_client_id,
            client_display_name,
            resource,
            status: AgentConnectionStatus::Pending,
            permissions,
            pending_expires_at,
            activated_at: None,
            last_used_at: None,
            revoked_at: None,
            created_at,
            authorization: AgentAuthorizationTransaction {
                id: transaction_id,
                continuation_digest,
                nonce_digest,
                consumed_at: None,
            },
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn rehydrate(
        id: AgentConnectionId,
        user_id: UserId,
        workspace_id: WorkspaceId,
        auth0_subject: String,
        auth0_client_id: String,
        client_display_name: String,
        resource: String,
        status: AgentConnectionStatus,
        permissions: Vec<WorkspacePermission>,
        pending_expires_at: DateTime<Utc>,
        activated_at: Option<DateTime<Utc>>,
        last_used_at: Option<DateTime<Utc>>,
        revoked_at: Option<DateTime<Utc>>,
        created_at: DateTime<Utc>,
        transaction_id: AgentAuthorizationTransactionId,
        continuation_digest: Sha256Digest,
        nonce_digest: Sha256Digest,
        consumed_at: Option<DateTime<Utc>>,
    ) -> Result<Self, AgentConnectionLifecycleError> {
        let mut connection = Self::request(
            id,
            transaction_id,
            user_id,
            workspace_id,
            auth0_subject,
            auth0_client_id,
            client_display_name,
            resource,
            permissions,
            pending_expires_at,
            continuation_digest,
            nonce_digest,
            created_at,
        )
        .map_err(|_| AgentConnectionLifecycleError::InvalidRehydration)?;
        let valid = match status {
            AgentConnectionStatus::Pending => {
                activated_at.is_none() && revoked_at.is_none() && consumed_at.is_none()
            }
            AgentConnectionStatus::Authorized => {
                activated_at.is_none() && revoked_at.is_none() && consumed_at.is_some()
            }
            AgentConnectionStatus::Active => {
                activated_at.is_some() && revoked_at.is_none() && consumed_at.is_some()
            }
            AgentConnectionStatus::Revoked => revoked_at.is_some(),
        };
        if !valid
            || activated_at.is_some_and(|at| at < created_at)
            || last_used_at.is_some_and(|at| at < created_at)
            || revoked_at.is_some_and(|at| at < created_at)
            || consumed_at.is_some_and(|at| at < created_at)
        {
            return Err(AgentConnectionLifecycleError::InvalidRehydration);
        }
        connection.status = status;
        connection.activated_at = activated_at;
        connection.last_used_at = last_used_at;
        connection.revoked_at = revoked_at;
        connection.authorization.consumed_at = consumed_at;
        Ok(connection)
    }

    pub fn consume_continuation(
        &mut self,
        continuation: Sha256Digest,
        nonce: Sha256Digest,
        now: DateTime<Utc>,
    ) -> AgentConnectionConsumption {
        if self.status != AgentConnectionStatus::Pending
            || now >= self.pending_expires_at
            || self.authorization.continuation_digest != continuation
            || self.authorization.nonce_digest != nonce
        {
            return AgentConnectionConsumption::Unavailable;
        }
        self.status = AgentConnectionStatus::Authorized;
        self.authorization.consumed_at = Some(now);
        AgentConnectionConsumption::Authorized
    }

    pub fn activate(&mut self, now: DateTime<Utc>) -> AgentConnectionActivation {
        match self.status {
            AgentConnectionStatus::Active => AgentConnectionActivation::AlreadyActive,
            AgentConnectionStatus::Authorized if now < self.pending_expires_at => {
                self.status = AgentConnectionStatus::Active;
                self.activated_at = Some(now);
                AgentConnectionActivation::Activated
            }
            _ => AgentConnectionActivation::Unavailable,
        }
    }

    pub fn use_at(&mut self, now: DateTime<Utc>) -> AgentConnectionUse {
        if self.status != AgentConnectionStatus::Active {
            return AgentConnectionUse::Unavailable;
        }
        self.last_used_at = Some(now);
        AgentConnectionUse::Used
    }

    pub fn revoke(
        &mut self,
        now: DateTime<Utc>,
    ) -> Result<AgentConnectionRevocation, AgentConnectionLifecycleError> {
        if self.status == AgentConnectionStatus::Revoked {
            return Ok(AgentConnectionRevocation::AlreadyRevoked);
        }
        if now < self.created_at {
            return Err(AgentConnectionLifecycleError::InvalidTransition);
        }
        self.status = AgentConnectionStatus::Revoked;
        self.revoked_at = Some(now);
        Ok(AgentConnectionRevocation::Revoked)
    }

    pub fn continuation_digest(&self) -> Sha256Digest {
        self.authorization.continuation_digest
    }
    pub fn nonce_digest(&self) -> Sha256Digest {
        self.authorization.nonce_digest
    }
    pub fn authorization_transaction_id(&self) -> AgentAuthorizationTransactionId {
        self.authorization.id
    }
    pub fn continuation_consumed_at(&self) -> Option<DateTime<Utc>> {
        self.authorization.consumed_at
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewPendingAgentConnection {
    pub id: AgentConnectionId,
    pub transaction_id: AgentAuthorizationTransactionId,
    pub user_id: UserId,
    pub workspace_id: WorkspaceId,
    pub auth0_subject: String,
    pub auth0_client_id: String,
    pub client_display_name: String,
    pub resource: String,
    pub permissions: Vec<WorkspacePermission>,
    pub pending_expires_at: DateTime<Utc>,
    pub continuation_digest: Sha256Digest,
    pub nonce_digest: Sha256Digest,
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, TimeZone, Utc};
    use uuid::Uuid;
    fn pending() -> AgentConnection {
        let now = Utc.with_ymd_and_hms(2026, 8, 9, 12, 0, 0).unwrap();
        AgentConnection::request(
            Uuid::new_v4().into(),
            Uuid::new_v4().into(),
            Uuid::new_v4().into(),
            Uuid::new_v4().into(),
            "sub".into(),
            "client".into(),
            "Client".into(),
            "resource".into(),
            vec![WorkspacePermission::ReadEvidence],
            now + Duration::minutes(1),
            Sha256Digest::digest(b"continuation"),
            Sha256Digest::digest(b"nonce"),
            now,
        )
        .unwrap()
    }
    #[test]
    fn continuation_is_one_shot_and_expires_at_the_boundary() {
        let mut connection = pending();
        let now = connection.created_at;
        assert_eq!(
            connection.consume_continuation(
                Sha256Digest::digest(b"continuation"),
                Sha256Digest::digest(b"nonce"),
                now
            ),
            AgentConnectionConsumption::Authorized
        );
        assert_eq!(
            connection.consume_continuation(
                Sha256Digest::digest(b"continuation"),
                Sha256Digest::digest(b"nonce"),
                now
            ),
            AgentConnectionConsumption::Unavailable
        );
        let mut expired = pending();
        assert_eq!(
            expired.consume_continuation(
                Sha256Digest::digest(b"continuation"),
                Sha256Digest::digest(b"nonce"),
                expired.pending_expires_at
            ),
            AgentConnectionConsumption::Unavailable
        );
    }
    #[test]
    fn lifecycle_activates_uses_and_revokes_without_revival() {
        let mut connection = pending();
        let now = connection.created_at;
        connection.consume_continuation(
            Sha256Digest::digest(b"continuation"),
            Sha256Digest::digest(b"nonce"),
            now,
        );
        assert_eq!(
            connection.activate(now),
            AgentConnectionActivation::Activated
        );
        assert_eq!(connection.use_at(now), AgentConnectionUse::Used);
        assert_eq!(
            connection.revoke(now),
            Ok(AgentConnectionRevocation::Revoked)
        );
        assert_eq!(
            connection.activate(now),
            AgentConnectionActivation::Unavailable
        );
    }
}
