use deadpool_postgres::{Object, Pool};

use crate::domain::WorkspaceId;

mod agent_connections;
mod agent_evidence_upload_grants;
mod agent_policy_document_upload_grants;
mod auditor_access_grants;
mod auditor_access_sessions;
mod auditor_auth_transactions;
mod auditor_portal;
pub mod constraints;
mod controls;
mod document_upload_grants;
mod documents;
pub mod error;
mod evidence;
mod evidence_submissions;
mod oauth;
mod outbox;
mod policies;
mod policy_document_upload_grants;
mod projections;
mod snapshot;
#[cfg(test)]
pub(crate) mod test_support;
mod users;
mod workspace_memberships;
mod workspaces;

pub use agent_connections::AgentConnectionRepository;
pub use agent_evidence_upload_grants::AgentEvidenceUploadGrantRepository;
pub use agent_policy_document_upload_grants::AgentPolicyDocumentUploadGrantRepository;
pub use auditor_access_grants::AuditorAccessGrantRepository;
pub use auditor_access_sessions::AuditorSessionRepository;
pub use auditor_auth_transactions::AuditorAuthTransactionRepository;
pub use constraints::ConflictKind;
pub use document_upload_grants::EvidenceDocumentUploadGrantRepository;
pub use documents::{DocumentRepository, TypedDocumentUploadWork, WorkspaceDocumentRepository};
pub use error::{BatchMapRejection, BatchUnmapRejection, Error};
pub use evidence_submissions::{ArchiveDocumentResult, EvidenceSubmissionRepository};
pub use oauth::OAuthAuthorizationFlowRepository;
pub use outbox::{NewOutboxMessage, OutboxMessage};
pub use policies::{
    ArchivePolicyResult, CreatePolicyDocumentResult, PolicyDocumentUploadEligibility,
};
pub use policy_document_upload_grants::PolicyDocumentUploadGrantRepository;
pub use users::UserRepository;
pub use workspace_memberships::NewWorkspaceMembership;
pub use workspaces::WorkspaceRepository;

pub struct Postgres {
    pool: Pool,
}

pub struct UnitOfWork<'transaction> {
    transaction: deadpool_postgres::Transaction<'transaction>,
}

pub struct WorkspaceRepositories<'unit_of_work> {
    pub(super) workspace_id: WorkspaceId,
    pub(super) transaction: &'unit_of_work deadpool_postgres::Transaction<'unit_of_work>,
}

impl<'unit_of_work> UnitOfWork<'unit_of_work> {
    pub fn for_workspace(
        &'unit_of_work self,
        workspace_id: WorkspaceId,
    ) -> WorkspaceRepositories<'unit_of_work> {
        WorkspaceRepositories {
            workspace_id,
            transaction: &self.transaction,
        }
    }

    fn new(transaction: deadpool_postgres::Transaction<'unit_of_work>) -> Self {
        Self { transaction }
    }

    async fn commit(self) -> Result<(), tokio_postgres::Error> {
        self.transaction.commit().await
    }
}

impl WorkspaceRepositories<'_> {
    pub fn workspace_id(&self) -> WorkspaceId {
        self.workspace_id
    }
}

pub(super) struct WorkspaceClient {
    pub(super) workspace_id: WorkspaceId,
    pub(super) client: deadpool_postgres::Object,
}

impl WorkspaceClient {
    fn new(workspace_id: WorkspaceId, client: deadpool_postgres::Object) -> Self {
        Self {
            workspace_id,
            client,
        }
    }
}

impl Postgres {
    pub fn new(pool: Pool) -> Self {
        Self { pool }
    }

    pub async fn get(&self) -> Result<Object, deadpool_postgres::PoolError> {
        self.pool.get().await
    }

    pub async fn in_unit_of_work<T, F>(&self, operation: F) -> Result<T, Error>
    where
        T: Send,
        F: for<'context, 'transaction> AsyncFnOnce(
                &'context UnitOfWork<'transaction>,
            ) -> Result<T, Error>
            + Send,
    {
        let mut client = self.get().await?;
        let transaction = client.transaction().await?;
        let unit_of_work = UnitOfWork::new(transaction);
        let result = operation(&unit_of_work).await?;

        unit_of_work.commit().await?;

        Ok(result)
    }

    pub(super) async fn with_workspace_client<T, F>(
        &self,
        workspace_id: WorkspaceId,
        operation: F,
    ) -> Result<T, Error>
    where
        T: Send,
        F: for<'context> AsyncFnOnce(&'context WorkspaceClient) -> Result<T, Error> + Send,
    {
        let client = self.get().await?;
        let context = WorkspaceClient::new(workspace_id, client);
        operation(&context).await
    }
}
