use uuid::Uuid;

use crate::{
    domain::{UserId, WorkspaceId, WorkspaceRole},
    persistence::Error,
    read_models::{WorkspaceDetails, WorkspaceWithRole},
};

use super::{ReadExecutor, TransactionalReadExecutor};

pub(crate) struct WorkspaceReads<'a, E> {
    executor: &'a E,
}
impl<'a, E> WorkspaceReads<'a, E> {
    pub(crate) fn new(executor: &'a E) -> Self {
        Self { executor }
    }
}

impl<E: ReadExecutor> WorkspaceReads<'_, E> {
    pub async fn get(&self, id: WorkspaceId) -> Result<Option<WorkspaceDetails>, Error> {
        self.executor
            .query_opt(
                "SELECT id, slug, name, created_at FROM workspaces WHERE id = $1",
                &[&Uuid::from(id)],
            )
            .await?
            .map(workspace_from_row)
            .transpose()
    }

    pub async fn get_for_user(&self, user_id: UserId) -> Result<Option<WorkspaceWithRole>, Error> {
        self.executor
            .query_opt(
                r#"SELECT w.id, w.slug, w.name, w.created_at, m.role
FROM workspace_memberships m
JOIN workspaces w ON w.id = m.workspace_id
WHERE m.user_id = $1"#,
                &[&Uuid::from(user_id)],
            )
            .await?
            .map(|row| {
                Ok(WorkspaceWithRole {
                    workspace: workspace_from_row(row.clone())?,
                    role: row.try_get::<_, String>("role")?.parse().map_err(|_| {
                        Error::InvariantViolation("unknown workspace membership role")
                    })?,
                })
            })
            .transpose()
    }

    pub async fn role_for(
        &self,
        workspace_id: WorkspaceId,
        user_id: UserId,
    ) -> Result<Option<WorkspaceRole>, Error> {
        self.executor
            .query_opt(
                "SELECT role FROM workspace_memberships WHERE workspace_id = $1 AND user_id = $2",
                &[&Uuid::from(workspace_id), &Uuid::from(user_id)],
            )
            .await?
            .map(|row| {
                row.try_get::<_, String>("role")?
                    .parse()
                    .map_err(|_| Error::InvariantViolation("unknown workspace membership role"))
            })
            .transpose()
    }
}

impl WorkspaceReads<'_, TransactionalReadExecutor<'_>> {
    pub async fn resolve_id_for_member(
        &self,
        user_id: UserId,
    ) -> Result<Option<WorkspaceId>, Error> {
        let user_key = Uuid::from(user_id).to_string();
        self.executor
            .query_one(
                "SELECT pg_advisory_xact_lock(hashtextextended($1, 0))",
                &[&user_key],
            )
            .await?;
        self.executor
            .query_opt(
                "SELECT workspace_id FROM workspace_memberships WHERE user_id = $1",
                &[&Uuid::from(user_id)],
            )
            .await?
            .map(|row| {
                row.try_get::<_, Uuid>("workspace_id")
                    .map(WorkspaceId::from)
            })
            .transpose()
            .map_err(Into::into)
    }
}

fn workspace_from_row(row: tokio_postgres::Row) -> Result<WorkspaceDetails, Error> {
    Ok(WorkspaceDetails {
        id: row.try_get::<_, Uuid>("id")?.into(),
        slug: row.try_get("slug")?,
        name: row.try_get("name")?,
        created_at: row.try_get("created_at")?,
    })
}
