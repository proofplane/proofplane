use tokio_postgres::Row;
use uuid::Uuid;

use crate::domain::{
    CreateWorkspacePayload, UpdateWorkspacePayload, UserId, Workspace, WorkspaceAggregate,
    WorkspaceId, WorkspaceMembership, WorkspaceRole,
};

use super::{constraints::classify_db_error, Error, Postgres, TransactionContext};

/// Complete-snapshot persistence for a workspace and all of its memberships.
pub struct WorkspaceRepository<'a> {
    context: &'a TransactionContext<'a>,
}

impl<'a> TransactionContext<'a> {
    pub fn workspace_aggregates(&'a self) -> WorkspaceRepository<'a> {
        WorkspaceRepository { context: self }
    }
}

impl WorkspaceRepository<'_> {
    pub async fn get(&self, id: WorkspaceId) -> Result<Option<WorkspaceAggregate>, Error> {
        // A workspace can be absent while a create command is in flight. The
        // transaction-scoped key lock serializes that absent-to-present
        // transition before taking the row and membership locks below.
        let workspace_key = Uuid::from(id).to_string();
        self.context
            .transaction
            .query_one(
                "SELECT pg_advisory_xact_lock(hashtextextended($1, 0))",
                &[&workspace_key],
            )
            .await?;
        let Some(row) = self
            .context
            .transaction
            .query_opt(
                "SELECT id, slug, name, created_at FROM workspaces WHERE id = $1 FOR UPDATE",
                &[&Uuid::from(id)],
            )
            .await?
        else {
            return Ok(None);
        };
        self.aggregate_from_workspace_row(row).await.map(Some)
    }

    pub async fn get_for_member(
        &self,
        user_id: UserId,
    ) -> Result<Option<WorkspaceAggregate>, Error> {
        // A user may have no membership yet, so this lock serializes creation
        // of that membership with any command that resolves their workspace.
        let user_key = Uuid::from(user_id).to_string();
        self.context
            .transaction
            .query_one(
                "SELECT pg_advisory_xact_lock(hashtextextended($1, 0))",
                &[&user_key],
            )
            .await?;
        let workspace_id = self
            .context
            .transaction
            .query_opt(
                "SELECT workspace_id FROM workspace_memberships WHERE user_id = $1",
                &[&Uuid::from(user_id)],
            )
            .await?
            .map(|row| row.get::<_, Uuid>("workspace_id"));
        match workspace_id {
            Some(workspace_id) => self.get(workspace_id.into()).await,
            None => Ok(None),
        }
    }

    pub async fn save(&self, aggregate: &WorkspaceAggregate) -> Result<(), Error> {
        let workspace = aggregate.workspace();
        let affected = self
            .context
            .transaction
            .execute(
                r#"
INSERT INTO workspaces (id, slug, name, created_at)
VALUES ($1, $2, $3, $4)
ON CONFLICT (id) DO UPDATE
SET slug = EXCLUDED.slug, name = EXCLUDED.name, created_at = EXCLUDED.created_at
"#,
                &[
                    &Uuid::from(workspace.id),
                    &workspace.slug,
                    &workspace.name,
                    &workspace.created_at,
                ],
            )
            .await
            .map_err(classify_db_error)?;
        if affected != 1 {
            return Err(Error::InvariantViolation(
                "workspace snapshot save affected an unexpected row count",
            ));
        }

        let member_ids = aggregate
            .memberships()
            .iter()
            .map(|membership| Uuid::from(membership.user_id))
            .collect::<Vec<_>>();
        self.context
            .transaction
            .execute(
                "DELETE FROM workspace_memberships WHERE workspace_id = $1 AND NOT (user_id = ANY($2::uuid[]))",
                &[&Uuid::from(workspace.id), &member_ids],
            )
            .await?;

        for membership in aggregate.memberships() {
            let affected = self
                .context
                .transaction
                .execute(
                    r#"
INSERT INTO workspace_memberships (user_id, workspace_id, role, created_at)
VALUES ($1, $2, $3, $4)
ON CONFLICT (user_id) DO UPDATE
SET role = EXCLUDED.role, created_at = EXCLUDED.created_at
WHERE workspace_memberships.workspace_id = EXCLUDED.workspace_id
"#,
                    &[
                        &Uuid::from(membership.user_id),
                        &Uuid::from(membership.workspace_id),
                        &membership.role.as_str(),
                        &membership.created_at,
                    ],
                )
                .await
                .map_err(classify_db_error)?;
            if affected != 1 {
                return Err(Error::Conflict(
                    super::constraints::ConflictKind::WorkspaceMembershipExists,
                ));
            }
        }
        Ok(())
    }

    async fn aggregate_from_workspace_row(&self, row: Row) -> Result<WorkspaceAggregate, Error> {
        let workspace = workspace_from_row(row)?;
        let rows = self
            .context
            .transaction
            .query(
                r#"
SELECT user_id, workspace_id, role, created_at
FROM workspace_memberships
WHERE workspace_id = $1
ORDER BY created_at, user_id
FOR UPDATE
"#,
                &[&Uuid::from(workspace.id)],
            )
            .await?;
        let memberships = rows
            .into_iter()
            .map(workspace_membership_from_row)
            .collect::<Result<Vec<_>, _>>()?;
        WorkspaceAggregate::rehydrate(workspace, memberships).map_err(|_| {
            Error::InvariantViolation("persisted workspace membership snapshot is inconsistent")
        })
    }
}

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

fn workspace_membership_from_row(row: Row) -> Result<WorkspaceMembership, Error> {
    let role = row
        .try_get::<_, String>("role")?
        .parse::<WorkspaceRole>()
        .map_err(|_| Error::InvariantViolation("unknown workspace membership role"))?;
    Ok(WorkspaceMembership {
        user_id: UserId::from(row.try_get::<_, Uuid>("user_id")?),
        workspace_id: WorkspaceId::from(row.try_get::<_, Uuid>("workspace_id")?),
        role,
        created_at: row.try_get("created_at")?,
    })
}
