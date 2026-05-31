use std::ops::AsyncFnOnce;

use deadpool_postgres::{Object, Pool};

use crate::{
    routes::authentication::ActorContext,
    services::{ReadServiceContext, ServiceContext},
};

mod actors;
mod api_credentials;
mod controls;
pub mod error;
mod evidence_requests;
mod evidence_submissions;
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

    pub async fn in_actor_context<T, F>(
        &self,
        actor: ActorContext,
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
        let mut context = ServiceContext::new(actor.workspace_id, actor.id, transaction);
        let result = operation(&mut context).await?;

        context.commit().await?;

        Ok(result)
    }

    pub async fn in_actor_context_read<T, F>(
        &self,
        actor: ActorContext,
        operation: F,
    ) -> Result<T, Error>
    where
        T: Send,
        F: for<'context> AsyncFnOnce(&'context ReadServiceContext) -> Result<T, Error> + Send,
    {
        let client = self.get().await?;
        let context = ReadServiceContext::new(actor.workspace_id, actor.id, client);

        operation(&context).await
    }
}
