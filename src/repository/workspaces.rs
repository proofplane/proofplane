use tokio_postgres::Row;
use uuid::Uuid;

use crate::domain::{CreateWorkspacePayload, UpdateWorkspacePayload, Workspace, WorkspaceId};

use super::{constraints::classify_db_error, Error, Postgres, TransactionContext};

impl TransactionContext<'_> {
    pub async fn create_workspace(
        &self,
        workspace: &CreateWorkspacePayload,
    ) -> Result<Workspace, Error> {
        let id = workspace.id.map(Uuid::from);
        let row = self
            .transaction
            .query_one(
                r#"
INSERT INTO workspaces (id, slug, name)
VALUES (COALESCE($1, gen_random_uuid()), $2, $3)
RETURNING
    id,
    slug,
    name,
    created_at
"#,
                &[&id, &workspace.slug, &workspace.name],
            )
            .await
            .map_err(classify_db_error)?;

        workspace_from_row(row)
    }
}

// TODO: add cursor-based pagination to repository list behavior.
impl Postgres {
    pub async fn create_workspace(
        &self,
        workspace: &CreateWorkspacePayload,
    ) -> Result<Workspace, Error> {
        let client = self.get().await?;
        let id = workspace.id.map(Uuid::from);
        let row = client
            .query_one(
                r#"
INSERT INTO workspaces (id, slug, name)
VALUES (COALESCE($1, gen_random_uuid()), $2, $3)
RETURNING
    id,
    slug,
    name,
    created_at
"#,
                &[&id, &workspace.slug, &workspace.name],
            )
            .await
            .map_err(classify_db_error)?;

        workspace_from_row(row)
    }

    pub async fn get_workspace(&self, id: WorkspaceId) -> Result<Option<Workspace>, Error> {
        let client = self.get().await?;
        let rows = client
            .query(
                r#"
SELECT
    id,
    slug,
    name,
    created_at
FROM workspaces
WHERE id = $1
"#,
                &[&Uuid::from(id)],
            )
            .await?;

        rows.into_iter().next().map(workspace_from_row).transpose()
    }

    pub async fn list_workspaces(&self) -> Result<Vec<Workspace>, Error> {
        let client = self.get().await?;
        let rows = client
            .query(
                r#"
SELECT
    id,
    slug,
    name,
    created_at
FROM workspaces
ORDER BY created_at, id
"#,
                &[],
            )
            .await?;

        rows.into_iter().map(workspace_from_row).collect()
    }

    pub async fn update_workspace(
        &self,
        id: WorkspaceId,
        update: &UpdateWorkspacePayload,
    ) -> Result<Option<Workspace>, Error> {
        let client = self.get().await?;
        let rows = client
            .query(
                r#"
UPDATE workspaces
SET
    slug = $2,
    name = $3
WHERE id = $1
RETURNING
    id,
    slug,
    name,
    created_at
"#,
                &[&Uuid::from(id), &update.slug, &update.name],
            )
            .await
            .map_err(classify_db_error)?;

        rows.into_iter().next().map(workspace_from_row).transpose()
    }

    pub async fn delete_workspace(&self, id: WorkspaceId) -> Result<bool, Error> {
        let client = self.get().await?;
        let deleted = client
            .execute("DELETE FROM workspaces WHERE id = $1", &[&Uuid::from(id)])
            .await?;

        Ok(deleted > 0)
    }
}

fn workspace_from_row(row: Row) -> Result<Workspace, Error> {
    Ok(Workspace {
        id: WorkspaceId::from(row.try_get::<_, Uuid>("id")?),
        slug: row.try_get("slug")?,
        name: row.try_get("name")?,
        created_at: row.try_get("created_at")?,
    })
}
