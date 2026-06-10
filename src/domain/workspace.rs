use std::str::FromStr;

use chrono::{DateTime, Utc};

use super::ids::uuid_id;
use super::UserId;

uuid_id!(WorkspaceId);

/// Human management-plane role within a workspace. Owners can do everything an
/// admin can plus delete or transfer the workspace; admins manage members,
/// actors, and keys. Maps one-to-one to the `workspace_memberships.role` column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceRole {
    Owner,
    Admin,
}

impl WorkspaceRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Owner => "owner",
            Self::Admin => "admin",
        }
    }
}

impl FromStr for WorkspaceRole {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "owner" => Ok(Self::Owner),
            "admin" => Ok(Self::Admin),
            _ => Err(()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceMembership {
    pub user_id: UserId,
    pub workspace_id: WorkspaceId,
    pub role: WorkspaceRole,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceWithRole {
    pub workspace: Workspace,
    pub role: WorkspaceRole,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddMemberPayload {
    pub user_id: UserId,
    pub role: WorkspaceRole,
}

/**
 * Workspace is the tenant boundary. Most things are basically scoped
 * to workspaces, excepting global things like frameworks which are universal.
 */
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
