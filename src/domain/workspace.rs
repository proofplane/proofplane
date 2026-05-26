use chrono::{DateTime, Utc};

use super::ids::uuid_id;

uuid_id!(WorkspaceId);

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

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::WorkspaceId;

    #[test]
    fn workspace_id_is_uuid_value_type() {
        let uuid = Uuid::parse_str("00000000-0000-4000-8000-000000000001").unwrap();
        let id = WorkspaceId::from(uuid);

        assert_eq!(Uuid::from(id), uuid);
        assert_eq!(id, WorkspaceId::from(uuid));
    }
}
