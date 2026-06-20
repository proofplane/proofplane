use deadpool_postgres::GenericClient;
use tokio_postgres::Row;
use uuid::Uuid;

use crate::{
    authentication::opaque_token::ApiTokenDigest,
    domain::{
        ApiToken, ApiTokenId, ApiTokenWithPermissions, CreateApiTokenPayload, UserId, WorkspaceId,
        WorkspacePermission,
    },
};

use super::{Error, Postgres};

const API_TOKEN_COLUMNS: &str =
    "id, user_id, workspace_id, name, expires_at, revoked_at, last_used_at, created_at";

impl Postgres {
    pub async fn create_api_token(
        &self,
        token: &CreateApiTokenPayload,
    ) -> Result<ApiTokenWithPermissions, Error> {
        let mut client = self.get().await?;
        let transaction = client.transaction().await?;
        let digest: &[u8] = token.digest.as_bytes();

        let row = transaction
            .query_one(
                r#"
INSERT INTO api_tokens (id, digest, user_id, workspace_id, name, expires_at)
VALUES ($1, $2, $3, $4, $5, $6)
RETURNING id, user_id, workspace_id, name, expires_at, revoked_at, last_used_at, created_at
"#,
                &[
                    &Uuid::from(token.id),
                    &digest,
                    &Uuid::from(token.user_id),
                    &Uuid::from(token.workspace_id),
                    &token.name,
                    &token.expires_at,
                ],
            )
            .await?;
        let created = api_token_from_row(&row)?;

        for permission in &token.permissions {
            transaction
                .execute(
                    "INSERT INTO api_token_permissions (api_token_id, permission) VALUES ($1, $2)",
                    &[&Uuid::from(created.id), &permission.as_str()],
                )
                .await?;
        }

        transaction.commit().await?;

        Ok(ApiTokenWithPermissions {
            token: created,
            permissions: token.permissions.clone(),
        })
    }

    pub async fn list_api_tokens_for_owner_workspace(
        &self,
        user_id: UserId,
        workspace_id: WorkspaceId,
    ) -> Result<Vec<ApiTokenWithPermissions>, Error> {
        let client = self.get().await?;
        let rows = client
            .query(
                &format!(
                    r#"
SELECT {API_TOKEN_COLUMNS}
FROM api_tokens
WHERE user_id = $1 AND workspace_id = $2
ORDER BY created_at DESC, id DESC
"#
                ),
                &[&Uuid::from(user_id), &Uuid::from(workspace_id)],
            )
            .await?;

        let mut tokens = Vec::with_capacity(rows.len());
        for row in rows {
            let token = api_token_from_row(&row)?;
            let permissions = permissions_for_token(&client, token.id).await?;
            tokens.push(ApiTokenWithPermissions { token, permissions });
        }

        Ok(tokens)
    }

    pub async fn get_api_token(
        &self,
        id: ApiTokenId,
    ) -> Result<Option<ApiTokenWithPermissions>, Error> {
        let client = self.get().await?;
        let rows = client
            .query(
                &format!("SELECT {API_TOKEN_COLUMNS} FROM api_tokens WHERE id = $1"),
                &[&Uuid::from(id)],
            )
            .await?;
        let Some(row) = rows.into_iter().next() else {
            return Ok(None);
        };
        let token = api_token_from_row(&row)?;
        let permissions = permissions_for_token(&client, token.id).await?;

        Ok(Some(ApiTokenWithPermissions { token, permissions }))
    }

    pub async fn get_api_token_by_digest(
        &self,
        digest: ApiTokenDigest,
    ) -> Result<Option<ApiTokenWithPermissions>, Error> {
        let client = self.get().await?;
        let digest: &[u8] = digest.as_bytes();
        let rows = client
            .query(
                &format!(
                    r#"
SELECT {API_TOKEN_COLUMNS}, api_token_permissions.permission
FROM api_tokens
LEFT JOIN api_token_permissions ON api_token_permissions.api_token_id = api_tokens.id
WHERE digest = $1
"#
                ),
                &[&digest],
            )
            .await?;
        let mut rows = rows.into_iter();
        let Some(row) = rows.next() else {
            return Ok(None);
        };
        let token = api_token_from_row(&row)?;
        let mut permissions = Vec::with_capacity(rows.len() + 1);
        permission_from_joined_row(&row, &mut permissions)?;
        for row in rows {
            permission_from_joined_row(&row, &mut permissions)?;
        }

        Ok(Some(ApiTokenWithPermissions { token, permissions }))
    }

    pub async fn revoke_api_token_for_owner_workspace(
        &self,
        id: ApiTokenId,
        user_id: UserId,
        workspace_id: WorkspaceId,
    ) -> Result<bool, Error> {
        let client = self.get().await?;
        let updated = client
            .execute(
                r#"
UPDATE api_tokens
SET revoked_at = COALESCE(revoked_at, now())
WHERE id = $1 AND user_id = $2 AND workspace_id = $3
"#,
                &[
                    &Uuid::from(id),
                    &Uuid::from(user_id),
                    &Uuid::from(workspace_id),
                ],
            )
            .await?;

        Ok(updated > 0)
    }

    pub async fn touch_api_token_last_used_at(&self, id: ApiTokenId) -> Result<bool, Error> {
        let client = self.get().await?;
        let updated = client
            .execute(
                "UPDATE api_tokens SET last_used_at = now() WHERE id = $1",
                &[&Uuid::from(id)],
            )
            .await?;

        Ok(updated > 0)
    }
}

async fn permissions_for_token(
    client: &impl GenericClient,
    token_id: ApiTokenId,
) -> Result<Vec<WorkspacePermission>, Error> {
    let rows = client
        .query(
            "SELECT permission FROM api_token_permissions WHERE api_token_id = $1",
            &[&Uuid::from(token_id)],
        )
        .await?;

    let mut values = Vec::with_capacity(rows.len());
    for row in rows {
        let value: String = row.try_get("permission")?;
        values.push(
            value
                .parse::<WorkspacePermission>()
                .map_err(|_| Error::InvariantViolation("unknown API token permission"))?,
        );
    }

    // Keep the API stable.
    values.sort_by_key(|permission| {
        WorkspacePermission::ALL
            .iter()
            .position(|canonical| canonical == permission)
            .expect("parsed permission is listed in WorkspacePermission::ALL")
    });

    Ok(values)
}

fn api_token_from_row(row: &Row) -> Result<ApiToken, Error> {
    Ok(ApiToken {
        id: ApiTokenId::from(row.try_get::<_, Uuid>("id")?),
        user_id: UserId::from(row.try_get::<_, Uuid>("user_id")?),
        workspace_id: WorkspaceId::from(row.try_get::<_, Uuid>("workspace_id")?),
        name: row.try_get("name")?,
        expires_at: row.try_get("expires_at")?,
        revoked_at: row.try_get("revoked_at")?,
        last_used_at: row.try_get("last_used_at")?,
        created_at: row.try_get("created_at")?,
    })
}

fn permission_from_joined_row(
    row: &Row,
    permissions: &mut Vec<WorkspacePermission>,
) -> Result<(), Error> {
    let Some(value) = row.try_get::<_, Option<String>>("permission")? else {
        return Ok(());
    };

    permissions.push(
        value
            .parse::<WorkspacePermission>()
            .map_err(|_| Error::InvariantViolation("unknown API token permission"))?,
    );

    Ok(())
}
