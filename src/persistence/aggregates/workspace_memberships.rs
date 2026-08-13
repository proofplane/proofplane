use tokio_postgres::Row;
use uuid::Uuid;

use crate::domain::{UserId, WorkspaceId, WorkspaceMembership, WorkspaceRole};

use super::params::param;
use super::{constraints::classify_db_error, Error, Postgres, UnitOfWork};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewWorkspaceMembership {
    pub user_id: UserId,
    pub workspace_id: WorkspaceId,
    pub role: WorkspaceRole,
}

impl UnitOfWork<'_> {
    pub async fn get_membership_role(
        &self,
        workspace_id: WorkspaceId,
        user_id: UserId,
    ) -> Result<Option<WorkspaceRole>, Error> {
        let rows = self
            .transaction
            .query_typed(
                "SELECT role FROM workspace_memberships WHERE workspace_id = $1 AND user_id = $2 FOR UPDATE",
                &[param(&Uuid::from(workspace_id)), param(&Uuid::from(user_id))],
            )
            .await?;

        rows.into_iter().next().map(role_from_row).transpose()
    }

    pub async fn insert_workspace_membership(
        &self,
        membership: &NewWorkspaceMembership,
    ) -> Result<WorkspaceMembership, Error> {
        let row = self
            .transaction
            .query_typed_one(
                r#"
INSERT INTO workspace_memberships (user_id, workspace_id, role)
VALUES ($1, $2, $3)
RETURNING user_id, workspace_id, role, created_at
"#,
                &[
                    param(&Uuid::from(membership.user_id)),
                    param(&Uuid::from(membership.workspace_id)),
                    param(&membership.role.as_str()),
                ],
            )
            .await
            .map_err(classify_db_error)?;

        membership_from_row(row)
    }
}

impl Postgres {
    pub async fn get_membership_role(
        &self,
        workspace_id: WorkspaceId,
        user_id: UserId,
    ) -> Result<Option<WorkspaceRole>, Error> {
        let rows = self
            .get()
            .await?
            .query_typed(
                "SELECT role FROM workspace_memberships WHERE workspace_id = $1 AND user_id = $2",
                &[
                    param(&Uuid::from(workspace_id)),
                    param(&Uuid::from(user_id)),
                ],
            )
            .await?;

        rows.into_iter().next().map(role_from_row).transpose()
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

fn role_from_value(value: String) -> Result<WorkspaceRole, Error> {
    value
        .parse()
        .map_err(|_| Error::InvariantViolation("unknown workspace membership role"))
}
