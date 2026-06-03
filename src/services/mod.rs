use crate::domain::{ActorId, WorkspaceId};
use thiserror::Error;

pub mod controls;
pub mod evidence_requests;
pub mod evidence_submissions;

#[derive(Debug, Error)]
pub enum Error {
    #[error("repository error")]
    Repository(#[from] crate::repository::Error),

    #[error("object storage error")]
    Storage(#[from] crate::object_storage::StorageError),

    #[error("invalid framework requirement references")]
    InvalidFrameworkRequirementReferences,
}

// TODO: see if it's a bad idea for the repository layer to be importing from the
// service layer like this instead of having the service layer just import and
// call repository APIs.
pub struct ServiceContext<'transaction> {
    pub workspace_id: WorkspaceId,
    pub actor_id: ActorId,
    pub(crate) transaction: deadpool_postgres::Transaction<'transaction>,
}

impl<'transaction> ServiceContext<'transaction> {
    pub(crate) fn new(
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

    pub(crate) async fn commit(self) -> Result<(), tokio_postgres::Error> {
        self.transaction.commit().await
    }
}

pub struct ReadServiceContext {
    pub workspace_id: WorkspaceId,
    pub actor_id: ActorId,
    pub(crate) client: deadpool_postgres::Object,
}

impl ReadServiceContext {
    pub(crate) fn new(
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
