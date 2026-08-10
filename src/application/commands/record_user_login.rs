use std::sync::Arc;

use chrono::{DateTime, Utc};

use crate::{
    application::ExecutionMetadata,
    domain::{User, UserId, UserTransition},
    persistence::Postgres,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecordUserLogin {
    pub user_id: UserId,
    pub logged_in_at: DateTime<Utc>,
}

#[derive(Clone)]
pub struct RecordUserLoginHandler {
    repository: Arc<Postgres>,
}

impl RecordUserLoginHandler {
    pub fn new(repository: Arc<Postgres>) -> Self {
        Self { repository }
    }

    pub async fn handle(
        &self,
        command: RecordUserLogin,
        _metadata: ExecutionMetadata,
    ) -> Result<User, RecordUserLoginError> {
        let outcome = self
            .repository
            .in_unit_of_work(async move |unit_of_work| {
                let repository = unit_of_work.aggregates().users();
                let Some(mut user) = repository.get(command.user_id).await? else {
                    return Ok(LoginOutcome::Unavailable);
                };
                let transition = user.record_login(command.logged_in_at).map_err(|_| {
                    crate::persistence::Error::InvariantViolation(
                        "login timestamp predates user provisioning",
                    )
                })?;
                if transition == UserTransition::Applied {
                    repository.save(&user).await?;
                }
                Ok(LoginOutcome::Recorded(user))
            })
            .await?;

        match outcome {
            LoginOutcome::Recorded(user) => Ok(user),
            LoginOutcome::Unavailable => Err(RecordUserLoginError::Unavailable),
        }
    }
}

enum LoginOutcome {
    Recorded(User),
    Unavailable,
}

#[derive(Debug, thiserror::Error)]
pub enum RecordUserLoginError {
    #[error("user is unavailable")]
    Unavailable,
    #[error("user login repository error")]
    Repository(#[from] crate::persistence::Error),
}
