use std::sync::Arc;

use crate::{
    domain::{User, UserId},
    repository::Postgres,
    services::Error,
};

#[derive(Clone)]
pub struct UserService {
    repository: Arc<Postgres>,
}

impl UserService {
    pub fn new(repository: Arc<Postgres>) -> Self {
        Self { repository }
    }

    pub async fn get_user(&self, id: UserId) -> Result<Option<User>, Error> {
        Ok(self.repository.get_user(id).await?)
    }
}
