use std::sync::{Arc, Mutex};

use crate::{
    authorization::spicedb::{ClientError, SpiceDbClient, WorkspacePermission},
    routes::authentication::ActorContext,
};

#[derive(Clone)]
pub struct WorkspaceAuthorizer {
    spicedb: Arc<Mutex<SpiceDbClient>>,
}

impl WorkspaceAuthorizer {
    pub fn new(spicedb: SpiceDbClient) -> Self {
        Self {
            spicedb: Arc::new(Mutex::new(spicedb)),
        }
    }

    pub async fn can_read_evidence_requests(
        &self,
        actor: &ActorContext,
    ) -> Result<bool, ClientError> {
        let spicedb = self.spicedb();
        spicedb
            .check_workspace_permission(actor, WorkspacePermission::ReadEvidenceRequests)
            .await
    }

    pub async fn can_write_evidence_requests(
        &self,
        actor: &ActorContext,
    ) -> Result<bool, ClientError> {
        let spicedb = self.spicedb();
        spicedb
            .check_workspace_permission(actor, WorkspacePermission::WriteEvidenceRequests)
            .await
    }

    pub async fn can_read_controls(&self, actor: &ActorContext) -> Result<bool, ClientError> {
        let spicedb = self.spicedb();
        spicedb
            .check_workspace_permission(actor, WorkspacePermission::ReadControls)
            .await
    }

    pub async fn can_write_controls(&self, actor: &ActorContext) -> Result<bool, ClientError> {
        let spicedb = self.spicedb();
        spicedb
            .check_workspace_permission(actor, WorkspacePermission::WriteControls)
            .await
    }

    fn spicedb(&self) -> SpiceDbClient {
        self.spicedb
            .lock()
            .expect("SpiceDB authorizer client mutex poisoned")
            .clone()
    }
}
