use std::sync::Arc;

use chrono::Utc;
use uuid::Uuid;

use crate::{
    application::ExecutionMetadata,
    domain::{User, UserId},
    persistence::Postgres,
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
            .in_unit_of_work(async move |unit_of_work| {
                let existing_user_id = unit_of_work
                    .reads()
                    .users()
                    .resolve_id_by_auth0_sub(&command.auth0_sub)
                    .await?;
                let repository = unit_of_work.aggregates().users();
                let user = match existing_user_id {
                    Some(user_id) => {
                        let mut user = repository.get(user_id).await?.ok_or(
                            crate::persistence::Error::InvariantViolation(
                                "resolved user must exist",
                            ),
                        )?;
                        let _transition = user.provision_profile(command.email, command.name);
                        user
                    }
                    None => User::provision(
                        UserId::from(Uuid::new_v4()),
                        command.auth0_sub,
                        command.email,
                        command.name,
                        Utc::now(),
                    ),
                };
                repository.save(&user).await?;
                Ok(user)
            })
            .await
            .map_err(ProvisionUserError::Repository)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ProvisionUserError {
    #[error("user provisioning repository error")]
    Repository(#[from] crate::persistence::Error),
}
