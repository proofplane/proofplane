use tokio_postgres::Row;
use uuid::Uuid;

use crate::domain::{ProvisionUserPayload, User, UserId};

use super::{
    snapshot::{save_snapshot, snapshot_record},
    Error, Postgres, UnitOfWork,
};

enum RepositoryConnection<'a> {
    Postgres(&'a Postgres),
    Transaction(&'a UnitOfWork<'a>),
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
        UserRecord::try_from_row(&row)?.into_domain()
    }
}

impl<'a> UnitOfWork<'a> {
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
            RepositoryConnection::Transaction(unit_of_work) => {
                unit_of_work
                    .transaction
                    .query(GET_BY_ID_FOR_UPDATE_SQL, &[&Uuid::from(id)])
                    .await?
            }
        };
        rows.into_iter()
            .next()
            .map(|row| UserRecord::try_from_row(&row)?.into_domain())
            .transpose()
    }

    pub async fn save(&self, user: &User) -> Result<(), Error> {
        let RepositoryConnection::Transaction(unit_of_work) = self.connection else {
            return Err(Error::InvariantViolation(
                "users must be saved in a transaction",
            ));
        };
        let record = UserRecord::from_domain(user)?;
        save_snapshot(&unit_of_work.transaction, record.as_snapshot()).await
    }
}

const GET_BY_ID_SQL: &str =
    "SELECT id, auth0_sub, email, name, last_login_at, created_at FROM users WHERE id = $1";
const GET_BY_ID_FOR_UPDATE_SQL: &str =
    "SELECT id, auth0_sub, email, name, last_login_at, created_at FROM users WHERE id = $1 FOR UPDATE";

snapshot_record! {
    struct UserRecord {
        id: Uuid,
        auth0_sub: String,
        email: Option<String>,
        name: Option<String>,
        last_login_at: Option<chrono::DateTime<chrono::Utc>>,
        created_at: chrono::DateTime<chrono::Utc>,
    }
    table: users,
    conflict: id,
}

impl UserRecord {
    fn try_from_row(row: &Row) -> Result<Self, Error> {
        Ok(Self {
            id: row.try_get("id")?,
            auth0_sub: row.try_get("auth0_sub")?,
            email: row.try_get("email")?,
            name: row.try_get("name")?,
            last_login_at: row.try_get("last_login_at")?,
            created_at: row.try_get("created_at")?,
        })
    }

    fn from_domain(user: &User) -> Result<Self, Error> {
        Ok(Self {
            id: user.id.into(),
            auth0_sub: user.auth0_sub.clone(),
            email: user.email.clone(),
            name: user.name.clone(),
            last_login_at: user.last_login_at,
            created_at: user.created_at,
        })
    }

    fn into_domain(self) -> Result<User, Error> {
        User::rehydrate(
            UserId::from(self.id),
            self.auth0_sub,
            self.email,
            self.name,
            self.last_login_at,
            self.created_at,
        )
        .map_err(|_| Error::InvariantViolation("persisted user lifecycle is inconsistent"))
    }
}
