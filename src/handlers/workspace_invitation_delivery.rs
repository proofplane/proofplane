use std::sync::Arc;

use chrono::Utc;

use crate::{
    domain::{WorkspaceInvitation, WorkspaceInvitationStatus},
    mail::{MailAdapter, MailFailureClass, MailMessage},
    messaging::WorkspaceInvitationDeliveryWork,
    persistence::Postgres,
    services::workspace_invitation_authority::WorkspaceInvitationAuthority,
    worker::{RetryableWorkerError, WorkerMessage},
};

#[derive(Clone)]
pub struct WorkspaceInvitationDeliveryHandler {
    repository: Arc<Postgres>,
    authority: WorkspaceInvitationAuthority,
    mail: Arc<dyn MailAdapter>,
    max_delivery_attempts: u16,
}

impl WorkspaceInvitationDeliveryHandler {
    pub fn new(
        repository: Arc<Postgres>,
        authority: WorkspaceInvitationAuthority,
        mail: Arc<dyn MailAdapter>,
        max_delivery_attempts: u16,
    ) -> Self {
        Self {
            repository,
            authority,
            mail,
            max_delivery_attempts,
        }
    }

    pub async fn handle(&self, message: WorkerMessage) -> Result<(), RetryableWorkerError> {
        let payload =
            match serde_json::from_value::<WorkspaceInvitationDeliveryWork>(message.payload) {
                Ok(payload) if payload.generation > 0 => payload,
                _ => {
                    tracing::warn!("acknowledging malformed workspace invitation delivery command");
                    return Ok(());
                }
            };
        if message.aggregate_id != payload.invitation_id.to_string() {
            tracing::warn!("acknowledging mismatched workspace invitation delivery command");
            return Ok(());
        }

        let final_delivery = message
            .delivery_attempt
            .is_some_and(|attempt| attempt >= u32::from(self.max_delivery_attempts));
        let outcome = self
            .repository
            .in_unit_of_work(async |unit_of_work| {
                let repository = unit_of_work.aggregates().workspace_invitations();
                let Some(mut invitation) = repository.get(payload.invitation_id.into()).await?
                else {
                    return Ok(DeliveryOutcome::Acknowledged);
                };
                let now = Utc::now();
                if invitation.generation() != payload.generation
                    || invitation.status_at(now) != WorkspaceInvitationStatus::Pending
                    || invitation.delivered_generation() == Some(payload.generation)
                {
                    return Ok(DeliveryOutcome::Acknowledged);
                }
                let Some(workspace) = unit_of_work
                    .reads()
                    .workspaces()
                    .get(invitation.workspace_id())
                    .await?
                else {
                    return Ok(DeliveryOutcome::Acknowledged);
                };
                let link = self.authority.issue(&invitation).map_err(|_| {
                    crate::persistence::Error::InvariantViolation(
                        "current invitation authority must issue",
                    )
                })?;
                let idempotency_key = format!(
                    "workspace-invitation/{}/{}",
                    payload.invitation_id, payload.generation
                );
                let mail = invitation_mail(
                    &invitation,
                    &workspace.name,
                    link.url.as_str(),
                    idempotency_key,
                );
                match self.mail.send(mail).await {
                    Ok(()) => {
                        invitation.record_delivery_success(payload.generation, Utc::now());
                        repository.save(&invitation).await?;
                        Ok(DeliveryOutcome::Delivered)
                    }
                    Err(error) => {
                        let failure = if final_delivery {
                            "exhausted"
                        } else if error.class == MailFailureClass::Permanent {
                            "permanent"
                        } else {
                            "retryable"
                        };
                        invitation.record_delivery_failure(payload.generation, failure, Utc::now());
                        repository.save(&invitation).await?;
                        if error.class == MailFailureClass::Retryable && !final_delivery {
                            Ok(DeliveryOutcome::Retry {
                                status_class: error.status_class,
                            })
                        } else {
                            Ok(DeliveryOutcome::Failed {
                                class: error.class,
                                status_class: error.status_class,
                            })
                        }
                    }
                }
            })
            .await
            .map_err(retryable)?;
        match outcome {
            DeliveryOutcome::Acknowledged => tracing::info!(
                outcome = "stale_or_unavailable",
                "workspace invitation delivery acknowledged"
            ),
            DeliveryOutcome::Delivered => tracing::info!(
                provider = "mail",
                outcome = "delivered",
                "workspace invitation delivery completed"
            ),
            DeliveryOutcome::Failed {
                class,
                status_class,
            } => tracing::warn!(
                provider = "mail",
                outcome = class.as_str(),
                status_class,
                "workspace invitation delivery failed"
            ),
            DeliveryOutcome::Retry { status_class } => {
                tracing::warn!(
                    provider = "mail",
                    outcome = "retryable",
                    status_class,
                    "workspace invitation delivery failed"
                );
                return Err(RetryableWorkerError(
                    "mail provider returned a retryable failure".to_owned(),
                ));
            }
        }
        Ok(())
    }
}

enum DeliveryOutcome {
    Acknowledged,
    Delivered,
    Failed {
        class: MailFailureClass,
        status_class: &'static str,
    },
    Retry {
        status_class: &'static str,
    },
}

fn invitation_mail(
    invitation: &WorkspaceInvitation,
    workspace_name: &str,
    url: &str,
    idempotency_key: String,
) -> MailMessage {
    let escaped_workspace = escape_html(workspace_name);
    let escaped_url = escape_html(url);
    MailMessage {
        to: invitation.invited_email().to_owned(),
        subject: format!("Join {workspace_name} on Proofplane"),
        text: format!(
            "You have been invited to join {workspace_name} as an administrator.\n\nOpen this invitation: {url}\n\nThis link expires in seven days."
        ),
        html: format!(
            "<p>You have been invited to join {escaped_workspace} as an administrator.</p><p><a href=\"{escaped_url}\">Open invitation</a></p><p>This link expires in seven days.</p>"
        ),
        idempotency_key,
    }
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn retryable(error: impl ToString) -> RetryableWorkerError {
    RetryableWorkerError(error.to_string())
}
