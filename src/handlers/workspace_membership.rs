use serde_json::Value;

use crate::{
    authorization::workspaces::WorkspaceAuthorizer,
    domain::{WorkspaceId, WorkspaceRole},
    services::workspaces::WorkspaceMembershipTuple,
    worker::{RetryableWorkerError, WorkerMessage},
};

/// Reconciles a workspace membership change from the outbox into SpiceDB. The
/// synchronous write-through in `WorkspaceService` is best-effort; this handler
/// is the backstop that guarantees Postgres and SpiceDB converge. The
/// `workspace.member_added` write (touch) and the `workspace.member_removed`
/// delete (filter-based) are both idempotent, so duplicate deliveries are safe.
#[derive(Clone)]
pub struct WorkspaceMembershipHandler {
    authorizer: WorkspaceAuthorizer,
}

impl WorkspaceMembershipHandler {
    pub fn new(authorizer: WorkspaceAuthorizer) -> Self {
        Self { authorizer }
    }

    pub async fn handle_member_added(
        &self,
        message: WorkerMessage,
    ) -> Result<(), RetryableWorkerError> {
        let Some(tuple) = resolved_tuple(message.payload) else {
            return Ok(());
        };

        self.authorizer
            .write_user_role(tuple.workspace_id, &tuple.subject_id, tuple.role)
            .await
            .map_err(|error| RetryableWorkerError(error.to_string()))
    }

    pub async fn handle_member_removed(
        &self,
        message: WorkerMessage,
    ) -> Result<(), RetryableWorkerError> {
        let Some(tuple) = resolved_tuple(message.payload) else {
            return Ok(());
        };

        self.authorizer
            .delete_user_role(tuple.workspace_id, &tuple.subject_id, tuple.role)
            .await
            .map_err(|error| RetryableWorkerError(error.to_string()))
    }
}

struct ResolvedTuple {
    workspace_id: WorkspaceId,
    subject_id: String,
    role: WorkspaceRole,
}

/// Parses the outbox payload into the SpiceDB relationship to reconcile.
/// Permanently-unreconcilable payloads (malformed, non-user subject, unknown
/// relation) return `None` so the caller acknowledges the message instead of
/// retrying it forever.
fn resolved_tuple(payload: Value) -> Option<ResolvedTuple> {
    let tuple = match serde_json::from_value::<WorkspaceMembershipTuple>(payload) {
        Ok(tuple) => tuple,
        Err(error) => {
            tracing::warn!(%error, "acknowledging malformed workspace membership tuple");
            return None;
        }
    };

    if tuple.subject_type != "user" {
        tracing::warn!(
            subject_type = %tuple.subject_type,
            "acknowledging unsupported membership subject type"
        );
        return None;
    }

    let Ok(role) = tuple.relation.parse::<WorkspaceRole>() else {
        tracing::warn!(relation = %tuple.relation, "acknowledging unknown membership relation");
        return None;
    };

    Some(ResolvedTuple {
        workspace_id: WorkspaceId::from(tuple.workspace_id),
        subject_id: tuple.subject_id,
        role,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use uuid::Uuid;

    fn tuple_payload(relation: &str, subject_type: &str) -> Value {
        json!({
            "workspace_id": "00000000-0000-4000-8000-000000000001",
            "subject_type": subject_type,
            "subject_id": "00000000-0000-4000-8000-000000000301",
            "relation": relation,
        })
    }

    #[test]
    fn resolves_a_valid_owner_tuple() {
        let tuple = resolved_tuple(tuple_payload("owner", "user")).expect("tuple resolves");

        assert_eq!(
            tuple.workspace_id,
            WorkspaceId::from(Uuid::parse_str("00000000-0000-4000-8000-000000000001").unwrap())
        );
        assert_eq!(tuple.subject_id, "00000000-0000-4000-8000-000000000301");
        assert_eq!(tuple.role, WorkspaceRole::Owner);
    }

    #[test]
    fn resolves_a_valid_admin_tuple() {
        let tuple = resolved_tuple(tuple_payload("admin", "user")).expect("tuple resolves");

        assert_eq!(tuple.role, WorkspaceRole::Admin);
    }

    #[test]
    fn skips_unknown_relation() {
        assert!(resolved_tuple(tuple_payload("superadmin", "user")).is_none());
    }

    #[test]
    fn skips_non_user_subject() {
        assert!(resolved_tuple(tuple_payload("member", "actor")).is_none());
    }

    #[test]
    fn skips_malformed_payload() {
        assert!(resolved_tuple(json!({ "nonsense": true })).is_none());
    }
}
