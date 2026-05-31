use crate::domain::{ActorId, WorkspaceId};
use thiserror::Error;

pub mod controls;
pub mod evidence_requests;

#[derive(Debug, Error)]
pub enum Error {
    #[error("repository error")]
    Repository(#[from] crate::repository::Error),

    #[error("invalid framework requirement references")]
    InvalidFrameworkRequirementReferences,
}

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
