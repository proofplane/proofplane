use std::sync::{Arc, Mutex};

use crate::{
    authorization::spicedb::{ClientError, SpiceDbClient, WorkspacePermission},
    domain::WorkspaceId,
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
        actor_id: &str,
        workspace_id: WorkspaceId,
    ) -> Result<bool, ClientError> {
        let spicedb = self.spicedb();
        spicedb
            .check_workspace_permission(
                workspace_id,
                WorkspacePermission::ReadEvidenceRequests,
                actor_id,
            )
            .await
    }

    pub async fn can_write_evidence_requests(
        &self,
        actor_id: &str,
        workspace_id: WorkspaceId,
    ) -> Result<bool, ClientError> {
        let spicedb = self.spicedb();
        spicedb
            .check_workspace_permission(
                workspace_id,
                WorkspacePermission::WriteEvidenceRequests,
                actor_id,
            )
            .await
    }

    fn spicedb(&self) -> SpiceDbClient {
        self.spicedb
            .lock()
            .expect("SpiceDB authorizer client mutex poisoned")
            .clone()
    }
}
