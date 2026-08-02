use std::{sync::Arc, time::Duration};

use chrono::{DateTime, Utc};
use secrecy::SecretString;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    application::ExecutionMetadata,
    authentication::paseto::{
        AgentEvidenceUploadGrantDecryptor, AgentEvidenceUploadGrantEncryptor, RegisteredClaims,
        VerifiedPasetoToken,
    },
    domain::{
        AgentConnectionId, AgentEvidenceUploadAuthority, AgentEvidenceUploadDeclaration,
        AgentEvidenceUploadGrant, AgentEvidenceUploadGrantId, CoverageWindow, EvidenceId,
        EvidenceSubmissionId, UserId, WorkspaceId, WorkspacePermission,
    },
    repository::Postgres,
    services::agent_connections::AgentConnectionContext,
};

const TOKEN_VERSION: u8 = 1;
const GRANT_TTL: Duration = Duration::from_secs(5 * 60);
pub const AGENT_EVIDENCE_UPLOAD_GRANT_AUDIENCE: &str = "proofplane-agent-evidence-upload-grant";

#[derive(Debug, Clone)]
pub struct IssueAgentEvidenceUploadGrant {
    pub connection: AgentConnectionContext,
    pub evidence_id: EvidenceId,
    pub coverage: CoverageWindow,
    pub declaration: AgentEvidenceUploadDeclaration,
}

#[derive(Clone)]
pub struct IssueAgentEvidenceUploadGrantHandler {
    repository: Arc<Postgres>,
    encryptor: AgentEvidenceUploadGrantEncryptor,
}

#[derive(Clone)]
pub struct AgentEvidenceUploadCredentialVerifier {
    decryptor: AgentEvidenceUploadGrantDecryptor,
}

#[derive(Debug)]
pub struct IssuedAgentEvidenceUploadGrant {
    pub grant: AgentEvidenceUploadGrant,
    pub credential: SecretString,
}

impl IssueAgentEvidenceUploadGrantHandler {
    pub fn new(repository: Arc<Postgres>, encryptor: AgentEvidenceUploadGrantEncryptor) -> Self {
        Self {
            repository,
            encryptor,
        }
    }

    pub async fn handle(
        &self,
        command: IssueAgentEvidenceUploadGrant,
        _metadata: ExecutionMetadata,
    ) -> Result<IssuedAgentEvidenceUploadGrant, AgentEvidenceUploadGrantError> {
        if !command
            .connection
            .permissions
            .has(WorkspacePermission::WriteEvidenceSubmissions)
        {
            return Err(AgentEvidenceUploadGrantError::Unavailable);
        }

        let upload_id = AgentEvidenceUploadGrantId::from(Uuid::new_v4());
        let submission_id = EvidenceSubmissionId::from(Uuid::new_v4());
        let issued_at = Utc::now();
        let expires_at = issued_at
            + chrono::Duration::from_std(GRANT_TTL)
                .map_err(|_| AgentEvidenceUploadGrantError::Internal)?;
        let issued = self
            .encryptor
            .encrypt(
                RegisteredClaims {
                    subject: Uuid::from(command.connection.user_id),
                    token_id: Uuid::from(upload_id),
                    expires_at,
                },
                &AgentEvidenceUploadGrantClaims {
                    version: TOKEN_VERSION,
                    upload_id: upload_id.to_string(),
                    workspace_id: command.connection.workspace_id.to_string(),
                    evidence_id: command.evidence_id.to_string(),
                    submission_id: submission_id.to_string(),
                    issued_by_user_id: command.connection.user_id.to_string(),
                    issued_via_agent_connection_id: command.connection.connection_id.to_string(),
                },
            )
            .map_err(|_| AgentEvidenceUploadGrantError::Internal)?;
        let grant = AgentEvidenceUploadGrant::issue(
            upload_id,
            submission_id,
            command.connection.workspace_id,
            command.evidence_id,
            command.coverage,
            command.declaration,
            command.connection.user_id,
            command.connection.connection_id,
            issued_at,
            issued.expires_at,
        )
        .map_err(|_| AgentEvidenceUploadGrantError::Internal)?;
        let grant = self
            .repository
            .in_agent_connection_workspace_context(
                command.connection.workspace_id,
                command.connection.user_id,
                command.connection.connection_id,
                async move |context| {
                    if context.get_evidence(grant.evidence_id()).await?.is_none() {
                        return Ok(None);
                    }
                    let repository = context.agent_evidence_upload_grants();
                    repository.save(&grant).await?;
                    repository.get(grant.id(), grant.workspace_id()).await
                },
            )
            .await?
            .ok_or(AgentEvidenceUploadGrantError::Unavailable)?;

        Ok(IssuedAgentEvidenceUploadGrant {
            grant,
            credential: SecretString::from(issued.token),
        })
    }
}

impl AgentEvidenceUploadCredentialVerifier {
    pub fn new(decryptor: AgentEvidenceUploadGrantDecryptor) -> Self {
        Self { decryptor }
    }

    pub fn verify(
        &self,
        credential: &str,
    ) -> Result<AgentEvidenceUploadAuthority, AgentEvidenceUploadGrantError> {
        let verified = self
            .decryptor
            .decrypt::<AgentEvidenceUploadGrantClaims>(credential)
            .map_err(|_| AgentEvidenceUploadGrantError::Unavailable)?;
        let claims = VerifiedAgentEvidenceUploadGrantClaims::try_from(verified)
            .map_err(|_| AgentEvidenceUploadGrantError::Unavailable)?;
        Ok(AgentEvidenceUploadAuthority::new(
            claims.upload_id,
            claims.workspace_id,
            claims.evidence_id,
            claims.submission_id,
            claims.issued_by_user_id,
            claims.issued_via_agent_connection_id,
            claims.expires_at,
        ))
    }
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
