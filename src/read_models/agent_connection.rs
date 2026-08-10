use chrono::{DateTime, Utc};

use crate::{
    authentication::AgentConnectionContext,
    domain::{AgentConnectionId, AgentConnectionStatus, UserId, WorkspaceId, WorkspacePermission},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReusableAgentConnection {
    pub id: AgentConnectionId,
    pub user_id: UserId,
    pub workspace_id: WorkspaceId,
    pub permissions: Vec<WorkspacePermission>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentConnectionAuthority {
    pub context: AgentConnectionContext,
    pub auth0_subject: String,
    pub auth0_client_id: String,
    pub resource: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserAgentConnectionSummary {
    pub id: AgentConnectionId,
    pub client_name: String,
    pub status: AgentConnectionStatus,
    pub authorized_at: DateTime<Utc>,
    pub last_used_at: Option<DateTime<Utc>>,
}
