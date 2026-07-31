use std::{sync::Arc, time::Duration};

use chrono::{DateTime, Utc};
use secrecy::SecretString;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    authentication::paseto::{
        AgentEvidenceUploadGrantDecryptor, AgentEvidenceUploadGrantEncryptor, RegisteredClaims,
        VerifiedPasetoToken,
    },
    domain::{
        AgentConnectionId, AgentEvidenceUploadDeclaration, AgentEvidenceUploadGrantId,
        CoverageWindow, EvidenceId, EvidenceSubmissionId, UserId, WorkspaceId,
    },
    repository::{AgentEvidenceUploadGrant, NewAgentEvidenceUploadGrant, Postgres},
};

use super::agent_connections::AgentConnectionContext;

const TOKEN_VERSION: u8 = 1;
const GRANT_TTL: Duration = Duration::from_secs(5 * 60);
pub const AGENT_EVIDENCE_UPLOAD_GRANT_AUDIENCE: &str = "proofplane-agent-evidence-upload-grant";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AgentEvidenceUploadGrantClaims {
    version: u8,
    upload_id: String,
    workspace_id: String,
    evidence_id: String,
    submission_id: String,
    issued_by_user_id: String,
    issued_via_agent_connection_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct VerifiedAgentEvidenceUploadGrantClaims {
    upload_id: AgentEvidenceUploadGrantId,
    workspace_id: WorkspaceId,
    evidence_id: EvidenceId,
    submission_id: EvidenceSubmissionId,
    issued_by_user_id: UserId,
    issued_via_agent_connection_id: AgentConnectionId,
    expires_at: DateTime<Utc>,
}

#[derive(Clone)]
pub struct AgentEvidenceUploadGrantService {
    repository: Arc<Postgres>,
    encryptor: AgentEvidenceUploadGrantEncryptor,
    decryptor: AgentEvidenceUploadGrantDecryptor,
}

#[derive(Debug)]
pub struct IssuedAgentEvidenceUploadGrant {
    pub grant: AgentEvidenceUploadGrant,
    pub credential: SecretString,
}

impl AgentEvidenceUploadGrantService {
    pub fn new(
        repository: Arc<Postgres>,
        encryptor: AgentEvidenceUploadGrantEncryptor,
        decryptor: AgentEvidenceUploadGrantDecryptor,
    ) -> Self {
        Self {
            repository,
            encryptor,
            decryptor,
        }
    }

    pub async fn issue(
        &self,
        connection: &AgentConnectionContext,
        evidence_id: EvidenceId,
        coverage: CoverageWindow,
        declaration: AgentEvidenceUploadDeclaration,
    ) -> Result<IssuedAgentEvidenceUploadGrant, AgentEvidenceUploadGrantError> {
        let upload_id = AgentEvidenceUploadGrantId::from(Uuid::new_v4());
        let submission_id = EvidenceSubmissionId::from(Uuid::new_v4());
        let expires_at = Utc::now()
            + chrono::Duration::from_std(GRANT_TTL)
                .map_err(|_| AgentEvidenceUploadGrantError::Internal)?;
        let issued = self
            .encryptor
            .encrypt(
                RegisteredClaims {
                    subject: Uuid::from(connection.user_id),
                    token_id: Uuid::from(upload_id),
                    expires_at,
                },
                &AgentEvidenceUploadGrantClaims {
                    version: TOKEN_VERSION,
                    upload_id: upload_id.to_string(),
                    workspace_id: connection.workspace_id.to_string(),
                    evidence_id: evidence_id.to_string(),
                    submission_id: submission_id.to_string(),
                    issued_by_user_id: connection.user_id.to_string(),
                    issued_via_agent_connection_id: connection.connection_id.to_string(),
                },
            )
            .map_err(|_| AgentEvidenceUploadGrantError::Internal)?;
        let grant = self
            .repository
            .in_agent_connection_workspace_context(
                connection.workspace_id,
                connection.user_id,
                connection.connection_id,
                async move |context| {
                    context
                        .create_agent_evidence_upload_grant(NewAgentEvidenceUploadGrant {
                            id: upload_id,
                            submission_id,
                            evidence_id,
                            coverage,
                            declaration,
                            expires_at: issued.expires_at,
                        })
                        .await
                },
            )
            .await?
            .ok_or(AgentEvidenceUploadGrantError::Unavailable)?;

        Ok(IssuedAgentEvidenceUploadGrant {
            grant,
            credential: SecretString::from(issued.token),
        })
    }

    pub async fn verify(
        &self,
        upload_id: AgentEvidenceUploadGrantId,
        credential: &str,
    ) -> Result<AgentEvidenceUploadGrant, AgentEvidenceUploadGrantError> {
        let verified = self
            .decryptor
            .decrypt::<AgentEvidenceUploadGrantClaims>(credential)
            .map_err(|_| AgentEvidenceUploadGrantError::Unavailable)?;
        let claims = VerifiedAgentEvidenceUploadGrantClaims::try_from(verified)
            .map_err(|_| AgentEvidenceUploadGrantError::Unavailable)?;
        if claims.upload_id != upload_id {
            return Err(AgentEvidenceUploadGrantError::Unavailable);
        }
        let grant = self
            .repository
            .get_unexpired_agent_evidence_upload_grant(upload_id, claims.workspace_id)
            .await?
            .ok_or(AgentEvidenceUploadGrantError::Unavailable)?;
        if !claims_match_grant(claims, &grant) {
            return Err(AgentEvidenceUploadGrantError::Unavailable);
        }

        Ok(grant)
    }
}

fn claims_match_grant(
    claims: VerifiedAgentEvidenceUploadGrantClaims,
    grant: &AgentEvidenceUploadGrant,
) -> bool {
    claims.upload_id == grant.id
        && claims.workspace_id == grant.workspace_id
        && claims.evidence_id == grant.evidence_id
        && claims.submission_id == grant.submission_id
        && claims.issued_by_user_id == grant.issued_by_user_id
        && claims.issued_via_agent_connection_id == grant.issued_via_agent_connection_id
        && claims.expires_at == grant.expires_at
}

impl TryFrom<VerifiedPasetoToken<AgentEvidenceUploadGrantClaims>>
    for VerifiedAgentEvidenceUploadGrantClaims
{
    type Error = InvalidAgentEvidenceUploadGrantClaims;

    fn try_from(
        token: VerifiedPasetoToken<AgentEvidenceUploadGrantClaims>,
    ) -> Result<Self, Self::Error> {
        if token.claims.version != TOKEN_VERSION {
            return Err(InvalidAgentEvidenceUploadGrantClaims);
        }
        let upload_id = AgentEvidenceUploadGrantId::from(parse_uuid(&token.claims.upload_id)?);
        let issued_by_user_id = UserId::from(parse_uuid(&token.claims.issued_by_user_id)?);
        if upload_id != AgentEvidenceUploadGrantId::from(token.token_id)
            || issued_by_user_id != UserId::from(token.subject)
        {
            return Err(InvalidAgentEvidenceUploadGrantClaims);
        }

        Ok(Self {
            upload_id,
            workspace_id: WorkspaceId::from(parse_uuid(&token.claims.workspace_id)?),
            evidence_id: EvidenceId::from(parse_uuid(&token.claims.evidence_id)?),
            submission_id: EvidenceSubmissionId::from(parse_uuid(&token.claims.submission_id)?),
            issued_by_user_id,
            issued_via_agent_connection_id: AgentConnectionId::from(parse_uuid(
                &token.claims.issued_via_agent_connection_id,
            )?),
            expires_at: token.expires_at,
        })
    }
}

#[derive(Debug)]
struct InvalidAgentEvidenceUploadGrantClaims;

fn parse_uuid(value: &str) -> Result<Uuid, InvalidAgentEvidenceUploadGrantClaims> {
    Uuid::parse_str(value).map_err(|_| InvalidAgentEvidenceUploadGrantClaims)
}

#[derive(Debug, thiserror::Error)]
pub enum AgentEvidenceUploadGrantError {
    #[error("agent evidence upload grant is unavailable")]
    Unavailable,
    #[error("internal agent evidence upload grant error")]
    Internal,
    #[error("repository error")]
    Repository(#[from] crate::repository::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn verified_token() -> VerifiedPasetoToken<AgentEvidenceUploadGrantClaims> {
        let upload_id = Uuid::new_v4();
        let user_id = Uuid::new_v4();
        VerifiedPasetoToken {
            subject: user_id,
            token_id: upload_id,
            key_id: "test-key".to_owned(),
            expires_at: Utc::now() + chrono::Duration::minutes(5),
            claims: AgentEvidenceUploadGrantClaims {
                version: TOKEN_VERSION,
                upload_id: upload_id.to_string(),
                workspace_id: Uuid::new_v4().to_string(),
                evidence_id: Uuid::new_v4().to_string(),
                submission_id: Uuid::new_v4().to_string(),
                issued_by_user_id: user_id.to_string(),
                issued_via_agent_connection_id: Uuid::new_v4().to_string(),
            },
        }
    }

    #[test]
    fn verified_claims_require_version_subject_token_id_and_typed_ids() {
        assert!(VerifiedAgentEvidenceUploadGrantClaims::try_from(verified_token()).is_ok());

        let mut wrong_version = verified_token();
        wrong_version.claims.version += 1;
        assert!(VerifiedAgentEvidenceUploadGrantClaims::try_from(wrong_version).is_err());

        let mut wrong_subject = verified_token();
        wrong_subject.subject = Uuid::new_v4();
        assert!(VerifiedAgentEvidenceUploadGrantClaims::try_from(wrong_subject).is_err());

        let mut invalid_workspace = verified_token();
        invalid_workspace.claims.workspace_id = "invalid".to_owned();
        assert!(VerifiedAgentEvidenceUploadGrantClaims::try_from(invalid_workspace).is_err());
    }
}
