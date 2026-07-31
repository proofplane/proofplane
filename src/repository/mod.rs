use deadpool_postgres::{Object, Pool};

use uuid::Uuid;

use crate::domain::{AgentConnectionId, UserId, WorkspaceId};

mod agent_connections;
mod agent_evidence_upload_grants;
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
mod users;
mod workspace_memberships;
mod workspaces;

pub use agent_evidence_upload_grants::{AgentEvidenceUploadGrant, NewAgentEvidenceUploadGrant};
pub use auditor_access_sessions::NewAuditorSession;
pub use auditor_auth_transactions::{ClaimedAuditorAuthTransaction, NewAuditorAuthTransaction};
pub use constraints::ConflictKind;
pub use document_upload_grants::{DocumentUploadGrant, NewDocumentUploadGrant};
pub use documents::TypedDocumentUploadWork;
pub use error::{BatchMapRejection, BatchUnmapRejection, Error};
pub use evidence_submissions::{
    ArchiveDocumentResult, DocumentDownloadCandidate, FinalizingDocumentUploadWork,
    PendingDocumentUploadWork,
};
pub use outbox::{NewOutboxMessage, OutboxMessage};
pub use policies::{ArchivePolicyResult, CreatePolicyDocumentResult};
pub use policy_document_upload_grants::{NewPolicyDocumentUploadGrant, PolicyDocumentUploadGrant};
pub use workspace_memberships::NewWorkspaceMembership;

pub struct Postgres {
    pool: Pool,
}

pub struct TransactionContext<'transaction> {
    transaction: deadpool_postgres::Transaction<'transaction>,
}

pub struct WorkspaceTransactionContext<'transaction> {
    pub workspace_id: WorkspaceId,
    pub user_id: UserId,
    pub credential: WorkspaceCredential,
    transaction: deadpool_postgres::Transaction<'transaction>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceCredential {
    AgentConnection(AgentConnectionId),
}

impl WorkspaceCredential {
    pub fn agent_connection_uuid(self) -> Option<Uuid> {
        match self {
            Self::AgentConnection(id) => Some(id.into()),
        }
    }
}

impl<'transaction> WorkspaceTransactionContext<'transaction> {
    fn new(
        workspace_id: WorkspaceId,
        user_id: UserId,
        credential: WorkspaceCredential,
        transaction: deadpool_postgres::Transaction<'transaction>,
    ) -> Self {
        Self {
            workspace_id,
            user_id,
            credential,
            transaction,
        }
    }

    async fn commit(self) -> Result<(), tokio_postgres::Error> {
        self.transaction.commit().await
    }
}

pub struct WorkspaceReadContext {
    pub workspace_id: WorkspaceId,
    client: deadpool_postgres::Object,
}

impl WorkspaceReadContext {
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

    pub async fn in_transaction<T, F>(&self, operation: F) -> Result<T, Error>
    where
        T: Send,
        F: for<'context, 'transaction> AsyncFnOnce(
                &'context mut TransactionContext<'transaction>,
            ) -> Result<T, Error>
            + Send,
    {
        let mut client = self.get().await?;
        let transaction = client.transaction().await?;
        let mut context = TransactionContext { transaction };
        let result = operation(&mut context).await?;

        context.transaction.commit().await?;

        Ok(result)
    }

    pub async fn in_agent_connection_workspace_context<T, F>(
        &self,
        workspace_id: WorkspaceId,
        user_id: UserId,
        agent_connection_id: AgentConnectionId,
        operation: F,
    ) -> Result<T, Error>
    where
        T: Send,
        F: for<'context, 'transaction> AsyncFnOnce(
                &'context mut WorkspaceTransactionContext<'transaction>,
            ) -> Result<T, Error>
            + Send,
    {
        let mut client = self.get().await?;
        let transaction = client.transaction().await?;
        let mut context = WorkspaceTransactionContext::new(
            workspace_id,
            user_id,
            WorkspaceCredential::AgentConnection(agent_connection_id),
            transaction,
        );
        let result = operation(&mut context).await?;

        context.commit().await?;

        Ok(result)
    }

    pub async fn in_workspace_context_read<T, F>(
        &self,
        workspace_id: WorkspaceId,
        operation: F,
    ) -> Result<T, Error>
    where
        T: Send,
        F: for<'context> AsyncFnOnce(&'context WorkspaceReadContext) -> Result<T, Error> + Send,
    {
        let client = self.get().await?;
        let context = WorkspaceReadContext::new(workspace_id, client);

        operation(&context).await
    }
}
