//! Temporary compatibility boundary for machine evidence-upload grants.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    application::{
        commands::issue_agent_evidence_upload_grant::{
            IssueAgentEvidenceUploadGrant, IssueAgentEvidenceUploadGrantHandler,
        },
        ExecutionMetadata,
    },
    authentication::paseto::{
        AgentEvidenceUploadGrantDecryptor, AgentEvidenceUploadGrantEncryptor, VerifiedPasetoToken,
    },
    domain::{
        AgentConnectionId, AgentEvidenceUploadAuthority, AgentEvidenceUploadDeclaration,
        AgentEvidenceUploadGrantId, CoverageWindow, EvidenceId, EvidenceSubmissionId, UserId,
        WorkspaceId,
    },
    repository::Postgres,
};

use super::agent_connections::AgentConnectionContext;

pub use crate::application::commands::issue_agent_evidence_upload_grant::{
    AgentEvidenceUploadGrantError, IssuedAgentEvidenceUploadGrant,
    AGENT_EVIDENCE_UPLOAD_GRANT_AUDIENCE,
};

const TOKEN_VERSION: u8 = 1;

#[derive(Clone)]
pub struct AgentEvidenceUploadCredentialVerifier {
    decryptor: AgentEvidenceUploadGrantDecryptor,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct AgentEvidenceUploadGrantClaims {
    version: u8,
    upload_id: String,
    workspace_id: String,
    evidence_id: String,
    submission_id: String,
    issued_by_user_id: String,
    issued_via_agent_connection_id: String,
}

impl AgentEvidenceUploadGrantClaims {
    pub(crate) fn new(
        upload_id: AgentEvidenceUploadGrantId,
        workspace_id: WorkspaceId,
        evidence_id: EvidenceId,
        submission_id: EvidenceSubmissionId,
        issued_by_user_id: UserId,
        issued_via_agent_connection_id: AgentConnectionId,
    ) -> Self {
        Self {
            version: TOKEN_VERSION,
            upload_id: upload_id.to_string(),
            workspace_id: workspace_id.to_string(),
            evidence_id: evidence_id.to_string(),
            submission_id: submission_id.to_string(),
            issued_by_user_id: issued_by_user_id.to_string(),
            issued_via_agent_connection_id: issued_via_agent_connection_id.to_string(),
        }
    }
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
    issue_handler: IssueAgentEvidenceUploadGrantHandler,
    credential_verifier: AgentEvidenceUploadCredentialVerifier,
}

impl AgentEvidenceUploadGrantService {
    pub fn new(
        repository: Arc<Postgres>,
        encryptor: AgentEvidenceUploadGrantEncryptor,
        decryptor: AgentEvidenceUploadGrantDecryptor,
    ) -> Self {
        Self {
            issue_handler: IssueAgentEvidenceUploadGrantHandler::new(repository, encryptor),
            credential_verifier: AgentEvidenceUploadCredentialVerifier::new(decryptor),
        }
    }

    pub fn credential_verifier(&self) -> AgentEvidenceUploadCredentialVerifier {
        self.credential_verifier.clone()
    }

    pub async fn issue(
        &self,
        connection: &AgentConnectionContext,
        evidence_id: EvidenceId,
        coverage: CoverageWindow,
        declaration: AgentEvidenceUploadDeclaration,
    ) -> Result<IssuedAgentEvidenceUploadGrant, AgentEvidenceUploadGrantError> {
        self.issue_handler
            .handle(
                IssueAgentEvidenceUploadGrant {
                    connection: *connection,
                    evidence_id,
                    coverage,
                    declaration,
                },
                ExecutionMetadata::background(),
            )
            .await
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
