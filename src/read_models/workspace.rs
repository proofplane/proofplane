use chrono::{DateTime, Utc};

use crate::domain::{WorkspaceId, WorkspaceRole};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceDetails {
    pub id: WorkspaceId,
    pub slug: Option<String>,
    pub name: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceWithRole {
    pub workspace: WorkspaceDetails,
    pub role: WorkspaceRole,
}
