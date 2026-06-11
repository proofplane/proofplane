use std::sync::Arc;

use crate::{
    authentication::ActorContext,
    authorization::spicedb::{ClientError, SpiceDbClient, WorkspacePermission},
};

#[derive(Clone)]
pub struct WorkspaceAuthorizer {
    spicedb: Arc<SpiceDbClient>,
}

impl WorkspaceAuthorizer {
    pub fn new(spicedb: SpiceDbClient) -> Self {
        Self {
            spicedb: Arc::new(spicedb),
        }
    }

    pub async fn can_read_evidence_requests(
        &self,
        actor: &ActorContext,
    ) -> Result<bool, ClientError> {
        self.spicedb
            .check_workspace_permission(actor, WorkspacePermission::ReadEvidenceRequests)
            .await
    }

    pub async fn can_write_evidence_requests(
        &self,
        actor: &ActorContext,
    ) -> Result<bool, ClientError> {
        self.spicedb
            .check_workspace_permission(actor, WorkspacePermission::WriteEvidenceRequests)
            .await
    }

    pub async fn can_read_evidence_submissions(
        &self,
        actor: &ActorContext,
    ) -> Result<bool, ClientError> {
        self.spicedb
            .check_workspace_permission(actor, WorkspacePermission::ReadEvidenceSubmissions)
            .await
    }

    pub async fn can_write_evidence_submissions(
        &self,
        actor: &ActorContext,
    ) -> Result<bool, ClientError> {
        self.spicedb
            .check_workspace_permission(actor, WorkspacePermission::WriteEvidenceSubmissions)
            .await
    }

    pub async fn can_read_controls(&self, actor: &ActorContext) -> Result<bool, ClientError> {
        self.spicedb
            .check_workspace_permission(actor, WorkspacePermission::ReadControls)
            .await
    }

    pub async fn can_write_controls(&self, actor: &ActorContext) -> Result<bool, ClientError> {
        self.spicedb
            .check_workspace_permission(actor, WorkspacePermission::WriteControls)
            .await
    }
}
