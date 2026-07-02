use std::sync::Arc;

use crate::{
    domain::{AuditorPortalReadModel, AuditorSession},
    repository::Postgres,
    services::Error,
};

#[derive(Clone)]
pub struct AuditorPortalReadModelService {
    repository: Arc<Postgres>,
}

impl AuditorPortalReadModelService {
    pub fn new(repository: Arc<Postgres>) -> Self {
        Self { repository }
    }

    pub async fn read_model(
        &self,
        session: &AuditorSession,
    ) -> Result<AuditorPortalReadModel, Error> {
        let controls = self
            .repository
            .in_workspace_context_read(session.workspace_id, async |context| {
                context.auditor_portal_controls().await
            })
            .await?;

        Ok(AuditorPortalReadModel {
            workspace_id: session.workspace_id,
            auditor_email: session.auditor_email.clone(),
            controls,
        })
    }
}
