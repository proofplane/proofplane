use tokio_postgres::Row;
use uuid::Uuid;

use crate::{
    domain::{UserId, Workspace, WorkspaceId, WorkspaceMembership, WorkspaceRole},
    projections::WorkspaceDetails,
};

use super::{constraints::classify_db_error, Error, Postgres, TransactionContext};

/// Complete-snapshot persistence for a workspace and all of its memberships.
pub struct WorkspaceRepository<'a> {
    context: &'a TransactionContext<'a>,
}

impl<'a> TransactionContext<'a> {
    pub fn workspaces(&'a self) -> WorkspaceRepository<'a> {
        WorkspaceRepository { context: self }
    }
}

impl WorkspaceRepository<'_> {
    pub async fn get(&self, id: WorkspaceId) -> Result<Option<Workspace>, Error> {
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

    pub async fn get_for_member(&self, user_id: UserId) -> Result<Option<Workspace>, Error> {
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

    pub async fn save(&self, workspace: &Workspace) -> Result<(), Error> {
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
                    &Uuid::from(workspace.id()),
                    &workspace.slug(),
                    &workspace.name(),
                    &workspace.created_at(),
                ],
            )
            .await
            .map_err(classify_db_error)?;
        if affected != 1 {
            return Err(Error::InvariantViolation(
                "workspace snapshot save affected an unexpected row count",
            ));
        }

        let member_ids = workspace
            .memberships()
            .iter()
            .map(|membership| Uuid::from(membership.user_id))
            .collect::<Vec<_>>();
        self.context
            .transaction
            .execute(
                "DELETE FROM workspace_memberships WHERE workspace_id = $1 AND NOT (user_id = ANY($2::uuid[]))",
                &[&Uuid::from(workspace.id()), &member_ids],
            )
            .await?;

        for membership in workspace.memberships() {
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

    async fn aggregate_from_workspace_row(&self, row: Row) -> Result<Workspace, Error> {
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
        Workspace::rehydrate(
            workspace.id,
            workspace.slug,
            workspace.name,
            workspace.created_at,
            memberships,
        )
        .map_err(|_| {
            Error::InvariantViolation("persisted workspace membership snapshot is inconsistent")
        })
    }
}

// TODO: add cursor-based pagination to repository list behavior.
impl Postgres {
    pub(super) async fn load_workspace_details(
        &self,
        id: WorkspaceId,
    ) -> Result<Option<WorkspaceDetails>, Error> {
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

    pub(super) async fn load_workspace_details_list(&self) -> Result<Vec<WorkspaceDetails>, Error> {
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
}

fn workspace_from_row(row: Row) -> Result<WorkspaceDetails, Error> {
    Ok(WorkspaceDetails {
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
