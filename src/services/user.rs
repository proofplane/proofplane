use std::sync::Arc;

use crate::{
    application::{
        commands::record_user_login::{
            RecordUserLogin, RecordUserLoginError, RecordUserLoginHandler,
        },
        queries::get_user::{GetUser, GetUserHandler},
        ExecutionMetadata,
    },
    domain::{User, UserId},
    repository::Postgres,
    services::Error,
};

#[derive(Clone)]
pub struct UserService {
    get_user: GetUserHandler,
    record_login: RecordUserLoginHandler,
}

impl UserService {
    pub fn new(repository: Arc<Postgres>) -> Self {
        Self {
            get_user: GetUserHandler::new(repository.clone()),
            record_login: RecordUserLoginHandler::new(repository),
        }
    }

    pub async fn get_user(&self, id: UserId) -> Result<Option<User>, Error> {
        Ok(self.get_user.handle(GetUser { user_id: id }).await?)
    }

    pub async fn record_login(&self, id: UserId) -> Result<Option<User>, Error> {
        match self
            .record_login
            .handle(
                RecordUserLogin {
                    user_id: id,
                    logged_in_at: chrono::Utc::now(),
                },
                ExecutionMetadata::background(),
            )
            .await
        {
            Ok(user) => Ok(Some(user)),
            Err(RecordUserLoginError::Unavailable) => Ok(None),
            Err(RecordUserLoginError::Repository(error)) => Err(Error::Repository(error)),
        }
    }
}
