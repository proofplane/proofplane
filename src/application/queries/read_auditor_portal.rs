use std::{collections::HashMap, sync::Arc};

use crate::{
    application::{
        queries::resolve_auditor_session_by_digest::ResolvedAuditorSession, ExecutionMetadata,
    },
    domain::{
        AuditorPortalPolicyDocumentStatus, AuditorPortalPolicySummary, AuditorPortalReadModel,
    },
    repository::{Error, Postgres},
};

#[derive(Debug, Clone)]
pub struct ReadAuditorPortal {
    pub session: ResolvedAuditorSession,
}

#[derive(Clone)]
pub struct ReadAuditorPortalHandler {
    repository: Arc<Postgres>,
}

impl ReadAuditorPortalHandler {
    pub fn new(repository: Arc<Postgres>) -> Self {
        Self { repository }
    }

    pub async fn handle(
        &self,
        query: ReadAuditorPortal,
        _metadata: ExecutionMetadata,
    ) -> Result<AuditorPortalReadModel, Error> {
        let mut framework_requirements = Vec::new();
        for framework in self.repository.list_frameworks().await? {
            framework_requirements.extend(
                self.repository
                    .list_framework_requirements(framework.id)
                    .await?,
            );
        }

        let session = query.session;
        let (mut controls, policies) = self
            .repository
            .in_workspace_context_read(session.workspace_id, async |context| {
                let controls = context
                    .auditor_portal_controls(session.period.start, session.period.end)
                    .await?;
                let policies = context.auditor_portal_policies().await?;
                Ok((controls, policies))
            })
            .await?;
        let control_indices = controls
            .iter()
            .enumerate()
            .map(|(index, control)| (control.id, index))
            .collect::<HashMap<_, _>>();
        for policy in &policies {
            let summary = AuditorPortalPolicySummary {
                id: policy.id,
                name: policy.name.clone(),
                description: policy.description.clone(),
                document: policy.document.as_ref().map(|document| {
                    AuditorPortalPolicyDocumentStatus {
                        upload_status: document.upload_status,
                    }
                }),
            };
            for mapped_control in &policy.controls {
                let Some(control) = control_indices
                    .get(&mapped_control.id)
                    .and_then(|index| controls.get_mut(*index))
                else {
                    continue;
                };
                control.policies.push(summary.clone());
            }
        }

        let workspace = self
            .repository
            .get_workspace(session.workspace_id)
            .await?
            .ok_or(Error::InvariantViolation(
                "auditor session workspace is missing",
            ))?;

        Ok(AuditorPortalReadModel {
            workspace_id: session.workspace_id,
            workspace_name: workspace.name,
            auditor_email: session.auditor_email,
            framework_requirements,
            controls,
            policies,
        })
    }
}
