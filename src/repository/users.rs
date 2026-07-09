use tokio_postgres::Row;
use uuid::Uuid;

use crate::domain::{ProvisionUserPayload, User, UserId};

use super::{Error, Postgres};

impl Postgres {
    pub async fn upsert_user_by_auth0_sub(
        &self,
        payload: &ProvisionUserPayload,
    ) -> Result<User, Error> {
        let client = self.get().await?;
        let row = client
            .query_one(
                r#"
INSERT INTO users (auth0_sub, email, name)
VALUES ($1, $2, $3)
ON CONFLICT (auth0_sub) DO UPDATE
SET
    email = COALESCE(EXCLUDED.email, users.email),
    name = COALESCE(EXCLUDED.name, users.name)
RETURNING
    id,
    auth0_sub,
    email,
    name,
    last_login_at,
    created_at
"#,
                &[&payload.auth0_sub, &payload.email, &payload.name],
            )
            .await?;

        user_from_row(row)
    }

    pub async fn get_user(&self, id: UserId) -> Result<Option<User>, Error> {
        let client = self.get().await?;
        let rows = client
            .query(
                r#"
SELECT
    id,
    auth0_sub,
    email,
    name,
    last_login_at,
    created_at
FROM users
WHERE id = $1
"#,
                &[&Uuid::from(id)],
            )
            .await?;

        rows.into_iter().next().map(user_from_row).transpose()
    }

    pub async fn get_user_by_auth0_sub(&self, auth0_sub: &str) -> Result<Option<User>, Error> {
        let client = self.get().await?;
        let row = client
            .query_opt(
                r#"
SELECT
    id,
    auth0_sub,
    email,
    name,
    last_login_at,
    created_at
FROM users
WHERE auth0_sub = $1
"#,
                &[&auth0_sub],
            )
            .await?;

        row.map(user_from_row).transpose()
    }

    pub async fn record_user_login(&self, id: UserId) -> Result<Option<User>, Error> {
        let client = self.get().await?;
        let rows = client
            .query(
                r#"
UPDATE users
SET last_login_at = now()
WHERE id = $1
RETURNING
    id,
    auth0_sub,
    email,
    name,
    last_login_at,
    created_at
"#,
                &[&Uuid::from(id)],
            )
            .await?;

        rows.into_iter().next().map(user_from_row).transpose()
    }
}

fn user_from_row(row: Row) -> Result<User, Error> {
    Ok(User {
        id: UserId::from(row.try_get::<_, Uuid>("id")?),
        auth0_sub: row.try_get("auth0_sub")?,
        email: row.try_get("email")?,
        name: row.try_get("name")?,
        last_login_at: row.try_get("last_login_at")?,
        created_at: row.try_get("created_at")?,
    })
}
