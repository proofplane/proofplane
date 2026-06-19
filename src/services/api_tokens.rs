use std::sync::Arc;

use chrono::{DateTime, Utc};
use secrecy::SecretString;
use thiserror::Error;
use uuid::Uuid;

use crate::{
    authentication::opaque_token::{generate_opaque_token, OpaqueTokenError},
    domain::{
        ApiTokenId, ApiTokenWithPermissions, CreateApiTokenPayload, UserId, WorkspaceId,
        WorkspacePermission,
    },
    repository::Postgres,
};

#[derive(Clone)]
pub struct ApiTokenService {
    repository: Arc<Postgres>,
}

#[derive(Debug, Error)]
pub enum ApiTokenError {
    #[error("API token request is invalid")]
    Invalid(Vec<String>),

    #[error("API token not found")]
    NotFound,

    #[error("API token issuance failed")]
    Issue(#[source] OpaqueTokenError),

    #[error("repository error")]
    Repository(#[from] crate::repository::Error),
}

pub struct IssuedUserApiToken {
    pub token: ApiTokenWithPermissions,
    pub raw_token: SecretString,
}

#[derive(Debug)]
pub struct CreateUserApiTokenPayload {
    pub name: String,
    pub expires_at: DateTime<Utc>,
    pub permissions: Vec<WorkspacePermission>,
}

impl ApiTokenService {
    pub fn new(repository: Arc<Postgres>) -> Self {
        Self { repository }
    }

    pub async fn create_token(
        &self,
        user_id: UserId,
        workspace_id: WorkspaceId,
        request: CreateUserApiTokenPayload,
    ) -> Result<IssuedUserApiToken, ApiTokenError> {
        self.authorize(workspace_id, user_id).await?;
        let token_id = ApiTokenId::from(Uuid::new_v4());
        let issued = generate_opaque_token().map_err(ApiTokenError::Issue)?;
        let token = self
            .repository
            .create_api_token(&CreateApiTokenPayload {
                id: token_id,
                token_digest: issued.digest,
                user_id,
                workspace_id,
                name: request.name,
                expires_at: request.expires_at,
                permissions: request.permissions,
            })
            .await?;

        Ok(IssuedUserApiToken {
            token,
            raw_token: issued.raw_token,
        })
    }

    pub async fn list_tokens(
        &self,
        user_id: UserId,
        workspace_id: WorkspaceId,
    ) -> Result<Vec<ApiTokenWithPermissions>, ApiTokenError> {
        self.authorize(workspace_id, user_id).await?;

        Ok(self
            .repository
            .list_api_tokens_for_owner_workspace(user_id, workspace_id)
            .await?)
    }

    pub async fn revoke_token(
        &self,
        user_id: UserId,
        workspace_id: WorkspaceId,
        token_id: ApiTokenId,
    ) -> Result<(), ApiTokenError> {
        self.authorize(workspace_id, user_id).await?;
        if !self
            .repository
            .revoke_api_token_for_owner_workspace(token_id, user_id, workspace_id)
            .await?
        {
            return Err(ApiTokenError::NotFound);
        }

        Ok(())
    }

    async fn authorize(
        &self,
        workspace_id: WorkspaceId,
        user_id: UserId,
    ) -> Result<(), ApiTokenError> {
        self.repository
            .get_membership_role(workspace_id, user_id)
            .await?
            .ok_or(ApiTokenError::NotFound)?;

        Ok(())
    }
}
