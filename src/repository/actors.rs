use deadpool_postgres::GenericClient;
use tokio_postgres::Row;
use uuid::Uuid;

use crate::domain::{
    Actor, ActorId, ActorKind, ActorPermissions, ActorWithPermissions, ApiCredential,
    CreateActorPayload, UpdateActorPayload, UserId, WorkspaceId, WorkspacePermission,
};

use super::{Error, Postgres};

const ACTOR_COLUMNS: &str =
    "id, actor_type, display_name, workspace_id, created_by_user_id, created_at";

impl Postgres {
    /// Resolves the actor's credential by the `key_id` extracted from the
    /// presented key, scoped to the claimed actor. Returns the actor, the
    /// matching credential, and the actor's permission grants so authentication
    /// can verify the key and bind the workspace in one place.
    pub async fn actor_credential_by_key_id(
        &self,
        actor_id: ActorId,
        key_id: &str,
    ) -> Result<Option<(Actor, ApiCredential, ActorPermissions)>, Error> {
        let client = self.get().await?;
        let rows = client
            .query(
                r#"
SELECT
    actors.id AS actor_id,
    actors.actor_type AS actor_type,
    actors.display_name AS actor_display_name,
    actors.workspace_id AS actor_workspace_id,
    actors.created_by_user_id AS actor_created_by_user_id,
    actors.created_at AS actor_created_at,
    api_credentials.id AS credential_id,
    api_credentials.name AS credential_name,
    api_credentials.key_id AS credential_key_id,
    api_credentials.credential_hash AS credential_hash,
    api_credentials.expires_at AS credential_expires_at,
    api_credentials.revoked_at AS credential_revoked_at,
    api_credentials.created_at AS credential_created_at
FROM actors
JOIN api_credentials
    ON api_credentials.actor_id = actors.id
WHERE actors.id = $1 AND api_credentials.key_id = $2
LIMIT 1
"#,
                &[&Uuid::from(actor_id), &key_id],
            )
            .await?;

        let Some(row) = rows.into_iter().next() else {
            return Ok(None);
        };
        let (actor, credential) = actor_with_credential_from_row(row)?;
        let permissions = permissions_for_actor(&client, actor.id).await?;

        Ok(Some((actor, credential, permissions)))
    }

    pub async fn create_actor(&self, actor: &CreateActorPayload) -> Result<Actor, Error> {
        let mut client = self.get().await?;
        let transaction = client.transaction().await?;

        let row = transaction
            .query_one(
                r#"
INSERT INTO actors (id, actor_type, display_name, workspace_id, created_by_user_id)
VALUES (COALESCE($1, gen_random_uuid()), $2, $3, $4, $5)
RETURNING id, actor_type, display_name, workspace_id, created_by_user_id, created_at
"#,
                &[
                    &actor.id.map(Uuid::from),
                    &actor.kind.as_str(),
                    &actor.display_name,
                    &Uuid::from(actor.workspace_id),
                    &actor.created_by_user_id.map(Uuid::from),
                ],
            )
            .await?;
        let created = actor_from_row(row)?;

        for permission in &actor.permissions {
            transaction
                .execute(
                    "INSERT INTO actor_permissions (actor_id, permission) VALUES ($1, $2) ON CONFLICT DO NOTHING",
                    &[&Uuid::from(created.id), &permission.as_str()],
                )
                .await?;
        }

        transaction.commit().await?;

        Ok(created)
    }

    pub async fn get_actor(&self, id: ActorId) -> Result<Option<Actor>, Error> {
        let client = self.get().await?;
        let rows = client
            .query(
                &format!("SELECT {ACTOR_COLUMNS} FROM actors WHERE id = $1"),
                &[&Uuid::from(id)],
            )
            .await?;

        rows.into_iter().next().map(actor_from_row).transpose()
    }

    pub async fn list_actors(&self) -> Result<Vec<Actor>, Error> {
        let client = self.get().await?;
        let rows = client
            .query(
                &format!("SELECT {ACTOR_COLUMNS} FROM actors ORDER BY id"),
                &[],
            )
            .await?;

        rows.into_iter().map(actor_from_row).collect()
    }

    pub async fn list_actors_for_workspace(
        &self,
        workspace_id: WorkspaceId,
    ) -> Result<Vec<ActorWithPermissions>, Error> {
        let client = self.get().await?;
        let rows = client
            .query(
                &format!(
                    "SELECT {ACTOR_COLUMNS} FROM actors WHERE workspace_id = $1 ORDER BY created_at, id"
                ),
                &[&Uuid::from(workspace_id)],
            )
            .await?;

        let mut actors = Vec::with_capacity(rows.len());
        for row in rows {
            let actor = actor_from_row(row)?;
            let permissions = permissions_for_actor(&client, actor.id).await?;
            actors.push(ActorWithPermissions { actor, permissions });
        }

        Ok(actors)
    }

