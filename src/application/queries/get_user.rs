use std::sync::Arc;

use crate::{
    domain::{User, UserId},
    repository::Postgres,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GetUser {
    pub user_id: UserId,
}

#[derive(Clone)]
pub struct GetUserHandler {
    repository: Arc<Postgres>,
}

impl GetUserHandler {
    pub fn new(repository: Arc<Postgres>) -> Self {
        Self { repository }
    }

    pub async fn handle(&self, query: GetUser) -> Result<Option<User>, crate::repository::Error> {
        self.repository.users().get(query.user_id).await
    }
}
