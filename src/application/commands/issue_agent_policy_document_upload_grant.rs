use std::{sync::Arc, time::Duration};

use chrono::Utc;
use secrecy::SecretString;
use uuid::Uuid;

use crate::{
    application::ExecutionMetadata,
    authentication::{
        paseto::{
            AgentPolicyDocumentUploadGrantClaims, AgentPolicyDocumentUploadGrantEncryptor,
            RegisteredClaims,
        },
        AgentConnectionContext,
    },
    domain::{
        AgentPolicyDocumentUploadDeclaration, AgentPolicyDocumentUploadGrant,
        AgentPolicyDocumentUploadGrantId, PolicyId, WorkspacePermission,
    },
    repository::{PolicyDocumentUploadEligibility, Postgres},
};

const GRANT_TTL: Duration = Duration::from_secs(5 * 60);
pub use crate::authentication::paseto::AGENT_POLICY_DOCUMENT_UPLOAD_GRANT_AUDIENCE;

#[derive(Debug, Clone)]
pub struct IssueAgentPolicyDocumentUploadGrant {
    pub connection: AgentConnectionContext,
    pub policy_id: PolicyId,
    pub declaration: AgentPolicyDocumentUploadDeclaration,
}

#[derive(Clone)]
pub struct IssueAgentPolicyDocumentUploadGrantHandler {
    repository: Arc<Postgres>,
    encryptor: AgentPolicyDocumentUploadGrantEncryptor,
}

#[derive(Debug)]
pub struct IssuedAgentPolicyDocumentUploadGrant {
    pub grant: AgentPolicyDocumentUploadGrant,
    pub credential: SecretString,
}

impl IssueAgentPolicyDocumentUploadGrantHandler {
    pub fn new(
        repository: Arc<Postgres>,
        encryptor: AgentPolicyDocumentUploadGrantEncryptor,
    ) -> Self {
        Self {
            repository,
            encryptor,
        }
    }

    pub async fn handle(
        &self,
        command: IssueAgentPolicyDocumentUploadGrant,
        _metadata: ExecutionMetadata,
    ) -> Result<IssuedAgentPolicyDocumentUploadGrant, AgentPolicyDocumentUploadGrantError> {
        if !command
            .connection
            .permissions
            .has(WorkspacePermission::WriteControls)
        {
            return Err(AgentPolicyDocumentUploadGrantError::Unavailable);
        }

        let upload_id = AgentPolicyDocumentUploadGrantId::from(Uuid::new_v4());
        let issued_at = Utc::now();
        let expires_at = issued_at
            + chrono::Duration::from_std(GRANT_TTL)
                .map_err(|_| AgentPolicyDocumentUploadGrantError::Internal)?;
        let issued = self
            .encryptor
            .encrypt(
                RegisteredClaims {
                    subject: command.connection.user_id.into(),
                    token_id: upload_id.into(),
                    expires_at,
                },
                &AgentPolicyDocumentUploadGrantClaims::new(
                    upload_id.into(),
                    command.connection.workspace_id.into(),
                    command.policy_id.into(),
                    command.connection.user_id.into(),
                    command.connection.connection_id.into(),
                ),
            )
            .map_err(|_| AgentPolicyDocumentUploadGrantError::Internal)?;
        let grant = AgentPolicyDocumentUploadGrant::issue(
            upload_id,
            command.connection.workspace_id,
            command.policy_id,
            command.declaration,
            command.connection.user_id,
            command.connection.connection_id,
            issued_at,
            issued.expires_at,
        )
        .map_err(|_| AgentPolicyDocumentUploadGrantError::Internal)?;

        let outcome = self
            .repository
            .in_agent_connection_workspace_context(
                command.connection.workspace_id,
                command.connection.user_id,
                command.connection.connection_id,
                async move |context| {
                    match context
                        .lock_policy_document_upload_eligibility(grant.policy_id())
                        .await?
                    {
                        None => return Ok(IssueOutcome::Unavailable),
                        Some(PolicyDocumentUploadEligibility::CurrentDocument) => {
                            return Ok(IssueOutcome::CurrentDocument);
                        }
                        Some(PolicyDocumentUploadEligibility::Eligible) => {}
                    }
                    let repository = context.agent_policy_document_upload_grants();
                    repository.save(&grant).await?;
                    let grant = repository
                        .get(grant.id(), grant.workspace_id())
                        .await?
                        .ok_or(crate::repository::Error::InvariantViolation(
                            "saved policy machine upload grant must be readable",
                        ))?;
                    Ok(IssueOutcome::Issued(Box::new(grant)))
                },
            )
            .await?;

        let grant = match outcome {
            IssueOutcome::Issued(grant) => *grant,
            IssueOutcome::Unavailable => {
                return Err(AgentPolicyDocumentUploadGrantError::Unavailable);
            }
            IssueOutcome::CurrentDocument => {
                return Err(AgentPolicyDocumentUploadGrantError::CurrentDocument);
            }
        };

        Ok(IssuedAgentPolicyDocumentUploadGrant {
            grant,
            credential: SecretString::from(issued.token),
        })
    }
}

enum IssueOutcome {
    Issued(Box<AgentPolicyDocumentUploadGrant>),
    Unavailable,
    CurrentDocument,
}

#[derive(Debug, thiserror::Error)]
pub enum AgentPolicyDocumentUploadGrantError {
    #[error("agent policy document upload grant is unavailable")]
    Unavailable,
    #[error("policy already has a current document")]
    CurrentDocument,
    #[error("internal agent policy document upload grant error")]
    Internal,
    #[error("repository error")]
    Repository(#[from] crate::repository::Error),
}
