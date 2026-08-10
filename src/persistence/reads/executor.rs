use async_trait::async_trait;
use deadpool_postgres::Object;
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

pub(crate) struct PooledReadExecutor {
    client: Object,
}

impl PooledReadExecutor {
    pub(crate) fn new(client: Object) -> Self {
        Self { client }
    }
}

#[async_trait]
impl ReadExecutor for PooledReadExecutor {
    async fn query(
        &self,
        statement: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> Result<Vec<Row>, tokio_postgres::Error> {
        self.client.query(statement, params).await
    }

    async fn query_one(
        &self,
        statement: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> Result<Row, tokio_postgres::Error> {
        self.client.query_one(statement, params).await
    }

    async fn query_opt(
        &self,
        statement: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> Result<Option<Row>, tokio_postgres::Error> {
        self.client.query_opt(statement, params).await
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
