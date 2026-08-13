use async_trait::async_trait;
use deadpool_postgres::Object;
use tokio_postgres::{
    types::{ToSql, Type},
    Row,
};

/// Parameters paired with their Postgres types.
///
/// Built with [`crate::persistence::param`]; see `persistence::params` for why
/// the types travel with the values.
pub(crate) type Params<'params> = [(&'params (dyn ToSql + Sync + 'params), Type)];

#[async_trait]
pub(crate) trait ReadExecutor: Send + Sync {
    async fn query(
        &self,
        statement: &str,
        params: &Params<'_>,
    ) -> Result<Vec<Row>, tokio_postgres::Error>;

    async fn query_one(
        &self,
        statement: &str,
        params: &Params<'_>,
    ) -> Result<Row, tokio_postgres::Error>;

    async fn query_opt(
        &self,
        statement: &str,
        params: &Params<'_>,
    ) -> Result<Option<Row>, tokio_postgres::Error>;
}

#[async_trait]
impl<T: ReadExecutor + ?Sized> ReadExecutor for &T {
    async fn query(
        &self,
        statement: &str,
        params: &Params<'_>,
    ) -> Result<Vec<Row>, tokio_postgres::Error> {
        (*self).query(statement, params).await
    }

    async fn query_one(
        &self,
        statement: &str,
        params: &Params<'_>,
    ) -> Result<Row, tokio_postgres::Error> {
        (*self).query_one(statement, params).await
    }

    async fn query_opt(
        &self,
        statement: &str,
        params: &Params<'_>,
    ) -> Result<Option<Row>, tokio_postgres::Error> {
        (*self).query_opt(statement, params).await
    }
}

/// Reads outside a [`crate::persistence::UnitOfWork`].
///
/// Each statement is its own implicit transaction. That is safe through a
/// transaction pooler because `query_typed` parses into the unnamed statement
/// and sends the whole exchange under one `Sync`, so nothing depends on the
/// pooler keeping the same backend across two round trips.
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
        params: &Params<'_>,
    ) -> Result<Vec<Row>, tokio_postgres::Error> {
        self.client.query_typed(statement, params).await
    }

    async fn query_one(
        &self,
        statement: &str,
        params: &Params<'_>,
    ) -> Result<Row, tokio_postgres::Error> {
        self.client.query_typed_one(statement, params).await
    }

    async fn query_opt(
        &self,
        statement: &str,
        params: &Params<'_>,
    ) -> Result<Option<Row>, tokio_postgres::Error> {
        self.client.query_typed_opt(statement, params).await
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
        params: &Params<'_>,
    ) -> Result<Vec<Row>, tokio_postgres::Error> {
        self.transaction.query_typed(statement, params).await
    }

    async fn query_one(
        &self,
        statement: &str,
        params: &Params<'_>,
    ) -> Result<Row, tokio_postgres::Error> {
        self.transaction.query_typed_one(statement, params).await
    }

    async fn query_opt(
        &self,
        statement: &str,
        params: &Params<'_>,
    ) -> Result<Option<Row>, tokio_postgres::Error> {
        self.transaction.query_typed_opt(statement, params).await
    }
}
