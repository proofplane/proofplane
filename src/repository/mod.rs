use deadpool_postgres::{Object, Pool};

use crate::domain::{ApiTokenId, UserId, WorkspaceId};

mod api_tokens;
pub mod constraints;
mod controls;
pub mod error;
mod evidence_requests;
mod evidence_submissions;
mod outbox;
mod users;
mod workspace_memberships;
mod workspaces;

pub use constraints::ConflictKind;
pub use error::Error;
pub use evidence_submissions::{
    AttachmentDownloadCandidate, FinalizingAttachmentUploadWork, PendingAttachmentUploadWork,
};
pub use outbox::{NewOutboxMessage, OutboxMessage};
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
    pub api_token_id: ApiTokenId,
    transaction: deadpool_postgres::Transaction<'transaction>,
}

impl<'transaction> WorkspaceTransactionContext<'transaction> {
    fn new(
        workspace_id: WorkspaceId,
        user_id: UserId,
        api_token_id: ApiTokenId,
        transaction: deadpool_postgres::Transaction<'transaction>,
    ) -> Self {
        Self {
            workspace_id,
            user_id,
            api_token_id,
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

    pub async fn in_workspace_context<T, F>(
        &self,
        workspace_id: WorkspaceId,
        user_id: UserId,
        api_token_id: ApiTokenId,
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
        let mut context =
            WorkspaceTransactionContext::new(workspace_id, user_id, api_token_id, transaction);
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
