use std::sync::Arc;

use chrono::{DateTime, Utc};

use crate::{
    application::{
        commands::issue_evidence_document_upload_grant::{
            EvidenceDocumentUploadGrantHandlerError, EvidenceDocumentUploadGrantIssuer,
        },
        ExecutionMetadata,
    },
    domain::{
        DocumentUploadGrantId, EvidenceDocumentUploadGrantAuthority, EvidenceId, UserId,
        WorkspaceId,
    },
    repository::Postgres,
};

#[derive(Debug, Clone, Copy)]
pub struct RedeemEvidenceDocumentUploadGrant {
    pub authority: EvidenceDocumentUploadGrantAuthority,
}

#[derive(Clone)]
pub struct RedeemEvidenceDocumentUploadGrantHandler {
    repository: Arc<Postgres>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RedeemedEvidenceDocumentUploadGrant {
    pub id: DocumentUploadGrantId,
    pub workspace_id: WorkspaceId,
    pub evidence_id: EvidenceId,
    pub coverage: crate::domain::CoverageWindow,
    pub issued_by_user_id: UserId,
    pub issued_via: EvidenceDocumentUploadGrantIssuer,
    pub expires_at: DateTime<Utc>,
    pub redeemed_at: DateTime<Utc>,
}

impl RedeemEvidenceDocumentUploadGrantHandler {
    pub fn new(repository: Arc<Postgres>) -> Self {
        Self { repository }
    }

    pub async fn handle(
        &self,
        command: RedeemEvidenceDocumentUploadGrant,
        _metadata: ExecutionMetadata,
    ) -> Result<RedeemedEvidenceDocumentUploadGrant, EvidenceDocumentUploadGrantHandlerError> {
        let outcome = self
            .repository
            .in_transaction(async move |context| {
                let repository = context.evidence_document_upload_grants();
                let Some(mut grant) = repository
                    .get(command.authority.id(), command.authority.workspace_id())
                    .await?
                else {
                    return Ok(RedeemOutcome::Unavailable);
                };
                if grant.redeem(command.authority, Utc::now()).is_err() {
                    return Ok(RedeemOutcome::Unavailable);
                }
                repository.save(&grant).await?;
                let grant = repository
                    .get(grant.id(), grant.workspace_id())
                    .await?
                    .ok_or(crate::repository::Error::InvariantViolation(
                        "redeemed evidence human upload grant must be readable",
                    ))?;
                let redeemed_at =
                    grant
                        .redeemed_at()
                        .ok_or(crate::repository::Error::InvariantViolation(
                            "redeemed evidence human upload grant has a redemption time",
                        ))?;
                Ok(RedeemOutcome::Redeemed(
                    RedeemedEvidenceDocumentUploadGrant {
                        id: grant.id(),
                        workspace_id: grant.workspace_id(),
                        evidence_id: grant.evidence_id(),
                        coverage: grant.coverage(),
                        issued_by_user_id: grant.issued_by_user_id(),
                        issued_via: EvidenceDocumentUploadGrantIssuer::AgentConnection(
                            grant.issued_via_agent_connection_id(),
                        ),
                        expires_at: grant.expires_at(),
                        redeemed_at,
                    },
                ))
            })
            .await?;

        match outcome {
            RedeemOutcome::Redeemed(redeemed) => Ok(redeemed),
            RedeemOutcome::Unavailable => Err(EvidenceDocumentUploadGrantHandlerError::Unavailable),
        }
    }
}

enum RedeemOutcome {
    Redeemed(RedeemedEvidenceDocumentUploadGrant),
    Unavailable,
}
