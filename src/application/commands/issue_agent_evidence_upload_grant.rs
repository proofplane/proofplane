use std::{sync::Arc, time::Duration};

use chrono::Utc;
use secrecy::SecretString;
use uuid::Uuid;

use crate::{
    application::ExecutionMetadata,
    authentication::paseto::{AgentEvidenceUploadGrantEncryptor, RegisteredClaims},
    domain::{
        AgentEvidenceUploadDeclaration, AgentEvidenceUploadGrant, AgentEvidenceUploadGrantId,
        CoverageWindow, EvidenceId, EvidenceSubmissionId, WorkspacePermission,
    },
    repository::Postgres,
    services::{
        agent_connections::AgentConnectionContext,
        agent_evidence_upload_grants::AgentEvidenceUploadGrantClaims,
    },
};

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

        let encryptor = self.encryptor.clone();
        let outcome = self
            .repository
            .in_agent_connection_workspace_context(
                command.connection.workspace_id,
                command.connection.user_id,
                command.connection.connection_id,
                async move |context| {
                    if context.get_evidence(command.evidence_id).await?.is_none() {
                        return Ok(IssueOutcome::Unavailable);
                    }

                    let upload_id = AgentEvidenceUploadGrantId::from(Uuid::new_v4());
                    let submission_id = EvidenceSubmissionId::from(Uuid::new_v4());
                    let issued_at = Utc::now();
                    let Ok(ttl) = chrono::Duration::from_std(GRANT_TTL) else {
                        return Ok(IssueOutcome::Internal);
                    };
                    let expires_at = issued_at + ttl;
                    let issued = match encryptor.encrypt(
                        RegisteredClaims {
                            subject: Uuid::from(command.connection.user_id),
                            token_id: Uuid::from(upload_id),
                            expires_at,
                        },
                        &AgentEvidenceUploadGrantClaims::new(
                            upload_id,
                            command.connection.workspace_id,
                            command.evidence_id,
                            submission_id,
                            command.connection.user_id,
                            command.connection.connection_id,
                        ),
                    ) {
                        Ok(issued) => issued,
                        Err(_) => return Ok(IssueOutcome::Internal),
                    };
                    let grant = match AgentEvidenceUploadGrant::issue(
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
                    ) {
                        Ok(grant) => grant,
                        Err(_) => return Ok(IssueOutcome::Internal),
                    };
                    let repository = context.agent_evidence_upload_grants();
                    repository.save(&grant).await?;
                    let grant = repository
                        .get(grant.id(), grant.workspace_id())
                        .await?
                        .ok_or(crate::repository::Error::InvariantViolation(
                            "saved machine upload grant must be readable",
                        ))?;

                    Ok(IssueOutcome::Issued(Box::new(
                        IssuedAgentEvidenceUploadGrant {
                            grant,
                            credential: SecretString::from(issued.token),
                        },
                    )))
                },
            )
            .await?;

        match outcome {
            IssueOutcome::Issued(issued) => Ok(*issued),
            IssueOutcome::Unavailable => Err(AgentEvidenceUploadGrantError::Unavailable),
            IssueOutcome::Internal => Err(AgentEvidenceUploadGrantError::Internal),
        }
    }
}

enum IssueOutcome {
    Issued(Box<IssuedAgentEvidenceUploadGrant>),
    Unavailable,
    Internal,
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
