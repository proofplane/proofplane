use uuid::Uuid;

use crate::{domain::UserId, persistence::Error};

use super::{ReadExecutor, TransactionalReadExecutor};

pub(crate) struct UserReads<'a, E> {
    executor: &'a E,
}

impl<'a, E> UserReads<'a, E> {
    pub(crate) fn new(executor: &'a E) -> Self {
        Self { executor }
    }
}

impl UserReads<'_, TransactionalReadExecutor<'_>> {
    pub async fn resolve_id_by_auth0_sub(&self, auth0_sub: &str) -> Result<Option<UserId>, Error> {
        self.executor
            .query_one(
                "SELECT pg_advisory_xact_lock(hashtextextended($1, 0))",
                &[&auth0_sub],
            )
            .await?;
        self.executor
            .query_opt("SELECT id FROM users WHERE auth0_sub = $1", &[&auth0_sub])
            .await?
            .map(|row| row.try_get::<_, Uuid>("id").map(UserId::from))
            .transpose()
            .map_err(Into::into)
    }

    pub async fn auth0_sub(&self, id: UserId) -> Result<Option<String>, Error> {
        self.executor
            .query_opt(
                "SELECT auth0_sub FROM users WHERE id = $1",
                &[&Uuid::from(id)],
            )
            .await?
            .map(|row| row.try_get("auth0_sub"))
            .transpose()
            .map_err(Into::into)
    }
}
