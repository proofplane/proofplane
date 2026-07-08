use tokio_postgres::error::SqlState;

use super::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictKind {
    AgentConnectionExists,
    WorkspaceSlugTaken,
    ControlCodeTaken,
    WorkspaceMembershipExists,
    EvidenceRequestControlMappingExists,
}

impl ConflictKind {
    pub fn code(self) -> &'static str {
        match self {
            Self::AgentConnectionExists => "agent_connection_exists",
            Self::WorkspaceSlugTaken => "slug_taken",
            Self::ControlCodeTaken => "control_code_taken",
            Self::WorkspaceMembershipExists => "user_already_has_workspace",
            Self::EvidenceRequestControlMappingExists => "evidence_request_control_mapping_exists",
        }
    }

    pub fn message(self) -> &'static str {
        match self {
            Self::AgentConnectionExists => {
                "a live agent connection already exists for this user, client, and resource"
            }
            Self::WorkspaceSlugTaken => "a workspace with this slug already exists",
            Self::ControlCodeTaken => "a control with this code already exists in the workspace",
            Self::WorkspaceMembershipExists => "the user already belongs to a workspace",
            Self::EvidenceRequestControlMappingExists => {
                "this control is already mapped to the evidence request"
            }
        }
    }
}

fn conflict_kind_for_constraint(constraint: &str) -> Option<ConflictKind> {
    match constraint {
        "agent_connections_live_tuple_key" => Some(ConflictKind::AgentConnectionExists),
        "workspaces_slug_key" => Some(ConflictKind::WorkspaceSlugTaken),
        "controls_workspace_id_code_key" => Some(ConflictKind::ControlCodeTaken),
        "workspace_memberships_pkey" => Some(ConflictKind::WorkspaceMembershipExists),
        "evidence_request_control_mappings_pkey" => {
            Some(ConflictKind::EvidenceRequestControlMappingExists)
        }
        _ => None,
    }
}

pub fn classify_db_error(error: tokio_postgres::Error) -> Error {
    if let Some(db_error) = error.as_db_error() {
        if db_error.code() == &SqlState::UNIQUE_VIOLATION {
            if let Some(kind) = db_error.constraint().and_then(conflict_kind_for_constraint) {
                return Error::Conflict(kind);
            }
        }
    }

    Error::Database(error)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_known_constraints_to_kinds() {
        assert_eq!(
            conflict_kind_for_constraint("workspaces_slug_key"),
            Some(ConflictKind::WorkspaceSlugTaken)
        );
        assert_eq!(
            conflict_kind_for_constraint("controls_workspace_id_code_key"),
            Some(ConflictKind::ControlCodeTaken)
        );
        assert_eq!(
            conflict_kind_for_constraint("workspace_memberships_pkey"),
            Some(ConflictKind::WorkspaceMembershipExists)
        );
        assert_eq!(
            conflict_kind_for_constraint("evidence_request_control_mappings_pkey"),
            Some(ConflictKind::EvidenceRequestControlMappingExists)
        );
    }

    #[test]
    fn unknown_constraint_maps_to_none() {
        assert_eq!(conflict_kind_for_constraint("workspaces_pkey"), None);
        assert_eq!(conflict_kind_for_constraint(""), None);
    }

    #[test]
    fn workspace_slug_taken_exposes_stable_code_and_message() {
        assert_eq!(ConflictKind::WorkspaceSlugTaken.code(), "slug_taken");
        assert_eq!(
            ConflictKind::WorkspaceSlugTaken.message(),
            "a workspace with this slug already exists"
        );
    }

    #[test]
    fn control_code_taken_exposes_stable_code_and_message() {
        assert_eq!(ConflictKind::ControlCodeTaken.code(), "control_code_taken");
        assert_eq!(
            ConflictKind::ControlCodeTaken.message(),
            "a control with this code already exists in the workspace"
        );
    }
}
