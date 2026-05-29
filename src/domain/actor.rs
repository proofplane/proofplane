use std::{fmt, str::FromStr};

use chrono::{DateTime, Utc};

use super::{api_credential::ApiCredential, ids::uuid_id, DomainError, WorkspaceId};

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
 * ActorContext represents an actor acting in a specific workspace for a specific request.
 */
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActorContext {
    pub workspace_id: WorkspaceId,
    pub id: ActorId,
    pub kind: ActorKind,
    pub display_name: String,
}

/**
 * Actors are users of the system.
 */
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Actor {
    pub id: ActorId,
    pub kind: ActorKind,
    pub display_name: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActorWithApiCredential {
    pub actor: Actor,
    pub api_credential: ApiCredential,
}

impl Actor {
    pub fn context(&self, workspace_id: WorkspaceId) -> ActorContext {
        ActorContext {
            workspace_id,
            id: self.id.clone(),
            kind: self.kind,
            display_name: self.display_name.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateActorPayload {
    pub id: Option<ActorId>, // optional for tests to be able to pass in deterministic IDs
    pub kind: ActorKind,
    pub display_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateActorPayload {
    pub kind: ActorKind,
    pub display_name: String,
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use chrono::{TimeZone, Utc};
    use uuid::Uuid;

    use super::{Actor, ActorContext, ActorId, ActorKind, WorkspaceId};
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
    fn actor_context_carries_identity_for_downstream_work() {
        let workspace_id =
            WorkspaceId::from(Uuid::parse_str("00000000-0000-4000-8000-000000000001").unwrap());
        let actor_id =
            ActorId::from(Uuid::parse_str("00000000-0000-4000-8000-000000000002").unwrap());
        let actor = ActorContext {
            workspace_id,
            id: actor_id,
            kind: ActorKind::System,
            display_name: "System".to_owned(),
        };

        assert_eq!(actor.workspace_id, workspace_id);
        assert_eq!(actor.id, actor_id);
        assert_eq!(actor.kind.as_str(), "system");
    }

    #[test]
    fn actor_id_wraps_uuid() {
        let uuid = Uuid::parse_str("00000000-0000-4000-8000-000000000002").unwrap();
        let id = ActorId::from(uuid);

        assert_eq!(Uuid::from(id), uuid);
        assert_eq!(id.to_string(), "00000000-0000-4000-8000-000000000002");
    }

    #[test]
    fn actor_maps_to_authenticated_context() {
        let workspace_id =
            WorkspaceId::from(Uuid::parse_str("00000000-0000-4000-8000-000000000001").unwrap());
        let actor_id =
            ActorId::from(Uuid::parse_str("00000000-0000-4000-8000-000000000002").unwrap());
        let actor = Actor {
            id: actor_id,
            kind: ActorKind::System,
            display_name: "System".to_owned(),
            created_at: Utc.timestamp_opt(0, 0).unwrap(),
        };

        assert_eq!(
            actor.context(workspace_id),
            ActorContext {
                workspace_id,
                id: actor_id,
                kind: ActorKind::System,
                display_name: "System".to_owned(),
            }
        );
    }
}
