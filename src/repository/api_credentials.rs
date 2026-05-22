use tokio_postgres::Row;

use crate::domain::{ApiCredential, CreateApiCredentialPayload, UpdateApiCredentialPayload};

use super::{Error, Postgres};

impl Postgres {
    pub async fn create_api_credential(
        &self,
        credential: &CreateApiCredentialPayload,
    ) -> Result<ApiCredential, Error> {
        let client = self.get().await?;
        let row = client
            .query_one(
                r#"
INSERT INTO api_credentials (
    id,
    actor_id,
    name,
    key_id,
    credential_hash,
    expires_at,
    revoked_at
)
VALUES ($1, $2, $3, $4, $5, $6, $7)
RETURNING
    id,
    actor_id,
    name,
    key_id,
    credential_hash,
    expires_at,
    revoked_at,
    created_at
"#,
                &[
                    &credential.id,
                    &credential.actor_id,
                    &credential.name,
                    &credential.key_id,
                    &credential.credential_hash,
                    &credential.expires_at,
                    &credential.revoked_at,
                ],
            )
            .await?;

        api_credential_from_row(row)
    }

    pub async fn get_api_credential(&self, id: &str) -> Result<Option<ApiCredential>, Error> {
        let client = self.get().await?;
        let rows = client
            .query(
                r#"
SELECT
    id,
    actor_id,
    name,
    key_id,
    credential_hash,
    expires_at,
    revoked_at,
    created_at
FROM api_credentials
WHERE id = $1
"#,
                &[&id],
            )
            .await?;

        rows.into_iter()
            .next()
            .map(api_credential_from_row)
            .transpose()
    }

    pub async fn list_api_credentials(&self) -> Result<Vec<ApiCredential>, Error> {
        let client = self.get().await?;
        let rows = client
            .query(
                r#"
SELECT
    id,
    actor_id,
    name,
    key_id,
    credential_hash,
    expires_at,
    revoked_at,
    created_at
FROM api_credentials
ORDER BY id
"#,
                &[],
            )
            .await?;

        rows.into_iter().map(api_credential_from_row).collect()
    }

    pub async fn update_api_credential(
        &self,
        id: &str,
        update: &UpdateApiCredentialPayload,
    ) -> Result<Option<ApiCredential>, Error> {
        let client = self.get().await?;
        let rows = client
            .query(
                r#"
UPDATE api_credentials
SET
    actor_id = $2,
    name = $3,
    key_id = $4,
    credential_hash = $5,
    expires_at = $6,
    revoked_at = $7
WHERE id = $1
RETURNING
    id,
    actor_id,
    name,
    key_id,
    credential_hash,
    expires_at,
    revoked_at,
    created_at
"#,
                &[
                    &id,
                    &update.actor_id,
                    &update.name,
                    &update.key_id,
                    &update.credential_hash,
                    &update.expires_at,
                    &update.revoked_at,
                ],
            )
            .await?;

        rows.into_iter()
            .next()
            .map(api_credential_from_row)
            .transpose()
    }

    pub async fn delete_api_credential(&self, id: &str) -> Result<bool, Error> {
        let client = self.get().await?;
        let deleted = client
            .execute("DELETE FROM api_credentials WHERE id = $1", &[&id])
            .await?;

        Ok(deleted > 0)
    }
}

fn api_credential_from_row(row: Row) -> Result<ApiCredential, Error> {
    Ok(ApiCredential {
        id: row.try_get("id")?,
        actor_id: row.try_get("actor_id")?,
        name: row.try_get("name")?,
        key_id: row.try_get("key_id")?,
        credential_hash: row.try_get("credential_hash")?,
        expires_at: row.try_get("expires_at")?,
        revoked_at: row.try_get("revoked_at")?,
        created_at: row.try_get("created_at")?,
    })
}
