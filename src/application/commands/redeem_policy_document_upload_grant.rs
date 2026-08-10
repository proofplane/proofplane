use std::sync::Arc;

use chrono::{DateTime, Utc};

use crate::{
    application::{
        commands::issue_policy_document_upload_grant::PolicyDocumentUploadGrantHandlerError,
        ExecutionMetadata,
    },
    domain::{
        AgentConnectionId, PolicyDocumentUploadGrantAuthority, PolicyDocumentUploadGrantId,
        PolicyId, UserId, WorkspaceId,
    },
    persistence::Postgres,
};

#[derive(Debug, Clone, Copy)]
pub struct RedeemPolicyDocumentUploadGrant {
    pub authority: PolicyDocumentUploadGrantAuthority,
}

#[derive(Clone)]
pub struct RedeemPolicyDocumentUploadGrantHandler {
    repository: Arc<Postgres>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RedeemedPolicyDocumentUploadGrant {
    pub id: PolicyDocumentUploadGrantId,
    pub workspace_id: WorkspaceId,
    pub policy_id: PolicyId,
    pub issued_by_user_id: UserId,
    pub issued_via_agent_connection_id: AgentConnectionId,
    pub expires_at: DateTime<Utc>,
    pub redeemed_at: DateTime<Utc>,
}

impl RedeemPolicyDocumentUploadGrantHandler {
    pub fn new(repository: Arc<Postgres>) -> Self {
        Self { repository }
    }

    pub async fn handle(
        &self,
        command: RedeemPolicyDocumentUploadGrant,
        _metadata: ExecutionMetadata,
    ) -> Result<RedeemedPolicyDocumentUploadGrant, PolicyDocumentUploadGrantHandlerError> {
        let outcome =
            self.repository
                .in_unit_of_work(async move |unit_of_work| {
                    let workspace = unit_of_work.workspace(command.authority.workspace_id());
                    if !workspace
                        .reads()
                        .policies()
                        .is_active(command.authority.policy_id())
                        .await?
                    {
                        return Ok(RedeemOutcome::Unavailable);
                    }
                    let repository = unit_of_work.aggregates().policy_document_upload_grants();
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
                        .ok_or(crate::persistence::Error::InvariantViolation(
                            "redeemed policy human upload grant must be readable",
                        ))?;
                    let redeemed_at = grant.redeemed_at().ok_or(
                        crate::persistence::Error::InvariantViolation(
                            "redeemed policy human upload grant has a redemption time",
                        ),
                    )?;
                    Ok(RedeemOutcome::Redeemed(RedeemedPolicyDocumentUploadGrant {
                        id: grant.id(),
                        workspace_id: grant.workspace_id(),
                        policy_id: grant.policy_id(),
                        issued_by_user_id: grant.issued_by_user_id(),
                        issued_via_agent_connection_id: grant.issued_via_agent_connection_id(),
                        expires_at: grant.expires_at(),
                        redeemed_at,
                    }))
                })
                .await?;

        match outcome {
            RedeemOutcome::Redeemed(redeemed) => Ok(redeemed),
            RedeemOutcome::Unavailable => Err(PolicyDocumentUploadGrantHandlerError::Unavailable),
        }
    }
}

enum RedeemOutcome {
    Redeemed(RedeemedPolicyDocumentUploadGrant),
    Unavailable,
}
