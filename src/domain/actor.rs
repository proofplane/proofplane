use std::{fmt, str::FromStr};

use chrono::{DateTime, Utc};

use super::{
    ids::uuid_id, ActorPermissions, DomainError, UserId, WorkspaceId, WorkspacePermission,
};

uuid_id!(ActorId);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActorKind {
    HumanUser,
    AiAgent,
    ServiceAccount,
    Integration,
    PolicyAutomation,
    System,
}

impl ActorKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::HumanUser => "human_user",
            Self::AiAgent => "ai_agent",
            Self::ServiceAccount => "service_account",
            Self::Integration => "integration",
            Self::PolicyAutomation => "policy_automation",
            Self::System => "system",
        }
    }
}

impl fmt::Display for ActorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for ActorKind {
    type Err = DomainError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "human_user" => Ok(Self::HumanUser),
            "ai_agent" => Ok(Self::AiAgent),
            "service_account" => Ok(Self::ServiceAccount),
            "integration" => Ok(Self::Integration),
            "policy_automation" => Ok(Self::PolicyAutomation),
            "system" => Ok(Self::System),
            _ => Err(DomainError::InvalidEnumValue {
                field: "actor_type",
                value: value.to_owned(),
            }),
        }
    }
}

/**
 * Actors are users of the system.
 */
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Actor {
    pub id: ActorId,
    pub kind: ActorKind,
    pub display_name: String,
    pub workspace_id: WorkspaceId,
    pub created_by_user_id: Option<UserId>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActorWithPermissions {
    pub actor: Actor,
    pub permissions: ActorPermissions,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateActorPayload {
    pub id: Option<ActorId>, // optional for tests to be able to pass in deterministic IDs
    pub kind: ActorKind,
    pub display_name: String,
    pub workspace_id: WorkspaceId,
    pub created_by_user_id: Option<UserId>,
    pub permissions: Vec<WorkspacePermission>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateActorPayload {
    pub kind: ActorKind,
    pub display_name: String,
    pub workspace_id: WorkspaceId,
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use uuid::Uuid;

    use super::{ActorId, ActorKind};
    use crate::domain::DomainError;

    #[test]
    fn actor_kinds_parse_all_authenticated_kinds() {
        for (value, expected) in [
            ("human_user", ActorKind::HumanUser),
            ("ai_agent", ActorKind::AiAgent),
            ("service_account", ActorKind::ServiceAccount),
            ("integration", ActorKind::Integration),
            ("policy_automation", ActorKind::PolicyAutomation),
            ("system", ActorKind::System),
        ] {
            assert_eq!(ActorKind::from_str(value).unwrap(), expected);
        }
    }

    #[test]
    fn actor_kind_rejects_invalid_persisted_values() {
        assert_eq!(
            ActorKind::from_str("admin").unwrap_err(),
            DomainError::InvalidEnumValue {
                field: "actor_type",
                value: "admin".to_owned()
            }
        );
    }

    #[test]
    fn actor_id_wraps_uuid() {
        let uuid = Uuid::parse_str("00000000-0000-4000-8000-000000000002").unwrap();
        let id = ActorId::from(uuid);

        assert_eq!(Uuid::from(id), uuid);
        assert_eq!(id.to_string(), "00000000-0000-4000-8000-000000000002");
    }
}
