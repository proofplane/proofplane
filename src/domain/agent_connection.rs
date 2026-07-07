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
