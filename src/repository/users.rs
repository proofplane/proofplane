use tokio_postgres::Row;
use uuid::Uuid;

use crate::domain::{ProvisionUserPayload, User, UserId};

use super::{constraints::classify_db_error, Error, Postgres, TransactionContext};

enum RepositoryConnection<'a> {
    Postgres(&'a Postgres),
    Transaction(&'a TransactionContext<'a>),
}

/// Complete-snapshot persistence for the user aggregate.
pub struct UserRepository<'a> {
    connection: RepositoryConnection<'a>,
}

impl Postgres {
    pub fn users(&self) -> UserRepository<'_> {
        UserRepository {
            connection: RepositoryConnection::Postgres(self),
        }
    }

    // Temporary compatibility reads for callers migrated by later tickets.
    pub async fn get_user(&self, id: UserId) -> Result<Option<User>, Error> {
        self.users().get(id).await
    }

    pub async fn get_user_by_auth0_sub(&self, auth0_sub: &str) -> Result<Option<User>, Error> {
        self.users().get_by_auth0_sub(auth0_sub).await
    }

    // One-shot seed compatibility. Runtime authentication is cut over to the
    // concrete provisioning handler in this ticket.
    pub async fn upsert_user_by_auth0_sub(
        &self,
        payload: &ProvisionUserPayload,
    ) -> Result<User, Error> {
        let row = self
            .get()
            .await?
            .query_one(
                r#"
INSERT INTO users (auth0_sub, email, name)
VALUES ($1, $2, $3)
ON CONFLICT (auth0_sub) DO UPDATE
SET
    email = COALESCE(EXCLUDED.email, users.email),
    name = COALESCE(EXCLUDED.name, users.name)
RETURNING id, auth0_sub, email, name, last_login_at, created_at
"#,
                &[&payload.auth0_sub, &payload.email, &payload.name],
            )
            .await?;
        user_from_row(row)
    }
}

impl<'a> TransactionContext<'a> {
    pub fn users(&'a self) -> UserRepository<'a> {
        UserRepository {
            connection: RepositoryConnection::Transaction(self),
        }
    }
}

impl UserRepository<'_> {
    pub async fn get(&self, id: UserId) -> Result<Option<User>, Error> {
        let rows = match self.connection {
            RepositoryConnection::Postgres(postgres) => {
                postgres
                    .get()
                    .await?
                    .query(GET_BY_ID_SQL, &[&Uuid::from(id)])
                    .await?
            }
            RepositoryConnection::Transaction(context) => {
                context
                    .transaction
                    .query(GET_BY_ID_FOR_UPDATE_SQL, &[&Uuid::from(id)])
                    .await?
            }
        };
        rows.into_iter().next().map(user_from_row).transpose()
    }

    pub async fn get_by_auth0_sub(&self, auth0_sub: &str) -> Result<Option<User>, Error> {
        let rows = match self.connection {
            RepositoryConnection::Postgres(postgres) => {
                postgres
                    .get()
                    .await?
                    .query(GET_BY_SUB_SQL, &[&auth0_sub])
                    .await?
            }
            RepositoryConnection::Transaction(context) => {
                // A subject has no row to lock before first provisioning. This
                // transaction-scoped key lock serializes that absent-to-present
                // transition without broad table locks.
                context
                    .transaction
                    .query_one(
                        "SELECT pg_advisory_xact_lock(hashtextextended($1, 0))",
                        &[&auth0_sub],
                    )
                    .await?;
                context
                    .transaction
                    .query(GET_BY_SUB_FOR_UPDATE_SQL, &[&auth0_sub])
                    .await?
            }
        };
        rows.into_iter().next().map(user_from_row).transpose()
    }

    pub async fn save(&self, user: &User) -> Result<(), Error> {
        let RepositoryConnection::Transaction(context) = self.connection else {
            return Err(Error::InvariantViolation(
                "users must be saved in a transaction",
            ));
        };
        let affected = context
            .transaction
            .execute(
                r#"
INSERT INTO users (id, auth0_sub, email, name, last_login_at, created_at)
VALUES ($1, $2, $3, $4, $5, $6)
ON CONFLICT (id) DO UPDATE
SET
    auth0_sub = EXCLUDED.auth0_sub,
    email = EXCLUDED.email,
    name = EXCLUDED.name,
    last_login_at = EXCLUDED.last_login_at,
    created_at = EXCLUDED.created_at
"#,
                &[
                    &Uuid::from(user.id),
                    &user.auth0_sub,
                    &user.email,
                    &user.name,
                    &user.last_login_at,
                    &user.created_at,
                ],
            )
            .await
            .map_err(classify_db_error)?;
        if affected != 1 {
            return Err(Error::InvariantViolation(
                "user snapshot save affected an unexpected row count",
            ));
        }
        Ok(())
    }
}

const GET_BY_ID_SQL: &str =
    "SELECT id, auth0_sub, email, name, last_login_at, created_at FROM users WHERE id = $1";
const GET_BY_ID_FOR_UPDATE_SQL: &str =
    "SELECT id, auth0_sub, email, name, last_login_at, created_at FROM users WHERE id = $1 FOR UPDATE";
const GET_BY_SUB_SQL: &str =
    "SELECT id, auth0_sub, email, name, last_login_at, created_at FROM users WHERE auth0_sub = $1";
const GET_BY_SUB_FOR_UPDATE_SQL: &str =
    "SELECT id, auth0_sub, email, name, last_login_at, created_at FROM users WHERE auth0_sub = $1 FOR UPDATE";

fn user_from_row(row: Row) -> Result<User, Error> {
    User::rehydrate(
        UserId::from(row.try_get::<_, Uuid>("id")?),
        row.try_get("auth0_sub")?,
        row.try_get("email")?,
        row.try_get("name")?,
        row.try_get("last_login_at")?,
        row.try_get("created_at")?,
    )
    .map_err(|_| Error::InvariantViolation("persisted user lifecycle is inconsistent"))
}
