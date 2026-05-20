use deadpool_postgres::{Object, Pool, Transaction};

mod error;

pub struct Client {
    client: Object,
}

impl Client {
    pub async fn txn(&mut self) -> Result<Transaction<'_>, error::Error> {
        Ok(self.client.transaction().await?)
    }
}

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

    pub async fn get_client(&self) -> Result<Client, error::Error> {
        let client = self.pool.get().await?;

        Ok(Client { client })
    }
}
