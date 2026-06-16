use std::sync::Arc;

use api_keys_simplified::{Environment, SecureString};
use chrono::{DateTime, Utc};
use thiserror::Error;
use uuid::Uuid;

use crate::{
    authentication::ApiKeyManager,
    domain::{
        Actor, ActorId, ActorKind, ActorPermissions, ActorWithPermissions, CreateActorPayload,
        CreateApiCredentialPayload, UserId, WorkspaceId, WorkspacePermission,
    },
    repository::Postgres,
    services::workspaces::WorkspaceMemberPolicy,
};

/// Management-plane operations on workspace-scoped actors and their API keys.
/// Authorization is answered from Postgres (`workspace_memberships`), matching
/// the rest of the human management plane.
#[derive(Clone)]
pub struct ActorService {
    repository: Arc<Postgres>,
    api_keys: ApiKeyManager,
}

#[derive(Debug, Error)]
pub enum ActorError {
    #[error("the caller may not manage actors")]
    Forbidden,

    #[error("actor or credential not found")]
    NotFound,

    #[error("API key issuance failed")]
    KeyIssue(#[source] crate::authentication::Error),

    #[error("repository error")]
    Repository(#[from] crate::repository::Error),
}

/// A freshly issued credential. The raw key is present exactly once, here, and is
/// never persisted, re-shown, or logged.
pub struct IssuedCredential {
    pub id: String,
    pub name: String,
    pub raw_key: SecureString,
    pub created_at: DateTime<Utc>,
}

impl ActorService {
    pub fn new(repository: Arc<Postgres>, api_keys: ApiKeyManager) -> Self {
        Self {
            repository,
            api_keys,
        }
    }

    pub async fn create_actor(
        &self,
        user_id: UserId,
        workspace_id: WorkspaceId,
        kind: ActorKind,
        display_name: String,
        permissions: Vec<WorkspacePermission>,
    ) -> Result<ActorWithPermissions, ActorError> {
        self.authorize(workspace_id, user_id).await?;

        let actor = self
            .repository
            .create_actor(&CreateActorPayload {
                id: None,
                kind,
                display_name,
                workspace_id,
                created_by_user_id: Some(user_id),
                permissions: permissions.clone(),
            })
            .await?;

        Ok(ActorWithPermissions {
            actor,
            permissions: ActorPermissions::from_iter(permissions),
        })
    }

    pub async fn list_actors(
        &self,
        user_id: UserId,
        workspace_id: WorkspaceId,
    ) -> Result<Vec<ActorWithPermissions>, ActorError> {
        self.authorize(workspace_id, user_id).await?;

        Ok(self
            .repository
            .list_actors_for_workspace(workspace_id)
            .await?)
    }

    pub async fn issue_credential(
        &self,
        user_id: UserId,
        workspace_id: WorkspaceId,
        actor_id: ActorId,
        name: String,
    ) -> Result<IssuedCredential, ActorError> {
        self.authorize(workspace_id, user_id).await?;
        self.actor_in_workspace(workspace_id, actor_id).await?;

        let issued = self
            .api_keys
            .issue(Environment::dev())
            .map_err(ActorError::KeyIssue)?;
        let credential = self
            .repository
            .create_api_credential(&CreateApiCredentialPayload {
                id: Uuid::new_v4().to_string(),
                actor_id,
                name,
                key_id: issued.key_id,
                credential_hash: issued.credential_hash,
                expires_at: None,
                revoked_at: None,
            })
            .await?;

        Ok(IssuedCredential {
            id: credential.id,
            name: credential.name,
            raw_key: issued.raw_key,
            created_at: credential.created_at,
        })
    }

    pub async fn revoke_credential(
        &self,
        user_id: UserId,
        workspace_id: WorkspaceId,
        actor_id: ActorId,
        credential_id: &str,
    ) -> Result<(), ActorError> {
        self.authorize(workspace_id, user_id).await?;
        self.actor_in_workspace(workspace_id, actor_id).await?;

        let credential = self
            .repository
            .get_api_credential(credential_id)
            .await?
            .filter(|credential| credential.actor_id == actor_id)
            .ok_or(ActorError::NotFound)?;

        self.repository
            .revoke_api_credential(&credential.id)
            .await?;

        Ok(())
    }

    /// Reads the caller's role and defers to `WorkspaceMemberPolicy`. A
    /// non-member, an unknown workspace, and an under-privileged member all yield
    /// `Forbidden`, which the API maps to 404 so existence is not leaked.
    async fn authorize(
        &self,
        workspace_id: WorkspaceId,
        user_id: UserId,
    ) -> Result<(), ActorError> {
        let role = self
            .repository
            .get_membership_role(workspace_id, user_id)
            .await?;

        if WorkspaceMemberPolicy::can_manage_actors(role) {
            Ok(())
        } else {
            Err(ActorError::Forbidden)
        }
    }

    /// Ensures the actor exists and belongs to the path workspace. An actor in
    /// another workspace is rejected as not found.
    async fn actor_in_workspace(
        &self,
        workspace_id: WorkspaceId,
        actor_id: ActorId,
    ) -> Result<Actor, ActorError> {
        self.repository
            .get_actor(actor_id)
            .await?
            .filter(|actor| actor.workspace_id == workspace_id)
            .ok_or(ActorError::NotFound)
    }
}
