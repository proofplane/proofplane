use chrono::{DateTime, Utc};

use super::WorkspaceId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Workspace {
    pub id: WorkspaceId,
    // TODO: settle workspace identity. Slug should be the ID when tenant
    // isolation moves to domain names.
    pub slug: Option<String>,
    pub name: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateWorkspacePayload {
    pub id: Option<WorkspaceId>,
    pub slug: Option<String>,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateWorkspacePayload {
    pub slug: Option<String>,
    pub name: String,
}
