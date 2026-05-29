use std::sync::{Arc, Mutex};

use crate::{
    authorization::spicedb::{ClientError, SpiceDbClient, WorkspacePermission},
    domain::ActorContext,
};

#[derive(Clone)]
pub struct EvidenceRequestAuthorizer {
    spicedb: Arc<Mutex<SpiceDbClient>>,
}

impl EvidenceRequestAuthorizer {
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

    // TODO: should reading and writing controls go in their own file?
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
