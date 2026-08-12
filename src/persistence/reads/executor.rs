use async_trait::async_trait;
use deadpool_postgres::Object;
use tokio::sync::Mutex;
use tokio_postgres::{types::ToSql, Row};

#[async_trait]
pub(crate) trait ReadExecutor: Send + Sync {
    async fn query(
        &self,
        statement: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> Result<Vec<Row>, tokio_postgres::Error>;

    async fn query_one(
        &self,
        statement: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> Result<Row, tokio_postgres::Error>;

    async fn query_opt(
        &self,
        statement: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> Result<Option<Row>, tokio_postgres::Error>;
}

#[async_trait]
impl<T: ReadExecutor + ?Sized> ReadExecutor for &T {
    async fn query(
        &self,
        statement: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> Result<Vec<Row>, tokio_postgres::Error> {
        (*self).query(statement, params).await
    }

    async fn query_one(
        &self,
        statement: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> Result<Row, tokio_postgres::Error> {
        (*self).query_one(statement, params).await
    }

    async fn query_opt(
        &self,
        statement: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> Result<Option<Row>, tokio_postgres::Error> {
        (*self).query_opt(statement, params).await
    }
}

/// Reads outside a [`crate::persistence::UnitOfWork`].
///
/// Each statement runs in a transaction of its own. That is not for atomicity —
/// a lone statement is already atomic — but because `tokio_postgres` prepares a
/// named statement for every query, and a transaction pooler only keeps one
/// server connection assigned for the length of a transaction. Left in
/// autocommit, the `Parse` and the `Bind` land on different backends and the
/// second fails with `prepared statement "sN" does not exist`.
///
/// The `Mutex` is what lets a `&self` trait method reach the `&mut` that
/// `transaction()` needs; it serializes statements on a handle that already
/// shared one connection.
pub(crate) struct PooledReadExecutor {
    client: Mutex<Object>,
}

impl PooledReadExecutor {
    pub(crate) fn new(client: Object) -> Self {
        Self {
            client: Mutex::new(client),
        }
    }
}

#[async_trait]
impl ReadExecutor for PooledReadExecutor {
    async fn query(
        &self,
        statement: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> Result<Vec<Row>, tokio_postgres::Error> {
        let mut client = self.client.lock().await;
        let transaction = client.transaction().await?;
        let rows = transaction.query(statement, params).await?;
        transaction.commit().await?;

        Ok(rows)
    }

    async fn query_one(
        &self,
        statement: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> Result<Row, tokio_postgres::Error> {
        let mut client = self.client.lock().await;
        let transaction = client.transaction().await?;
        let row = transaction.query_one(statement, params).await?;
        transaction.commit().await?;

        Ok(row)
    }

    async fn query_opt(
        &self,
        statement: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> Result<Option<Row>, tokio_postgres::Error> {
        let mut client = self.client.lock().await;
        let transaction = client.transaction().await?;
        let row = transaction.query_opt(statement, params).await?;
        transaction.commit().await?;

        Ok(row)
    }
}

pub(crate) struct TransactionalReadExecutor<'a> {
    transaction: &'a deadpool_postgres::Transaction<'a>,
}

impl<'a> TransactionalReadExecutor<'a> {
    pub(crate) fn new(transaction: &'a deadpool_postgres::Transaction<'a>) -> Self {
        Self { transaction }
    }
}

#[async_trait]
impl ReadExecutor for TransactionalReadExecutor<'_> {
    async fn query(
        &self,
        statement: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> Result<Vec<Row>, tokio_postgres::Error> {
        self.transaction.query(statement, params).await
    }

    async fn query_one(
        &self,
        statement: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> Result<Row, tokio_postgres::Error> {
        self.transaction.query_one(statement, params).await
    }

    async fn query_opt(
        &self,
        statement: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> Result<Option<Row>, tokio_postgres::Error> {
        self.transaction.query_opt(statement, params).await
    }
}
