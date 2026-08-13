use tokio_postgres::Row;
use uuid::Uuid;

use crate::domain::{UserId, Workspace, WorkspaceId, WorkspaceMembership, WorkspaceRole};

use super::params::param;
use super::{
    constraints::classify_db_error,
    snapshot::{save_snapshot, snapshot_record},
    Error, UnitOfWork,
};

/// Complete-snapshot persistence for a workspace and all of its memberships.
pub struct WorkspaceRepository<'a> {
    unit_of_work: &'a UnitOfWork<'a>,
}

impl<'a> UnitOfWork<'a> {
    pub fn workspaces(&'a self) -> WorkspaceRepository<'a> {
        WorkspaceRepository { unit_of_work: self }
    }
}

impl WorkspaceRepository<'_> {
    pub async fn get(&self, id: WorkspaceId) -> Result<Option<Workspace>, Error> {
        // A workspace can be absent while a create command is in flight. The
        // transaction-scoped key lock serializes that absent-to-present
        // transition before taking the row and membership locks below.
        let workspace_key = Uuid::from(id).to_string();
        self.unit_of_work
            .transaction
            .query_typed_one(
                "SELECT pg_advisory_xact_lock(hashtextextended($1, 0))",
                &[param(&workspace_key)],
            )
            .await?;
        let Some(row) = self
            .unit_of_work
            .transaction
            .query_typed_opt(
                "SELECT id, slug, name, created_at FROM workspaces WHERE id = $1 FOR UPDATE",
                &[param(&Uuid::from(id))],
            )
            .await?
        else {
            return Ok(None);
        };
        self.aggregate_from_workspace_row(row).await.map(Some)
    }

    pub async fn save(&self, workspace: &Workspace) -> Result<(), Error> {
        let record = WorkspaceRecord::from_domain(workspace)?;
        save_snapshot(&self.unit_of_work.transaction, record.as_snapshot()).await?;
        self.unit_of_work
            .transaction
            .execute_typed(
                "DELETE FROM workspace_memberships WHERE workspace_id = $1",
                &[param(&Uuid::from(workspace.id()))],
            )
            .await?;

        for membership in workspace.memberships() {
            let membership = WorkspaceMembershipRecord::from_domain(membership);
            let affected = self
                .unit_of_work
                .transaction
                .execute_typed(
                    "INSERT INTO workspace_memberships (user_id, workspace_id, role, created_at) VALUES ($1, $2, $3, $4)",
                    &[param(&membership.user_id), param(&membership.workspace_id), param(&membership.role), param(&membership.created_at)],
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
        let workspace = WorkspaceRecord::try_from_row(&row)?;
        let rows = self
            .unit_of_work
            .transaction
            .query_typed(
                r#"
SELECT user_id, workspace_id, role, created_at
FROM workspace_memberships
WHERE workspace_id = $1
ORDER BY created_at, user_id
FOR UPDATE
"#,
                &[param(&workspace.id)],
            )
            .await?;
        let memberships = rows
            .into_iter()
            .map(|row| workspace_membership_from_row(&row))
            .collect::<Result<Vec<_>, _>>()?;
        workspace.into_domain(memberships)
    }
}

struct WorkspaceMembershipRecord {
    user_id: Uuid,
    workspace_id: Uuid,
    role: String,
    created_at: chrono::DateTime<chrono::Utc>,
}

impl WorkspaceMembershipRecord {
    fn from_domain(membership: &WorkspaceMembership) -> Self {
        Self {
            user_id: membership.user_id.into(),
            workspace_id: membership.workspace_id.into(),
            role: membership.role.as_str().to_owned(),
            created_at: membership.created_at,
        }
    }
}

snapshot_record! {
    struct WorkspaceRecord {
        id: Uuid,
        slug: Option<String>,
        name: String,
        created_at: chrono::DateTime<chrono::Utc>,
    }
    table: workspaces,
    conflict: id,
}

impl WorkspaceRecord {
    fn try_from_row(row: &Row) -> Result<Self, Error> {
        Ok(Self {
            id: row.try_get("id")?,
            slug: row.try_get("slug")?,
            name: row.try_get("name")?,
            created_at: row.try_get("created_at")?,
        })
    }

    fn from_domain(workspace: &Workspace) -> Result<Self, Error> {
        Ok(Self {
            id: workspace.id().into(),
            slug: workspace.slug().map(str::to_owned),
            name: workspace.name().to_owned(),
            created_at: workspace.created_at(),
        })
    }

    fn into_domain(self, memberships: Vec<WorkspaceMembership>) -> Result<Workspace, Error> {
        Workspace::rehydrate(
            self.id.into(),
            self.slug,
            self.name,
            self.created_at,
            memberships,
        )
        .map_err(|_| {
            Error::InvariantViolation("persisted workspace membership snapshot is inconsistent")
        })
    }
}

fn workspace_membership_from_row(row: &Row) -> Result<WorkspaceMembership, Error> {
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
