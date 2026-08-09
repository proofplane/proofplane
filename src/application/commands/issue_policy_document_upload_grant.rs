use std::{sync::Arc, time::Duration};

use chrono::{DateTime, Utc};
use url::Url;
use uuid::Uuid;

use crate::{
    application::ExecutionMetadata,
    authentication::{
        paseto::{PolicyDocumentUploadGrantClaims, PolicyUploadGrantEncryptor, RegisteredClaims},
        AgentConnectionContext,
    },
    domain::{
        PolicyDocumentUploadGrant, PolicyDocumentUploadGrantId, PolicyId, UserId, WorkspaceId,
        WorkspacePermission,
    },
    repository::Postgres,
};

const GRANT_TTL: Duration = Duration::from_secs(5 * 60);

#[derive(Debug, Clone, Copy)]
pub struct IssuePolicyDocumentUploadGrant {
    pub connection: AgentConnectionContext,
    pub policy_id: PolicyId,
}

#[derive(Clone)]
pub struct IssuePolicyDocumentUploadGrantHandler {
    repository: Arc<Postgres>,
    public_api_base_url: Url,
    encryptor: PolicyUploadGrantEncryptor,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssuedPolicyDocumentUploadGrant {
    pub url: Url,
    pub expires_at: DateTime<Utc>,
    pub policy_id: PolicyId,
    pub audit: PolicyDocumentUploadGrantAuditContext,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PolicyDocumentUploadGrantAuditContext {
    pub workspace_id: WorkspaceId,
    pub policy_id: PolicyId,
    pub issued_by_user_id: UserId,
    pub issued_via_agent_connection_id: crate::domain::AgentConnectionId,
}

impl IssuePolicyDocumentUploadGrantHandler {
    pub fn new(
        repository: Arc<Postgres>,
        public_api_base_url: Url,
        encryptor: PolicyUploadGrantEncryptor,
    ) -> Self {
        Self {
            repository,
            public_api_base_url,
            encryptor,
        }
    }

    pub async fn handle(
        &self,
        command: IssuePolicyDocumentUploadGrant,
        _metadata: ExecutionMetadata,
    ) -> Result<IssuedPolicyDocumentUploadGrant, PolicyDocumentUploadGrantHandlerError> {
        if !command
            .connection
            .permissions
            .has(WorkspacePermission::WriteControls)
        {
            return Err(PolicyDocumentUploadGrantHandlerError::Unavailable);
        }
        let encryptor = self.encryptor.clone();
        let public_api_base_url = self.public_api_base_url.clone();
        let outcome = self
            .repository
            .in_unit_of_work(async move |unit_of_work| {
                let workspace = unit_of_work.for_workspace(command.connection.workspace_id);
                let context = &workspace;
                if context
                    .lock_policy_document_upload_eligibility(command.policy_id)
                    .await?
                    .is_none()
                {
                    return Ok(IssueOutcome::Unavailable);
                }
                let issued_at = Utc::now();
                let Ok(ttl) = chrono::Duration::from_std(GRANT_TTL) else {
                    return Ok(IssueOutcome::Internal);
                };
                let expires_at = issued_at + ttl;
                let grant_id = PolicyDocumentUploadGrantId::from(Uuid::new_v4());
                let issued = match encryptor.encrypt(
                    RegisteredClaims {
                        subject: command.connection.user_id.into(),
                        token_id: grant_id.into(),
                        expires_at,
                    },
                    &PolicyDocumentUploadGrantClaims::new(
                        grant_id.into(),
                        command.connection.workspace_id.into(),
                        command.policy_id.into(),
                        command.connection.user_id.into(),
                        command.connection.connection_id.into(),
                    ),
                ) {
                    Ok(issued) => issued,
                    Err(_) => return Ok(IssueOutcome::Internal),
                };
                let grant = match PolicyDocumentUploadGrant::issue(
                    grant_id,
                    command.connection.workspace_id,
                    command.policy_id,
                    command.connection.user_id,
                    command.connection.connection_id,
                    issued_at,
                    issued.expires_at,
                ) {
                    Ok(grant) => grant,
                    Err(_) => return Ok(IssueOutcome::Internal),
                };
                let mut url = match public_api_base_url.join("policy-document-uploads") {
                    Ok(url) => url,
                    Err(_) => return Ok(IssueOutcome::Internal),
                };
                url.query_pairs_mut().append_pair("token", &issued.token);
                let repository = context.policy_document_upload_grants();
                repository.save(&grant).await?;
                let grant = repository
                    .get(grant.id(), grant.workspace_id())
                    .await?
                    .ok_or(crate::repository::Error::InvariantViolation(
                        "saved policy human upload grant must be readable",
                    ))?;
                Ok(IssueOutcome::Issued(Box::new(
                    IssuedPolicyDocumentUploadGrant {
                        url,
                        expires_at: issued.expires_at,
                        policy_id: grant.policy_id(),
                        audit: PolicyDocumentUploadGrantAuditContext {
                            workspace_id: grant.workspace_id(),
                            policy_id: grant.policy_id(),
                            issued_by_user_id: grant.issued_by_user_id(),
                            issued_via_agent_connection_id: grant.issued_via_agent_connection_id(),
                        },
                    },
                )))
            })
            .await?;

        match outcome {
            IssueOutcome::Issued(issued) => Ok(*issued),
            IssueOutcome::Unavailable => Err(PolicyDocumentUploadGrantHandlerError::Unavailable),
            IssueOutcome::Internal => Err(PolicyDocumentUploadGrantHandlerError::Internal),
        }
    }
}

enum IssueOutcome {
    Issued(Box<IssuedPolicyDocumentUploadGrant>),
    Unavailable,
    Internal,
}

#[derive(Debug, thiserror::Error)]
pub enum PolicyDocumentUploadGrantHandlerError {
    #[error("policy document upload grant is unavailable")]
    Unavailable,
    #[error("internal policy document upload grant error")]
    Internal,
    #[error("repository error")]
    Repository(#[from] crate::repository::Error),
}
