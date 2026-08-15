//! Concrete Postgres persistence.
//!
//! Aggregate repositories rehydrate and save complete domain snapshots.
//! Read gateways return purpose-built read models without rehydrating mutable
//! aggregates. Both can share a transaction through [`UnitOfWork`].

use deadpool_postgres::{Object, Pool};

use crate::domain::WorkspaceId;

mod aggregates;
pub mod connection;
pub mod constraints;
pub mod error;
mod migrate;
mod outbox;
mod params;
mod reads;
mod snapshot;
#[cfg(test)]
pub(crate) mod test_support;

pub use crate::read_models::{PolicyDocumentUploadEligibility, TypedDocumentUploadWork};
pub use aggregates::{
    AgentConnectionRepository, AgentEvidenceUploadGrantRepository,
    AgentPolicyDocumentUploadGrantRepository, ArchiveDocumentResult, ArchivePolicyResult,
    AuditorAccessGrantRepository, AuditorAuthTransactionRepository, AuditorSessionRepository,
    CreatePolicyDocumentResult, DocumentRepository, EvidenceDocumentUploadGrantRepository,
    EvidenceSubmissionRepository, NewWorkspaceMembership, OAuthAuthorizationFlowRepository,
    PolicyDocumentUploadGrantRepository, UserRepository, WorkspaceDocumentRepository,
    WorkspaceRepository,
};
pub use connection::{conn, conn_pool, PoolBounds, PoolRuntime};
pub use constraints::ConflictKind;
pub use error::{BatchMapRejection, BatchUnmapRejection, Error};
pub use migrate::{
    apply_migrations, check_schema_revision, migration_runner, set_migration_lock_timeout,
    SchemaRevisionError, MIGRATION_LOCK_TIMEOUT,
};
pub use outbox::{NewOutboxMessage, OutboxMessage};
pub(crate) use params::param;

pub struct Postgres {
    pool: Pool,
}

pub struct UnitOfWork<'transaction> {
    transaction: deadpool_postgres::Transaction<'transaction>,
}

/// Workspace-scoped persistence available inside one database transaction.
/// Aggregate repositories and transactional reads are deliberately separate.
pub struct WorkspaceUnitOfWork<'unit_of_work> {
    pub(super) workspace_id: WorkspaceId,
    pub(super) transaction: &'unit_of_work deadpool_postgres::Transaction<'unit_of_work>,
}

impl<'unit_of_work> UnitOfWork<'unit_of_work> {
    pub(crate) fn reads(
        &'unit_of_work self,
    ) -> reads::Reads<reads::TransactionalReadExecutor<'unit_of_work>> {
        reads::Reads::new(reads::TransactionalReadExecutor::new(&self.transaction))
    }

    pub fn workspace(
        &'unit_of_work self,
        workspace_id: WorkspaceId,
    ) -> WorkspaceUnitOfWork<'unit_of_work> {
        WorkspaceUnitOfWork {
            workspace_id,
            transaction: &self.transaction,
        }
    }

    pub fn aggregates(&self) -> &Self {
        self
    }

    fn new(transaction: deadpool_postgres::Transaction<'unit_of_work>) -> Self {
        Self { transaction }
    }

    async fn commit(self) -> Result<(), tokio_postgres::Error> {
        self.transaction.commit().await
    }
}

impl WorkspaceUnitOfWork<'_> {
    pub fn workspace_id(&self) -> WorkspaceId {
        self.workspace_id
    }

    pub fn aggregates(&self) -> &Self {
        self
    }

    pub(crate) fn reads(
        &self,
    ) -> reads::WorkspaceScopedReads<reads::TransactionalReadExecutor<'_>> {
        reads::WorkspaceScopedReads::new(
            reads::TransactionalReadExecutor::new(self.transaction),
            self.workspace_id,
        )
    }
}

impl Postgres {
    pub fn new(pool: Pool) -> Self {
        Self { pool }
    }

    pub async fn get(&self) -> Result<Object, deadpool_postgres::PoolError> {
        self.pool.get().await
    }

    pub fn aggregates(&self) -> &Self {
        self
    }

    pub(crate) async fn reads(&self) -> Result<reads::Reads<reads::PooledReadExecutor>, Error> {
        Ok(reads::Reads::new(reads::PooledReadExecutor::new(
            self.get().await?,
        )))
    }

    pub(crate) async fn workspace_reads(
        &self,
        workspace_id: WorkspaceId,
    ) -> Result<reads::WorkspaceScopedReads<reads::PooledReadExecutor>, Error> {
        Ok(reads::WorkspaceScopedReads::new(
            reads::PooledReadExecutor::new(self.get().await?),
            workspace_id,
        ))
    }

    pub async fn in_unit_of_work<T, F>(&self, operation: F) -> Result<T, Error>
    where
        T: Send,
        F: for<'unit_of_work, 'transaction> AsyncFnOnce(
                &'unit_of_work UnitOfWork<'transaction>,
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
}
