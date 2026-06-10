use std::sync::{Arc, Mutex};

use crate::{
    authentication::ActorContext,
    authorization::spicedb::{
        ClientError, SpiceDbClient, UserWorkspacePermission, WorkspacePermission,
    },
    domain::{WorkspaceId, WorkspaceRole},
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

    pub async fn can_read_evidence_submissions(
        &self,
        actor: &ActorContext,
    ) -> Result<bool, ClientError> {
        let spicedb = self.spicedb();
        spicedb
            .check_workspace_permission(actor, WorkspacePermission::ReadEvidenceSubmissions)
            .await
    }

    pub async fn can_write_evidence_submissions(
        &self,
        actor: &ActorContext,
    ) -> Result<bool, ClientError> {
        let spicedb = self.spicedb();
        spicedb
            .check_workspace_permission(actor, WorkspacePermission::WriteEvidenceSubmissions)
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

    pub async fn write_user_role(
        &self,
        workspace_id: WorkspaceId,
        user_id: &str,
        role: WorkspaceRole,
    ) -> Result<(), ClientError> {
        let spicedb = self.spicedb();
        spicedb
            .write_workspace_user_role(workspace_id, user_id, role)
            .await
    }

    pub async fn delete_user_role(
        &self,
        workspace_id: WorkspaceId,
        user_id: &str,
        role: WorkspaceRole,
    ) -> Result<(), ClientError> {
        let spicedb = self.spicedb();
        spicedb
            .delete_workspace_user_role(workspace_id, user_id, role)
            .await
    }

    pub async fn can_manage_members(
        &self,
        workspace_id: WorkspaceId,
        user_id: &str,
    ) -> Result<bool, ClientError> {
        let spicedb = self.spicedb();
        spicedb
            .check_user_workspace_permission(
                workspace_id,
                user_id,
                UserWorkspacePermission::ManageMembers,
            )
            .await
    }

    pub async fn can_manage_workspace(
        &self,
        workspace_id: WorkspaceId,
        user_id: &str,
    ) -> Result<bool, ClientError> {
        let spicedb = self.spicedb();
        spicedb
            .check_user_workspace_permission(
                workspace_id,
                user_id,
                UserWorkspacePermission::ManageWorkspace,
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