    pub async fn update_actor(
        &self,
        id: ActorId,
        update: &UpdateActorPayload,
    ) -> Result<Option<Actor>, Error> {
        let client = self.get().await?;
        let rows = client
            .query(
                r#"
UPDATE actors
SET
    actor_type = $2,
    display_name = $3,
    workspace_id = $4
WHERE id = $1
RETURNING id, actor_type, display_name, workspace_id, created_by_user_id, created_at
"#,
                &[
                    &Uuid::from(id),
                    &update.kind.as_str(),
                    &update.display_name,
                    &Uuid::from(update.workspace_id),
                ],
            )
            .await?;

        rows.into_iter().next().map(actor_from_row).transpose()
    }

    /// Replaces an actor's permission grants. Used by seeding to keep a re-run
    /// idempotent.
    pub async fn set_actor_permissions(
        &self,
        id: ActorId,
        permissions: &[WorkspacePermission],
    ) -> Result<(), Error> {
        let mut client = self.get().await?;
        let transaction = client.transaction().await?;

        transaction
            .execute(
                "DELETE FROM actor_permissions WHERE actor_id = $1",
                &[&Uuid::from(id)],
            )
            .await?;
        for permission in permissions {
            transaction
                .execute(
                    "INSERT INTO actor_permissions (actor_id, permission) VALUES ($1, $2)",
                    &[&Uuid::from(id), &permission.as_str()],
                )
                .await?;
        }

        transaction.commit().await?;

        Ok(())
    }

    pub async fn delete_actor(&self, id: ActorId) -> Result<bool, Error> {
        let client = self.get().await?;
        let deleted = client
            .execute("DELETE FROM actors WHERE id = $1", &[&Uuid::from(id)])
            .await?;

        Ok(deleted > 0)
    }
}

async fn permissions_for_actor(
    client: &impl GenericClient,
    actor_id: ActorId,
) -> Result<ActorPermissions, Error> {
    let rows = client
        .query(
            "SELECT permission FROM actor_permissions WHERE actor_id = $1",
            &[&Uuid::from(actor_id)],
        )
        .await?;

    let mut permissions = ActorPermissions::none();
    for row in rows {
        let value: String = row.try_get("permission")?;
        let permission = value
            .parse::<WorkspacePermission>()
            .map_err(|_| Error::InvariantViolation("unknown actor permission"))?;
        permissions.insert(permission);
    }

    Ok(permissions)
}

fn actor_with_credential_from_row(row: Row) -> Result<(Actor, ApiCredential), Error> {
    let actor = Actor {
        id: ActorId::from(row.try_get::<_, Uuid>("actor_id")?),
        kind: row
            .try_get::<_, String>("actor_type")?
            .parse::<ActorKind>()?,
        display_name: row.try_get("actor_display_name")?,
        workspace_id: WorkspaceId::from(row.try_get::<_, Uuid>("actor_workspace_id")?),
        created_by_user_id: row
            .try_get::<_, Option<Uuid>>("actor_created_by_user_id")?
            .map(UserId::from),
        created_at: row.try_get("actor_created_at")?,
    };
    let credential = ApiCredential {
        id: row.try_get("credential_id")?,
        actor_id: actor.id,
        name: row.try_get("credential_name")?,
        key_id: row.try_get("credential_key_id")?,
        credential_hash: row.try_get("credential_hash")?,
        expires_at: row.try_get("credential_expires_at")?,
        revoked_at: row.try_get("credential_revoked_at")?,
        created_at: row.try_get("credential_created_at")?,
    };

    Ok((actor, credential))
}

fn actor_from_row(row: Row) -> Result<Actor, Error> {
    Ok(Actor {
        id: ActorId::from(row.try_get::<_, Uuid>("id")?),
        kind: row
            .try_get::<_, String>("actor_type")?
            .parse::<ActorKind>()?,
        display_name: row.try_get("display_name")?,
        workspace_id: WorkspaceId::from(row.try_get::<_, Uuid>("workspace_id")?),
        created_by_user_id: row
            .try_get::<_, Option<Uuid>>("created_by_user_id")?
            .map(UserId::from),
        created_at: row.try_get("created_at")?,
    })
}
