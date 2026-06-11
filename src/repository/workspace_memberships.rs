use deadpool_postgres::GenericClient;
use tokio_postgres::Row;
use uuid::Uuid;

use crate::domain::{
    UserId, Workspace, WorkspaceId, WorkspaceMembership, WorkspaceRole, WorkspaceWithRole,
};

use super::{constraints::classify_db_error, Error, Postgres, TransactionContext};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewWorkspaceMembership {
    pub user_id: UserId,
    pub workspace_id: WorkspaceId,
    pub role: WorkspaceRole,
}

impl TransactionContext<'_> {
    pub async fn insert_workspace_membership(
        &self,
        membership: &NewWorkspaceMembership,
    ) -> Result<WorkspaceMembership, Error> {
        let row = self
            .transaction
            .query_one(
                r#"
INSERT INTO workspace_memberships (user_id, workspace_id, role)
VALUES ($1, $2, $3)
RETURNING user_id, workspace_id, role, created_at
"#,
                &[
                    &Uuid::from(membership.user_id),
                    &Uuid::from(membership.workspace_id),
                    &membership.role.as_str(),
                ],
            )
            .await
            .map_err(classify_db_error)?;

        membership_from_row(row)
    }

    pub async fn delete_workspace_membership(
        &self,
        workspace_id: WorkspaceId,
        user_id: UserId,
    ) -> Result<bool, Error> {
        let deleted = self
            .transaction
            .execute(
                "DELETE FROM workspace_memberships WHERE workspace_id = $1 AND user_id = $2",
                &[&Uuid::from(workspace_id), &Uuid::from(user_id)],
            )
            .await?;

        Ok(deleted > 0)
    }

    pub async fn update_workspace_membership_role(
        &self,
        workspace_id: WorkspaceId,
        user_id: UserId,
        role: WorkspaceRole,
    ) -> Result<bool, Error> {
        let updated = self
            .transaction
            .execute(
                "UPDATE workspace_memberships SET role = $3 WHERE workspace_id = $1 AND user_id = $2",
                &[
                    &Uuid::from(workspace_id),
                    &Uuid::from(user_id),
                    &role.as_str(),
                ],
            )
            .await?;

        Ok(updated > 0)
    }

    pub async fn get_membership(
        &self,
        workspace_id: WorkspaceId,
        user_id: UserId,
    ) -> Result<Option<WorkspaceMembership>, Error> {
        let rows = self
            .transaction
            .query(
                r#"
SELECT user_id, workspace_id, role, created_at
FROM workspace_memberships
WHERE workspace_id = $1 AND user_id = $2
"#,
                &[&Uuid::from(workspace_id), &Uuid::from(user_id)],
            )
            .await?;

        rows.into_iter().next().map(membership_from_row).transpose()
    }

    pub async fn count_workspace_owners(&self, workspace_id: WorkspaceId) -> Result<i64, Error> {
        // `FOR UPDATE` cannot sit alongside an aggregate, so lock the owner rows
        // in a subquery and count the locked set. The lock makes the last-owner
        // guard race-safe against a concurrent removal in another transaction.
        let row = self
            .transaction
            .query_one(
                r#"
SELECT count(*) AS owner_count
FROM (
    SELECT 1
    FROM workspace_memberships
    WHERE workspace_id = $1 AND role = 'owner'
    FOR UPDATE
) AS locked_owners
"#,
                &[&Uuid::from(workspace_id)],
            )
            .await?;

        Ok(row.try_get("owner_count")?)
    }
}

impl Postgres {
    pub async fn get_membership_role(
        &self,
        workspace_id: WorkspaceId,
        user_id: UserId,
    ) -> Result<Option<WorkspaceRole>, Error> {
        let client = self.get().await?;
        let rows = client
            .query(
                "SELECT role FROM workspace_memberships WHERE workspace_id = $1 AND user_id = $2",
                &[&Uuid::from(workspace_id), &Uuid::from(user_id)],
            )
            .await?;

        rows.into_iter().next().map(role_from_row).transpose()
    }

    pub async fn list_workspaces_with_role_for_user(
        &self,
        user_id: UserId,
    ) -> Result<Vec<WorkspaceWithRole>, Error> {
        let client = self.get().await?;
        let rows = client
            .query(
                r#"
SELECT
    w.id,
    w.slug,
    w.name,
    w.created_at,
    m.role
FROM workspace_memberships m
JOIN workspaces w ON w.id = m.workspace_id
WHERE m.user_id = $1
ORDER BY w.created_at, w.id
"#,
                &[&Uuid::from(user_id)],
            )
            .await?;

        rows.into_iter().map(workspace_with_role_from_row).collect()
    }
}

fn membership_from_row(row: Row) -> Result<WorkspaceMembership, Error> {
    Ok(WorkspaceMembership {
        user_id: UserId::from(row.try_get::<_, Uuid>("user_id")?),
        workspace_id: WorkspaceId::from(row.try_get::<_, Uuid>("workspace_id")?),
        role: role_from_value(row.try_get::<_, String>("role")?)?,
        created_at: row.try_get("created_at")?,
    })
}

fn role_from_row(row: Row) -> Result<WorkspaceRole, Error> {
    role_from_value(row.try_get::<_, String>("role")?)
}

fn workspace_with_role_from_row(row: Row) -> Result<WorkspaceWithRole, Error> {
    Ok(WorkspaceWithRole {
        workspace: Workspace {
            id: WorkspaceId::from(row.try_get::<_, Uuid>("id")?),
            slug: row.try_get("slug")?,
            name: row.try_get("name")?,
            created_at: row.try_get("created_at")?,
        },
        role: role_from_value(row.try_get::<_, String>("role")?)?,
    })
}

fn role_from_value(value: String) -> Result<WorkspaceRole, Error> {
    value
        .parse()
        .map_err(|_| Error::InvariantViolation("unknown workspace membership role"))
}
