use std::{sync::Arc, time::Duration};

use chrono::{DateTime, Utc};
use url::Url;
use uuid::Uuid;

use crate::{
    application::ExecutionMetadata,
    authentication::{
        paseto::{EvidenceDocumentUploadGrantClaims, RegisteredClaims, UploadGrantEncryptor},
        AgentConnectionContext,
    },
    domain::{
        AgentConnectionId, CoverageWindow, DocumentUploadGrantId, EvidenceDocumentUploadGrant,
        EvidenceId, UserId, WorkspaceId, WorkspacePermission,
    },
    repository::Postgres,
};

const GRANT_TTL: Duration = Duration::from_secs(5 * 60);

#[derive(Debug, Clone, Copy)]
pub struct IssueEvidenceDocumentUploadGrant {
    pub connection: AgentConnectionContext,
    pub evidence_id: EvidenceId,
    pub coverage: CoverageWindow,
}

#[derive(Clone)]
pub struct IssueEvidenceDocumentUploadGrantHandler {
    repository: Arc<Postgres>,
    public_api_base_url: Url,
    encryptor: UploadGrantEncryptor,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssuedEvidenceDocumentUploadGrant {
    pub url: Url,
    pub expires_at: DateTime<Utc>,
    pub evidence_id: EvidenceId,
    pub coverage: CoverageWindow,
    pub audit: EvidenceDocumentUploadGrantAuditContext,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EvidenceDocumentUploadGrantAuditContext {
    pub workspace_id: WorkspaceId,
    pub evidence_id: EvidenceId,
    pub coverage: CoverageWindow,
    pub issued_by_user_id: UserId,
    pub issued_via: EvidenceDocumentUploadGrantIssuer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvidenceDocumentUploadGrantIssuer {
    AgentConnection(AgentConnectionId),
}

impl EvidenceDocumentUploadGrantIssuer {
    pub fn agent_connection_id(self) -> AgentConnectionId {
        match self {
            Self::AgentConnection(id) => id,
        }
    }
}

impl IssueEvidenceDocumentUploadGrantHandler {
    pub fn new(
        repository: Arc<Postgres>,
        public_api_base_url: Url,
        encryptor: UploadGrantEncryptor,
    ) -> Self {
        Self {
            repository,
            public_api_base_url,
            encryptor,
        }
    }

    pub async fn handle(
        &self,
        command: IssueEvidenceDocumentUploadGrant,
        _metadata: ExecutionMetadata,
    ) -> Result<IssuedEvidenceDocumentUploadGrant, EvidenceDocumentUploadGrantHandlerError> {
        if !command
            .connection
            .permissions
            .has(WorkspacePermission::WriteEvidenceSubmissions)
        {
            return Err(EvidenceDocumentUploadGrantHandlerError::Unavailable);
        }
        let encryptor = self.encryptor.clone();
        let public_api_base_url = self.public_api_base_url.clone();
        let outcome = self
            .repository
            .in_agent_connection_workspace_context(
                command.connection.workspace_id,
                command.connection.user_id,
                command.connection.connection_id,
                async move |context| {
                    if context
                        .evidence_projections()
                        .get(command.evidence_id)
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
                    let grant_id = DocumentUploadGrantId::from(Uuid::new_v4());
                    let issued = match encryptor.encrypt(
                        RegisteredClaims {
                            subject: command.connection.user_id.into(),
                            token_id: grant_id.into(),
                            expires_at,
                        },
                        &EvidenceDocumentUploadGrantClaims::new(
                            grant_id.into(),
                            command.connection.workspace_id.into(),
                            command.evidence_id.into(),
                            command.coverage.valid_from,
                            command.coverage.valid_until,
                            command.connection.user_id.into(),
                            command.connection.connection_id.into(),
                        ),
                    ) {
                        Ok(issued) => issued,
                        Err(_) => return Ok(IssueOutcome::Internal),
                    };
                    let grant = match EvidenceDocumentUploadGrant::issue(
                        grant_id,
                        command.connection.workspace_id,
                        command.evidence_id,
                        command.coverage,
                        command.connection.user_id,
                        command.connection.connection_id,
                        issued_at,
                        issued.expires_at,
                    ) {
                        Ok(grant) => grant,
                        Err(_) => return Ok(IssueOutcome::Internal),
                    };
                    let mut url = match public_api_base_url.join("evidence-document-uploads") {
                        Ok(url) => url,
                        Err(_) => return Ok(IssueOutcome::Internal),
                    };
                    url.query_pairs_mut().append_pair("token", &issued.token);
                    let repository = context.evidence_document_upload_grants();
                    repository.save(&grant).await?;
                    let grant = repository
                        .get(grant.id(), grant.workspace_id())
                        .await?
                        .ok_or(crate::repository::Error::InvariantViolation(
                            "saved evidence human upload grant must be readable",
                        ))?;
                    Ok(IssueOutcome::Issued(Box::new(
                        IssuedEvidenceDocumentUploadGrant {
                            url,
                            expires_at: issued.expires_at,
                            evidence_id: grant.evidence_id(),
                            coverage: grant.coverage(),
                            audit: EvidenceDocumentUploadGrantAuditContext {
                                workspace_id: grant.workspace_id(),
                                evidence_id: grant.evidence_id(),
                                coverage: grant.coverage(),
                                issued_by_user_id: grant.issued_by_user_id(),
                                issued_via: EvidenceDocumentUploadGrantIssuer::AgentConnection(
                                    grant.issued_via_agent_connection_id(),
                                ),
                            },
                        },
                    )))
                },
            )
            .await?;

        match outcome {
            IssueOutcome::Issued(issued) => Ok(*issued),
            IssueOutcome::Unavailable => Err(EvidenceDocumentUploadGrantHandlerError::Unavailable),
            IssueOutcome::Internal => Err(EvidenceDocumentUploadGrantHandlerError::Internal),
        }
    }
}

enum IssueOutcome {
    Issued(Box<IssuedEvidenceDocumentUploadGrant>),
    Unavailable,
    Internal,
}

#[derive(Debug, thiserror::Error)]
pub enum EvidenceDocumentUploadGrantHandlerError {
    #[error("document upload grant is unavailable")]
    Unavailable,
    #[error("internal document upload grant error")]
    Internal,
    #[error("repository error")]
    Repository(#[from] crate::repository::Error),
}
