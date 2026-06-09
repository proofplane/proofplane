use deadpool_postgres::{Object, Pool};

use crate::domain::{ActorId, WorkspaceId};

mod actors;
mod api_credentials;
mod controls;
pub mod error;
mod evidence_requests;
mod evidence_submissions;
mod outbox;
mod workspaces;

pub use error::Error;
pub use evidence_submissions::{FinalizingAttachmentUploadWork, PendingAttachmentUploadWork};
pub use outbox::{NewOutboxMessage, OutboxMessage};

pub struct Postgres {
    pool: Pool,
}

pub struct TransactionContext<'transaction> {
    transaction: deadpool_postgres::Transaction<'transaction>,
}

pub struct ActorTransactionContext<'transaction> {
    pub workspace_id: WorkspaceId,
    pub actor_id: ActorId,
    transaction: deadpool_postgres::Transaction<'transaction>,
}

impl<'transaction> ActorTransactionContext<'transaction> {
    fn new(
        workspace_id: WorkspaceId,
        actor_id: ActorId,
        transaction: deadpool_postgres::Transaction<'transaction>,
    ) -> Self {
        Self {
            workspace_id,
            actor_id,
            transaction,
        }
    }

    async fn commit(self) -> Result<(), tokio_postgres::Error> {
        self.transaction.commit().await
    }
}

pub struct ActorReadContext {
    pub workspace_id: WorkspaceId,
    pub actor_id: ActorId,
    client: deadpool_postgres::Object,
}

impl ActorReadContext {
    fn new(
        workspace_id: WorkspaceId,
        actor_id: ActorId,
        client: deadpool_postgres::Object,
    ) -> Self {
        Self {
            workspace_id,
            actor_id,
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

    pub async fn in_actor_context<T, F>(
        &self,
        workspace_id: WorkspaceId,
        actor_id: ActorId,
        operation: F,
    ) -> Result<T, Error>
    where
        T: Send,
        F: for<'context, 'transaction> AsyncFnOnce(
                &'context mut ActorTransactionContext<'transaction>,
            ) -> Result<T, Error>
            + Send,
    {
        let mut client = self.get().await?;
        let transaction = client.transaction().await?;
        let mut context = ActorTransactionContext::new(workspace_id, actor_id, transaction);
        let result = operation(&mut context).await?;

        context.commit().await?;

        Ok(result)
    }

    pub async fn in_actor_context_read<T, F>(
        &self,
        workspace_id: WorkspaceId,
        actor_id: ActorId,
        operation: F,
    ) -> Result<T, Error>
    where
        T: Send,
        F: for<'context> AsyncFnOnce(&'context ActorReadContext) -> Result<T, Error> + Send,
    {
        let client = self.get().await?;
        let context = ActorReadContext::new(workspace_id, actor_id, client);

        operation(&context).await
    }
}
