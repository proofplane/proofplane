use std::sync::Arc;

use chrono::{DateTime, Utc};
use secrecy::SecretString;
use thiserror::Error;
use uuid::Uuid;

use crate::{
    authentication::opaque_token::{
        generate_auditor_invite_secret, parse_auditor_invite_secret, OpaqueTokenError,
    },
    domain::{
        AuditorAccessGrant, AuditorAccessGrantId, CreateAuditorAccessGrantPayload, WorkspaceId,
        WorkspacePermission,
    },
    repository::Postgres,
};

use super::agent_connections::AgentConnectionContext;

const DEFAULT_GRANT_TTL_DAYS: i64 = 30;

#[derive(Clone)]
pub struct AuditorAccessGrantService {
    repository: Arc<Postgres>,
}

#[derive(Debug, Error)]
pub enum AuditorAccessGrantError {
    #[error("auditor access grant is unavailable")]
    Unavailable,
    #[error("auditor access grant request is denied")]
    Denied,
    #[error("auditor access grant request is invalid")]
    Invalid(&'static str),
    #[error("auditor access grant secret failed")]
    Secret(#[source] OpaqueTokenError),
    #[error("repository error")]
    Repository(#[from] crate::repository::Error),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateAuditorAccessGrantRequest {
    pub auditor_email: String,
    pub expires_at: Option<DateTime<Utc>>,
}

#[derive(Debug)]
pub struct IssuedAuditorAccessGrant {
    pub grant: AuditorAccessGrant,
    pub raw_secret: SecretString,
}

impl AuditorAccessGrantService {
    pub fn new(repository: Arc<Postgres>) -> Self {
        Self { repository }
    }

    pub async fn create(
        &self,
        connection: &AgentConnectionContext,
        request: CreateAuditorAccessGrantRequest,
    ) -> Result<IssuedAuditorAccessGrant, AuditorAccessGrantError> {
        authorize(connection)?;
        let auditor_email = normalize_email(&request.auditor_email)?;
        let expires_at = request.expires_at.unwrap_or_else(default_expires_at);
        if expires_at <= Utc::now() {
            return Err(AuditorAccessGrantError::Invalid(
                "expires_at must be in the future",
            ));
        }

        let issued = generate_auditor_invite_secret().map_err(AuditorAccessGrantError::Secret)?;
        let grant_id = AuditorAccessGrantId::from(Uuid::new_v4());
        let grant = self
            .repository
            .in_agent_connection_workspace_context(
                connection.workspace_id,
                connection.user_id,
                connection.connection_id,
                async move |context| {
                    context
                        .create_auditor_access_grant(CreateAuditorAccessGrantPayload {
                            id: grant_id,
                            secret_digest: issued.digest,
                            auditor_email,
                            expires_at,
                        })
                        .await
                },
            )
            .await?;

        Ok(IssuedAuditorAccessGrant {
            grant,
            raw_secret: issued.raw_secret,
        })
    }

    pub async fn list(
        &self,
        connection: &AgentConnectionContext,
    ) -> Result<Vec<AuditorAccessGrant>, AuditorAccessGrantError> {
        authorize(connection)?;

        Ok(self
            .repository
            .list_auditor_access_grants(connection.workspace_id)
            .await?)
    }

    pub async fn revoke(
        &self,
        connection: &AgentConnectionContext,
        grant_id: AuditorAccessGrantId,
    ) -> Result<AuditorAccessGrant, AuditorAccessGrantError> {
        authorize(connection)?;

        self.repository
            .revoke_auditor_access_grant(connection.workspace_id, grant_id)
            .await?
            .ok_or(AuditorAccessGrantError::Unavailable)
    }

    pub async fn load_for_use(
        &self,
        workspace_id: WorkspaceId,
        raw_secret: &str,
    ) -> Result<AuditorAccessGrant, AuditorAccessGrantError> {
        let digest = parse_auditor_invite_secret(raw_secret)
            .map_err(|_| AuditorAccessGrantError::Unavailable)?;

        self.repository
            .get_active_auditor_access_grant_by_digest(workspace_id, digest)
            .await?
            .ok_or(AuditorAccessGrantError::Unavailable)
    }
}

fn authorize(connection: &AgentConnectionContext) -> Result<(), AuditorAccessGrantError> {
    if connection
        .permissions
        .has(WorkspacePermission::ManageAuditorAccess)
    {
        Ok(())
    } else {
        Err(AuditorAccessGrantError::Denied)
    }
}

fn normalize_email(value: &str) -> Result<String, AuditorAccessGrantError> {
    let email = value.trim().to_ascii_lowercase();
    let Some((local, domain)) = email.split_once('@') else {
        return Err(AuditorAccessGrantError::Invalid("auditor_email is invalid"));
    };
    if local.is_empty() || domain.is_empty() || domain.contains('@') {
        return Err(AuditorAccessGrantError::Invalid("auditor_email is invalid"));
    }

    Ok(email)
}

fn default_expires_at() -> DateTime<Utc> {
    Utc::now() + chrono::Duration::days(DEFAULT_GRANT_TTL_DAYS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_email_trims_and_lowercases() {
        assert_eq!(
            normalize_email("  Auditor@Example.COM ").unwrap(),
            "auditor@example.com"
        );
    }

    #[test]
    fn normalize_email_rejects_invalid_shape() {
        for value in ["", "auditor", "@example.com", "auditor@", "a@b@c"] {
            assert!(matches!(
                normalize_email(value),
                Err(AuditorAccessGrantError::Invalid(_))
            ));
        }
    }
}
