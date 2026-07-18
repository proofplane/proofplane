use std::{sync::Arc, time::Duration};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use url::Url;
use uuid::Uuid;

use crate::{
    authentication::paseto::{
        PolicyUploadGrantDecryptor, PolicyUploadGrantEncryptor, RegisteredClaims,
        VerifiedPasetoToken,
    },
    domain::{AgentConnectionId, PolicyDocumentUploadGrantId, PolicyId, UserId, WorkspaceId},
    repository::{NewPolicyDocumentUploadGrant, Postgres},
};

use super::agent_connections::AgentConnectionContext;

const POLICY_UPLOAD_GRANT_TOKEN_VERSION: u8 = 1;
const POLICY_UPLOAD_GRANT_TTL: Duration = Duration::from_secs(5 * 60);
pub const POLICY_UPLOAD_GRANT_AUDIENCE: &str = "proofplane-policy-document-upload-grant";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PolicyUploadGrantClaims {
    version: u8,
    grant_id: String,
    workspace_id: String,
    policy_id: String,
    issued_by_user_id: String,
    issued_via_agent_connection_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct VerifiedPolicyUploadGrant {
    id: PolicyDocumentUploadGrantId,
    workspace_id: WorkspaceId,
    policy_id: PolicyId,
    issued_by_user_id: UserId,
    issued_via_agent_connection_id: AgentConnectionId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssuedPolicyUploadGrant {
    pub url: Url,
    pub expires_at: DateTime<Utc>,
    pub policy_id: PolicyId,
    pub audit: PolicyUploadGrantAuditContext,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RedeemedPolicyUploadGrant {
    pub id: PolicyDocumentUploadGrantId,
    pub workspace_id: WorkspaceId,
    pub policy_id: PolicyId,
    pub issued_by_user_id: UserId,
    pub issued_via_agent_connection_id: AgentConnectionId,
    pub expires_at: DateTime<Utc>,
    pub redeemed_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PolicyUploadGrantAuditContext {
    pub workspace_id: WorkspaceId,
    pub policy_id: PolicyId,
    pub issued_by_user_id: UserId,
    pub issued_via_agent_connection_id: AgentConnectionId,
}

#[derive(Clone)]
pub struct PolicyDocumentUploadGrantService {
    repository: Arc<Postgres>,
    public_api_base_url: Url,
    grant_encryptor: PolicyUploadGrantEncryptor,
    grant_decryptor: PolicyUploadGrantDecryptor,
}

impl PolicyDocumentUploadGrantService {
    pub fn new(
        repository: Arc<Postgres>,
        public_api_base_url: Url,
        grant_encryptor: PolicyUploadGrantEncryptor,
        grant_decryptor: PolicyUploadGrantDecryptor,
    ) -> Self {
        Self {
            repository,
            public_api_base_url,
            grant_encryptor,
            grant_decryptor,
        }
    }

    pub async fn issue(
        &self,
        connection: &AgentConnectionContext,
        policy_id: PolicyId,
    ) -> Result<IssuedPolicyUploadGrant, PolicyUploadGrantError> {
        let expires_at = Utc::now()
            + chrono::Duration::from_std(POLICY_UPLOAD_GRANT_TTL)
                .map_err(|_| PolicyUploadGrantError::Internal)?;
        let grant_id = PolicyDocumentUploadGrantId::from(Uuid::new_v4());
        let grant = self
            .repository
            .in_agent_connection_workspace_context(
                connection.workspace_id,
                connection.user_id,
                connection.connection_id,
                async move |context| {
                    context
                        .create_policy_document_upload_grant(NewPolicyDocumentUploadGrant {
                            id: grant_id,
                            policy_id,
                            expires_at,
                        })
                        .await
                },
            )
            .await?
            .ok_or(PolicyUploadGrantError::Unavailable)?;

        let issued = self
            .grant_encryptor
            .encrypt(
                RegisteredClaims {
                    subject: Uuid::from(grant.issued_by_user_id),
                    token_id: Uuid::from(grant.id),
                    expires_at: grant.expires_at,
                },
                &PolicyUploadGrantClaims {
                    version: POLICY_UPLOAD_GRANT_TOKEN_VERSION,
                    grant_id: grant.id.to_string(),
                    workspace_id: grant.workspace_id.to_string(),
                    policy_id: grant.policy_id.to_string(),
                    issued_by_user_id: grant.issued_by_user_id.to_string(),
                    issued_via_agent_connection_id: grant
                        .issued_via_agent_connection_id
                        .to_string(),
                },
            )
            .map_err(|_| PolicyUploadGrantError::Internal)?;
        let mut url = self
            .public_api_base_url
            .join("policy-document-uploads")
            .map_err(|_| PolicyUploadGrantError::Internal)?;
        url.query_pairs_mut().append_pair("token", &issued.token);

        Ok(IssuedPolicyUploadGrant {
            url,
            expires_at: issued.expires_at,
            policy_id,
            audit: PolicyUploadGrantAuditContext {
                workspace_id: grant.workspace_id,
                policy_id,
                issued_by_user_id: grant.issued_by_user_id,
                issued_via_agent_connection_id: grant.issued_via_agent_connection_id,
            },
        })
    }

    pub async fn redeem(
        &self,
        token: &str,
    ) -> Result<RedeemedPolicyUploadGrant, PolicyUploadGrantError> {
        let verified = self
            .grant_decryptor
            .decrypt::<PolicyUploadGrantClaims>(token)
            .map_err(|_| PolicyUploadGrantError::Unavailable)?;
        let grant = VerifiedPolicyUploadGrant::try_from(verified)
            .map_err(|_| PolicyUploadGrantError::Unavailable)?;
        let redeemed = self
            .repository
            .redeem_policy_document_upload_grant(grant.id, grant.workspace_id, grant.policy_id)
            .await?
            .ok_or(PolicyUploadGrantError::Unavailable)?;
        if redeemed.issued_by_user_id != grant.issued_by_user_id
            || redeemed.issued_via_agent_connection_id != grant.issued_via_agent_connection_id
        {
            return Err(PolicyUploadGrantError::Unavailable);
        }

        Ok(RedeemedPolicyUploadGrant {
            id: redeemed.id,
            workspace_id: redeemed.workspace_id,
            policy_id: redeemed.policy_id,
            issued_by_user_id: redeemed.issued_by_user_id,
            issued_via_agent_connection_id: redeemed.issued_via_agent_connection_id,
            expires_at: redeemed.expires_at,
            redeemed_at: redeemed
                .redeemed_at
                .ok_or(PolicyUploadGrantError::Internal)?,
        })
    }
}

impl TryFrom<VerifiedPasetoToken<PolicyUploadGrantClaims>> for VerifiedPolicyUploadGrant {
    type Error = InvalidPolicyUploadGrantClaims;

    fn try_from(token: VerifiedPasetoToken<PolicyUploadGrantClaims>) -> Result<Self, Self::Error> {
        let VerifiedPasetoToken {
            subject,
            token_id,
            key_id: _,
            expires_at: _,
            claims,
        } = token;
        if claims.version != POLICY_UPLOAD_GRANT_TOKEN_VERSION {
            return Err(InvalidPolicyUploadGrantClaims);
        }
        let id = PolicyDocumentUploadGrantId::from(parse_uuid(&claims.grant_id)?);
        let issued_by_user_id = UserId::from(parse_uuid(&claims.issued_by_user_id)?);
        if id != PolicyDocumentUploadGrantId::from(token_id)
            || issued_by_user_id != UserId::from(subject)
        {
            return Err(InvalidPolicyUploadGrantClaims);
        }

        Ok(Self {
            id,
            workspace_id: WorkspaceId::from(parse_uuid(&claims.workspace_id)?),
            policy_id: PolicyId::from(parse_uuid(&claims.policy_id)?),
            issued_by_user_id,
            issued_via_agent_connection_id: AgentConnectionId::from(parse_uuid(
                &claims.issued_via_agent_connection_id,
            )?),
        })
    }
}

#[derive(Debug)]
struct InvalidPolicyUploadGrantClaims;

#[derive(Debug, thiserror::Error)]
pub enum PolicyUploadGrantError {
    #[error("policy document upload grant is unavailable")]
    Unavailable,
    #[error("internal policy document upload grant error")]
    Internal,
    #[error("repository error")]
    Repository(#[from] crate::repository::Error),
}

fn parse_uuid(value: &str) -> Result<Uuid, InvalidPolicyUploadGrantClaims> {
    Uuid::parse_str(value).map_err(|_| InvalidPolicyUploadGrantClaims)
}
