use std::sync::Arc;

use crate::{
    authentication::AgentConnectionContext,
    domain::{ControlId, WorkspacePermission},
    projections::ControlDetail,
    repository::{Error as RepositoryError, Postgres},
};

#[derive(Debug, Clone, Copy)]
pub struct ListControls {
    pub connection: AgentConnectionContext,
}

#[derive(Clone)]
pub struct ListControlsHandler {
    repository: Arc<Postgres>,
}

impl ListControlsHandler {
    pub fn new(repository: Arc<Postgres>) -> Self {
        Self { repository }
    }

    pub async fn handle(
        &self,
        query: ListControls,
    ) -> Result<Vec<ControlDetail>, ListControlsError> {
        if !query
            .connection
            .permissions
            .has(WorkspacePermission::ReadControls)
        {
            return Err(ListControlsError::Unavailable);
        }
        Ok(self
            .repository
            .in_workspace_context_read(query.connection.workspace_id, async |context| {
                context.control_projections().list().await
            })
            .await?)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ListControlsError {
    #[error("control catalog is unavailable")]
    Unavailable,
    #[error("repository error")]
    Repository(#[from] RepositoryError),
}

#[derive(Debug, Clone, Copy)]
pub struct GetControl {
    pub connection: AgentConnectionContext,
    pub control_id: ControlId,
}

#[derive(Clone)]
pub struct GetControlHandler {
    repository: Arc<Postgres>,
}

impl GetControlHandler {
    pub fn new(repository: Arc<Postgres>) -> Self {
        Self { repository }
    }

    pub async fn handle(
        &self,
        query: GetControl,
    ) -> Result<Option<ControlDetail>, GetControlError> {
        if !query
            .connection
            .permissions
            .has(WorkspacePermission::ReadControls)
        {
            return Err(GetControlError::Unavailable);
        }
        Ok(self
            .repository
            .in_workspace_context_read(query.connection.workspace_id, async move |context| {
                context.control_projections().get(query.control_id).await
            })
            .await?)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum GetControlError {
    #[error("control is unavailable")]
    Unavailable,
    #[error("repository error")]
    Repository(#[from] RepositoryError),
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use deadpool_postgres::{Manager, Pool};
    use tokio_postgres::{Config, NoTls};
    use uuid::Uuid;

    use crate::{
        authentication::AgentConnectionContext,
        domain::{AgentConnectionId, ControlId, UserId, WorkspaceId, WorkspacePermissions},
        repository::Postgres,
    };

    use super::{
        GetControl, GetControlError, GetControlHandler, ListControls, ListControlsError,
        ListControlsHandler,
    };

    #[tokio::test]
    async fn catalog_queries_conceal_controls_without_read_permission() {
        let connection = connection();
        let list_result = handler_list().handle(ListControls { connection }).await;
        let get_result = handler_get()
            .handle(GetControl {
                connection,
                control_id: ControlId::from(Uuid::new_v4()),
            })
            .await;

        assert!(matches!(list_result, Err(ListControlsError::Unavailable)));
        assert!(matches!(get_result, Err(GetControlError::Unavailable)));
    }

    fn handler_list() -> ListControlsHandler {
        ListControlsHandler::new(repository())
    }

    fn handler_get() -> GetControlHandler {
        GetControlHandler::new(repository())
    }

    fn repository() -> Arc<Postgres> {
        let pool = Pool::builder(Manager::new(Config::new(), NoTls))
            .build()
            .unwrap();
        Arc::new(Postgres::new(pool))
    }

    fn connection() -> AgentConnectionContext {
        AgentConnectionContext {
            user_id: UserId::from(Uuid::new_v4()),
            connection_id: AgentConnectionId::from(Uuid::new_v4()),
            workspace_id: WorkspaceId::from(Uuid::new_v4()),
            permissions: WorkspacePermissions::none(),
        }
    }
}
