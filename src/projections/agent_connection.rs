use chrono::{DateTime, Utc};

use crate::domain::{AgentConnectionId, AgentConnectionStatus};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserAgentConnectionSummary {
    pub id: AgentConnectionId,
    pub client_name: String,
    pub status: AgentConnectionStatus,
    pub authorized_at: DateTime<Utc>,
    pub last_used_at: Option<DateTime<Utc>>,
}
