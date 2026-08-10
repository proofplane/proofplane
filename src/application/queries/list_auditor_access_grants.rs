use std::sync::Arc;

use thiserror::Error;

use crate::{
    authentication::AgentConnectionContext, domain::WorkspacePermission, persistence::Postgres,
    read_models::AuditorAccessGrantSummary,
};

#[derive(Debug, Clone, Copy)]
pub struct ListAuditorAccessGrants {
    pub connection: AgentConnectionContext,
}

#[derive(Clone)]
pub struct ListAuditorAccessGrantsHandler {
    repository: Arc<Postgres>,
}

#[derive(Debug, Error)]
pub enum ListAuditorAccessGrantsError {
    #[error("auditor access grant request is denied")]
    Denied,
    #[error("repository error")]
    Repository(#[from] crate::persistence::Error),
}

impl ListAuditorAccessGrantsHandler {
    pub fn new(repository: Arc<Postgres>) -> Self {
        Self { repository }
    }

    pub async fn handle(
        &self,
        query: ListAuditorAccessGrants,
    ) -> Result<Vec<AuditorAccessGrantSummary>, ListAuditorAccessGrantsError> {
        authorize(&query.connection)?;

        Ok(self
            .repository
            .reads()
            .await?
            .auditor_access_grants()
            .list(query.connection.workspace_id)
            .await?)
    }
}

fn authorize(connection: &AgentConnectionContext) -> Result<(), ListAuditorAccessGrantsError> {
    connection
        .permissions
        .has(WorkspacePermission::ManageAuditorAccess)
        .then_some(())
        .ok_or(ListAuditorAccessGrantsError::Denied)
}

#[cfg(test)]
mod tests {
    use super::{authorize, ListAuditorAccessGrantsError};
    use crate::{
        authentication::AgentConnectionContext,
        domain::{AgentConnectionId, UserId, WorkspaceId, WorkspacePermissions},
    };
    use uuid::Uuid;

    #[test]
    fn listing_requires_auditor_access_permission() {
        let connection = AgentConnectionContext {
            user_id: UserId::from(Uuid::new_v4()),
            connection_id: AgentConnectionId::from(Uuid::new_v4()),
            workspace_id: WorkspaceId::from(Uuid::new_v4()),
            permissions: WorkspacePermissions::none(),
        };

        assert!(matches!(
            authorize(&connection),
            Err(ListAuditorAccessGrantsError::Denied)
        ));
    }
}
