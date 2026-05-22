use std::ops::AsyncFnOnce;

use deadpool_postgres::{Object, Pool};

use crate::{domain::WorkspaceId, services::ServiceContext};

mod actors;
mod api_credentials;
pub mod error;
mod evidence_requests;
mod workspaces;

pub use error::Error;

pub struct Postgres {
    pool: Pool,
}

impl Postgres {
    pub fn new(pool: Pool) -> Self {
        Self { pool }
    }

    pub async fn get(&self) -> Result<Object, deadpool_postgres::PoolError> {
        self.pool.get().await
    }

    pub async fn in_workspace<T, F>(
        &self,
        workspace_id: WorkspaceId,
        operation: F,
    ) -> Result<T, Error>
    where
        T: Send,
        F: for<'context, 'transaction> AsyncFnOnce(
                &'context mut ServiceContext<'transaction>,
            ) -> Result<T, Error>
            + Send,
    {
        let mut client = self.get().await?;
        let transaction = client.transaction().await?;
        let mut context = ServiceContext::new(workspace_id, transaction);
        let result = operation(&mut context).await?;

        context.commit().await?;

        Ok(result)
    }
}
