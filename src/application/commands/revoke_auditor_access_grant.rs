use std::sync::Arc;

use chrono::Utc;
use thiserror::Error;

use crate::{
    application::ExecutionMetadata,
    authentication::AgentConnectionContext,
    domain::{
        AuditorAccessGrant, AuditorAccessGrantId, AuditorAccessGrantRevocation, WorkspacePermission,
    },
    repository::Postgres,
};

#[derive(Debug, Clone, Copy)]
pub struct RevokeAuditorAccessGrant {
    pub connection: AgentConnectionContext,
    pub grant_id: AuditorAccessGrantId,
}

#[derive(Clone)]
pub struct RevokeAuditorAccessGrantHandler {
    repository: Arc<Postgres>,
}

#[derive(Debug, Error)]
pub enum RevokeAuditorAccessGrantError {
    #[error("auditor access grant is unavailable")]
    Unavailable,
    #[error("auditor access grant request is denied")]
    Denied,
    #[error("repository error")]
    Repository(#[from] crate::repository::Error),
}

impl RevokeAuditorAccessGrantHandler {
    pub fn new(repository: Arc<Postgres>) -> Self {
        Self { repository }
    }

    pub async fn handle(
        &self,
        command: RevokeAuditorAccessGrant,
        _metadata: ExecutionMetadata,
    ) -> Result<AuditorAccessGrant, RevokeAuditorAccessGrantError> {
        authorize(&command.connection)?;
        let connection = command.connection;
        self.repository
            .in_agent_connection_workspace_context(
                connection.workspace_id,
                connection.user_id,
                connection.connection_id,
                async move |context| {
                    let repository = context.auditor_access_grants();
                    let Some(mut grant) = repository
                        .get(command.grant_id, connection.workspace_id)
                        .await?
                    else {
                        return Ok(None);
                    };
                    match grant.revoke(Utc::now()).map_err(|_| {
                        crate::repository::Error::InvariantViolation(
                            "auditor access grant revocation must be valid",
                        )
                    })? {
                        AuditorAccessGrantRevocation::Revoked => repository.save(&grant).await?,
                        AuditorAccessGrantRevocation::AlreadyRevoked => {}
                    }
                    Ok(Some(grant))
                },
            )
            .await?
            .ok_or(RevokeAuditorAccessGrantError::Unavailable)
    }
}

fn authorize(connection: &AgentConnectionContext) -> Result<(), RevokeAuditorAccessGrantError> {
    connection
        .permissions
        .has(WorkspacePermission::ManageAuditorAccess)
        .then_some(())
        .ok_or(RevokeAuditorAccessGrantError::Denied)
}

#[cfg(test)]
mod tests {
    use super::{authorize, RevokeAuditorAccessGrantError};
    use crate::{
        authentication::AgentConnectionContext,
        domain::{AgentConnectionId, UserId, WorkspaceId, WorkspacePermissions},
    };
    use uuid::Uuid;

    #[test]
    fn revocation_requires_auditor_access_permission() {
        let connection = AgentConnectionContext {
            user_id: UserId::from(Uuid::new_v4()),
            connection_id: AgentConnectionId::from(Uuid::new_v4()),
            workspace_id: WorkspaceId::from(Uuid::new_v4()),
            permissions: WorkspacePermissions::none(),
        };

        assert!(matches!(
            authorize(&connection),
            Err(RevokeAuditorAccessGrantError::Denied)
        ));
    }
}
