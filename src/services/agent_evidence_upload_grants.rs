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
        AgentEvidenceUploadGrantDecryptor, AgentEvidenceUploadGrantEncryptor,
    },
    domain::{AgentEvidenceUploadDeclaration, CoverageWindow, EvidenceId},
    repository::Postgres,
};

use super::agent_connections::AgentConnectionContext;

pub use crate::application::commands::issue_agent_evidence_upload_grant::{
    AgentEvidenceUploadCredentialVerifier, AgentEvidenceUploadGrantError,
    IssuedAgentEvidenceUploadGrant, AGENT_EVIDENCE_UPLOAD_GRANT_AUDIENCE,
};

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
