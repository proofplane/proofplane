use std::{collections::HashMap, sync::Arc};

use crate::{
    application::{
        queries::resolve_auditor_session_by_digest::ResolvedAuditorSession, ExecutionMetadata,
    },
    projections::{
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
        for framework in self.repository.framework_projections().list().await? {
            framework_requirements.extend(
                self.repository
                    .framework_projections()
                    .list_requirements(framework.id)
                    .await?,
            );
        }

        let session = query.session;
        let projections = self
            .repository
            .auditor_portal_projections(session.workspace_id);
        let mut controls = projections
            .controls(session.period.start, session.period.end)
            .await?;
        let policies = projections.policies().await?;
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
            .workspace_projections()
            .get(session.workspace_id)
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
