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

/**
 * Workspace is the tenant boundary. Most things are basically scoped
 * to workspaces, excepting global things like frameworks which are universal.
 */
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Workspace {
    id: WorkspaceId,
    // TODO: settle workspace identity. Slug should be the ID when tenant
    // isolation moves to domain names.
    slug: Option<String>,
    name: String,
    created_at: DateTime<Utc>,
    memberships: Vec<WorkspaceMembership>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum WorkspaceError {
    #[error("workspace membership snapshot is inconsistent")]
    InvalidMemberships,
    #[error("the user already belongs to the workspace")]
    AlreadyMember,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum WorkspaceMemberError {
    #[error("workspace membership is unavailable")]
    Unavailable,
    #[error("workspace member not found")]
    NotFound,
    #[error("the workspace must retain at least one owner")]
    LastOwner,
}

impl Workspace {
    pub fn create_owned(
        id: WorkspaceId,
        slug: Option<String>,
        name: String,
        created_at: DateTime<Utc>,
        owner_user_id: UserId,
    ) -> Self {
        let owner = WorkspaceMembership {
            user_id: owner_user_id,
            workspace_id: id,
            role: WorkspaceRole::Owner,
            created_at,
        };
        Self {
            id,
            slug,
            name,
            created_at,
            memberships: vec![owner],
        }
    }

    pub fn rehydrate(
        id: WorkspaceId,
        slug: Option<String>,
        name: String,
        created_at: DateTime<Utc>,
        memberships: Vec<WorkspaceMembership>,
    ) -> Result<Self, WorkspaceError> {
        let scoped = memberships
            .iter()
            .all(|membership| membership.workspace_id == id);
        let unique_users = memberships
            .iter()
            .map(|membership| membership.user_id)
            .collect::<std::collections::HashSet<_>>()
            .len()
            == memberships.len();
        let has_owner = memberships
            .iter()
            .any(|membership| membership.role == WorkspaceRole::Owner);
        if !scoped || !unique_users || !has_owner {
            return Err(WorkspaceError::InvalidMemberships);
        }
        Ok(Self {
            id,
            slug,
            name,
            created_at,
            memberships,
        })
    }

    pub fn id(&self) -> WorkspaceId {
        self.id
    }

    pub fn slug(&self) -> Option<&str> {
        self.slug.as_deref()
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn created_at(&self) -> DateTime<Utc> {
        self.created_at
    }

    pub fn memberships(&self) -> &[WorkspaceMembership] {
        &self.memberships
    }

    pub fn role_for(&self, user_id: UserId) -> Option<WorkspaceRole> {
        self.memberships
            .iter()
            .find(|membership| membership.user_id == user_id)
            .map(|membership| membership.role)
    }

    pub fn add_member(
        &mut self,
        user_id: UserId,
        role: WorkspaceRole,
        created_at: DateTime<Utc>,
    ) -> Result<(), WorkspaceError> {
        if self.role_for(user_id).is_some() {
            return Err(WorkspaceError::AlreadyMember);
        }
        self.memberships.push(WorkspaceMembership {
            user_id,
            workspace_id: self.id,
            role,
            created_at,
        });
        Ok(())
    }

    pub fn remove_member(
        &mut self,
        actor_user_id: UserId,
        target_user_id: UserId,
    ) -> Result<(), WorkspaceMemberError> {
        if !matches!(
            self.role_for(actor_user_id),
            Some(WorkspaceRole::Owner | WorkspaceRole::Admin)
        ) {
            return Err(WorkspaceMemberError::Unavailable);
        }
        let Some(index) = self
            .memberships
            .iter()
            .position(|membership| membership.user_id == target_user_id)
        else {
            return Err(WorkspaceMemberError::NotFound);
        };
        if self.memberships[index].role == WorkspaceRole::Owner
            && self
                .memberships
                .iter()
                .filter(|membership| membership.role == WorkspaceRole::Owner)
                .count()
                == 1
        {
            return Err(WorkspaceMemberError::LastOwner);
        }
        self.memberships.remove(index);
        Ok(())
    }
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
    use chrono::{TimeZone, Utc};
    use uuid::Uuid;

    use super::{Workspace, WorkspaceId, WorkspaceMemberError, WorkspaceRole};
    use crate::domain::UserId;

    #[test]
    fn workspace_id_is_uuid_value_type() {
        let uuid = Uuid::parse_str("00000000-0000-4000-8000-000000000001").unwrap();
        let id = WorkspaceId::from(uuid);

        assert_eq!(Uuid::from(id), uuid);
        assert_eq!(id, WorkspaceId::from(uuid));
    }

    #[test]
    fn aggregate_rejects_last_owner_removal_without_mutating_memberships() {
        let owner_id = UserId::from(Uuid::new_v4());
        let mut workspace = owned_workspace(owner_id);
        let before = workspace.clone();

        assert_eq!(
            workspace.remove_member(owner_id, owner_id),
            Err(WorkspaceMemberError::LastOwner)
        );
        assert_eq!(workspace, before);
    }

    #[test]
    fn aggregate_conceals_unauthorized_and_cross_workspace_removals_without_mutation() {
        let owner_id = UserId::from(Uuid::new_v4());
        let outsider_id = UserId::from(Uuid::new_v4());
        let mut workspace = owned_workspace(owner_id);
        let before = workspace.clone();

        assert_eq!(
            workspace.remove_member(outsider_id, owner_id),
            Err(WorkspaceMemberError::Unavailable)
        );
        assert_eq!(
            workspace.remove_member(owner_id, outsider_id),
            Err(WorkspaceMemberError::NotFound)
        );
        assert_eq!(workspace, before);
    }

    #[test]
    fn aggregate_removes_a_member_once_and_replay_is_state_idempotent() {
        let owner_id = UserId::from(Uuid::new_v4());
        let admin_id = UserId::from(Uuid::new_v4());
        let mut workspace = owned_workspace(owner_id);
        workspace
            .add_member(admin_id, WorkspaceRole::Admin, timestamp())
            .unwrap();

        workspace.remove_member(owner_id, admin_id).unwrap();
        let after = workspace.clone();
        assert_eq!(
            workspace.remove_member(owner_id, admin_id),
            Err(WorkspaceMemberError::NotFound)
        );
        assert_eq!(workspace, after);
    }

    fn owned_workspace(owner_id: UserId) -> Workspace {
        Workspace::create_owned(
            WorkspaceId::from(Uuid::new_v4()),
            Some("workspace".to_owned()),
            "Workspace".to_owned(),
            timestamp(),
            owner_id,
        )
    }

    fn timestamp() -> chrono::DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 8, 2, 12, 0, 0).single().unwrap()
    }
}
