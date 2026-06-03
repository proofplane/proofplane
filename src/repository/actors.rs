use tokio_postgres::Row;
use uuid::Uuid;

use crate::domain::{
    Actor, ActorId, ActorKind, ActorWithApiCredential, ApiCredential, CreateActorPayload,
    UpdateActorPayload,
};

use super::{Error, Postgres};

impl Postgres {
    pub async fn actor_with_api_credential(
        &self,
        actor_id: ActorId,
    ) -> Result<Option<ActorWithApiCredential>, Error> {
        let client = self.get().await?;
        let rows = client
            .query(
                r#"
SELECT
    actors.id AS actor_id,
    actors.actor_type AS actor_type,
    actors.display_name AS actor_display_name,
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
WHERE actors.id = $1
LIMIT 1
"#,
                &[&Uuid::from(actor_id)],
            )
            .await?;

        rows.into_iter()
            .next()
            .map(actor_with_credential_from_row)
            .transpose()
    }

    pub async fn create_actor(&self, actor: &CreateActorPayload) -> Result<Actor, Error> {
        let client = self.get().await?;
        let row = client
            .query_one(
                r#"
INSERT INTO actors (id, actor_type, display_name)
VALUES (COALESCE($1, gen_random_uuid()), $2, $3)
RETURNING
    id,
    actor_type,
    display_name,
    created_at
"#,
                &[
                    &actor.id.map(Uuid::from),
                    &actor.kind.as_str(),
                    &actor.display_name,
                ],
            )
            .await?;

        actor_from_row(row)
    }

    pub async fn get_actor(&self, id: ActorId) -> Result<Option<Actor>, Error> {
        let client = self.get().await?;
        let rows = client
            .query(
                r#"
SELECT
    id,
    actor_type,
    display_name,
    created_at
FROM actors
WHERE id = $1
"#,
                &[&Uuid::from(id)],
            )
            .await?;

        rows.into_iter().next().map(actor_from_row).transpose()
    }

    pub async fn list_actors(&self) -> Result<Vec<Actor>, Error> {
        let client = self.get().await?;
        let rows = client
            .query(
                r#"
SELECT
    id,
    actor_type,
    display_name,
    created_at
FROM actors
ORDER BY id
"#,
                &[],
            )
            .await?;

        rows.into_iter().map(actor_from_row).collect()
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
    display_name = $3
WHERE id = $1
RETURNING
    id,
    actor_type,
    display_name,
    created_at
"#,
                &[&Uuid::from(id), &update.kind.as_str(), &update.display_name],
            )
            .await?;

        rows.into_iter().next().map(actor_from_row).transpose()
    }

    pub async fn delete_actor(&self, id: ActorId) -> Result<bool, Error> {
        let client = self.get().await?;
        let deleted = client
            .execute("DELETE FROM actors WHERE id = $1", &[&Uuid::from(id)])
            .await?;

        Ok(deleted > 0)
    }
}

fn actor_with_credential_from_row(row: Row) -> Result<ActorWithApiCredential, Error> {
    let actor = Actor {
        id: ActorId::from(row.try_get::<_, uuid::Uuid>("actor_id")?),
        kind: row
            .try_get::<_, String>("actor_type")?
            .parse::<ActorKind>()?,
        display_name: row.try_get("actor_display_name")?,
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

    Ok(ActorWithApiCredential {
        actor,
        api_credential: credential,
    })
}

fn actor_from_row(row: Row) -> Result<Actor, Error> {
    Ok(Actor {
        id: ActorId::from(row.try_get::<_, uuid::Uuid>("id")?),
        kind: row
            .try_get::<_, String>("actor_type")?
            .parse::<ActorKind>()?,
        display_name: row.try_get("display_name")?,
        created_at: row.try_get("created_at")?,
    })
}
