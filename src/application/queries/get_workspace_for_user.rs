use std::sync::Arc;

use crate::{domain::UserId, projections::WorkspaceWithRole, repository::Postgres};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GetWorkspaceForUser {
    pub user_id: UserId,
}

#[derive(Clone)]
pub struct GetWorkspaceForUserHandler {
    repository: Arc<Postgres>,
}

impl GetWorkspaceForUserHandler {
    pub fn new(repository: Arc<Postgres>) -> Self {
        Self { repository }
    }

    pub async fn handle(
        &self,
        query: GetWorkspaceForUser,
    ) -> Result<Option<WorkspaceWithRole>, crate::repository::Error> {
        self.repository
            .workspace_projections()
            .get_for_user(query.user_id)
            .await
    }
}
