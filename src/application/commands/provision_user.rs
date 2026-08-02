use std::sync::Arc;

use chrono::Utc;
use uuid::Uuid;

use crate::{
    application::ExecutionMetadata,
    domain::{User, UserId, UserTransition},
    repository::Postgres,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProvisionUser {
    pub auth0_sub: String,
    pub email: Option<String>,
    pub name: Option<String>,
}

#[derive(Clone)]
pub struct ProvisionUserHandler {
    repository: Arc<Postgres>,
}

impl ProvisionUserHandler {
    pub fn new(repository: Arc<Postgres>) -> Self {
        Self { repository }
    }

    pub async fn handle(
        &self,
        command: ProvisionUser,
        _metadata: ExecutionMetadata,
    ) -> Result<User, ProvisionUserError> {
        self.repository
            .in_transaction(async move |context| {
                let repository = context.users();
                let (user, changed) = match repository.get_by_auth0_sub(&command.auth0_sub).await? {
                    Some(mut user) => {
                        let transition = user.provision_profile(command.email, command.name);
                        (user, transition == UserTransition::Applied)
                    }
                    None => (
                        User::provision(
                            UserId::from(Uuid::new_v4()),
                            command.auth0_sub,
                            command.email,
                            command.name,
                            Utc::now(),
                        ),
                        true,
                    ),
                };
                if changed {
                    repository.save(&user).await?;
                }
                Ok(user)
            })
            .await
            .map_err(ProvisionUserError::Repository)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ProvisionUserError {
    #[error("user provisioning repository error")]
    Repository(#[from] crate::repository::Error),
}
