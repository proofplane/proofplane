use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    authentication::paseto::{
        PolicyUploadSessionDecryptor, PolicyUploadSessionEncryptor, RegisteredClaims,
        VerifiedPasetoToken,
    },
    domain::{AgentConnectionId, PolicyId, UserId, WorkspaceId},
};

const POLICY_UPLOAD_SESSION_TOKEN_VERSION: u8 = 1;
pub const POLICY_UPLOAD_SESSION_AUDIENCE: &str = "proofplane-policy-document-upload-session";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PolicyUploadSessionClaims {
    version: u8,
    workspace_id: String,
    policy_id: String,
    issued_by_user_id: String,
    issued_via_agent_connection_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VerifiedPolicyUploadSession {
    pub workspace_id: WorkspaceId,
    pub policy_id: PolicyId,
    pub issued_by_user_id: UserId,
    pub issued_via_agent_connection_id: AgentConnectionId,
    pub expires_at: DateTime<Utc>,
}

#[derive(Clone)]
pub struct PolicyUploadSessionTokenService {
    encryptor: PolicyUploadSessionEncryptor,
    decryptor: PolicyUploadSessionDecryptor,
}

impl PolicyUploadSessionTokenService {
    pub fn new(
        encryptor: PolicyUploadSessionEncryptor,
        decryptor: PolicyUploadSessionDecryptor,
    ) -> Self {
        Self {
            encryptor,
            decryptor,
        }
    }

    pub fn issue_until(
        &self,
        workspace_id: WorkspaceId,
        policy_id: PolicyId,
        issued_by_user_id: UserId,
        issued_via_agent_connection_id: AgentConnectionId,
        expires_at: DateTime<Utc>,
    ) -> Result<String, PolicyUploadSessionError> {
        self.encryptor
            .encrypt(
                RegisteredClaims {
                    subject: Uuid::from(issued_by_user_id),
                    token_id: Uuid::new_v4(),
                    expires_at,
                },
                &PolicyUploadSessionClaims {
                    version: POLICY_UPLOAD_SESSION_TOKEN_VERSION,
                    workspace_id: workspace_id.to_string(),
                    policy_id: policy_id.to_string(),
                    issued_by_user_id: issued_by_user_id.to_string(),
                    issued_via_agent_connection_id: issued_via_agent_connection_id.to_string(),
                },
            )
            .map(|issued| issued.token)
            .map_err(|_| PolicyUploadSessionError::Internal)
    }

    pub fn verify(
        &self,
        token: &str,
    ) -> Result<VerifiedPolicyUploadSession, PolicyUploadSessionError> {
        let verified = self
            .decryptor
            .decrypt::<PolicyUploadSessionClaims>(token)
            .map_err(|_| PolicyUploadSessionError::Unavailable)?;
        VerifiedPolicyUploadSession::try_from(verified)
            .map_err(|_| PolicyUploadSessionError::Unavailable)
    }
}

impl TryFrom<VerifiedPasetoToken<PolicyUploadSessionClaims>> for VerifiedPolicyUploadSession {
    type Error = InvalidPolicyUploadSessionClaims;

    fn try_from(
        token: VerifiedPasetoToken<PolicyUploadSessionClaims>,
    ) -> Result<Self, Self::Error> {
        let VerifiedPasetoToken {
            subject,
            token_id: _,
            key_id: _,
            expires_at,
            claims,
        } = token;
        if claims.version != POLICY_UPLOAD_SESSION_TOKEN_VERSION {
            return Err(InvalidPolicyUploadSessionClaims);
        }
        let issued_by_user_id = UserId::from(parse_uuid(&claims.issued_by_user_id)?);
        if issued_by_user_id != UserId::from(subject) {
            return Err(InvalidPolicyUploadSessionClaims);
        }

        Ok(Self {
            workspace_id: WorkspaceId::from(parse_uuid(&claims.workspace_id)?),
            policy_id: PolicyId::from(parse_uuid(&claims.policy_id)?),
            issued_by_user_id,
            issued_via_agent_connection_id: AgentConnectionId::from(parse_uuid(
                &claims.issued_via_agent_connection_id,
            )?),
            expires_at,
        })
    }
}

#[derive(Debug)]
struct InvalidPolicyUploadSessionClaims;

#[derive(Debug, thiserror::Error)]
pub enum PolicyUploadSessionError {
    #[error("policy document upload session is unavailable")]
    Unavailable,
    #[error("internal policy document upload session error")]
    Internal,
}

fn parse_uuid(value: &str) -> Result<Uuid, InvalidPolicyUploadSessionClaims> {
    Uuid::parse_str(value).map_err(|_| InvalidPolicyUploadSessionClaims)
}
