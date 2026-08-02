//! Temporary compatibility boundary for machine evidence-upload grants.

use std::sync::Arc;

use crate::{
    application::{
        commands::issue_agent_evidence_upload_grant::{
            IssueAgentEvidenceUploadGrant, IssueAgentEvidenceUploadGrantHandler,
        },
        ExecutionMetadata,
    },
    authentication::paseto::{
        AgentEvidenceUploadGrantClaims, AgentEvidenceUploadGrantDecryptor,
        AgentEvidenceUploadGrantEncryptor, VerifiedAgentEvidenceUploadGrantClaims,
    },
    domain::{
        AgentEvidenceUploadAuthority, AgentEvidenceUploadDeclaration, CoverageWindow, EvidenceId,
    },
    repository::Postgres,
};

use super::agent_connections::AgentConnectionContext;

pub use crate::application::commands::issue_agent_evidence_upload_grant::{
    AgentEvidenceUploadGrantError, IssuedAgentEvidenceUploadGrant,
    AGENT_EVIDENCE_UPLOAD_GRANT_AUDIENCE,
};

#[derive(Clone)]
pub struct AgentEvidenceUploadCredentialVerifier {
    decryptor: AgentEvidenceUploadGrantDecryptor,
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
            claims.upload_id.into(),
            claims.workspace_id.into(),
            claims.evidence_id.into(),
            claims.submission_id.into(),
            claims.issued_by_user_id.into(),
            claims.issued_via_agent_connection_id.into(),
            claims.expires_at,
        ))
    }
}
