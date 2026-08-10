use chrono::{DateTime, Utc};
use tokio_postgres::Row;
use uuid::Uuid;

use crate::{
    domain::{Control, ControlDefinition, ControlId, FrameworkRequirementId},
    persistence::WorkspaceUnitOfWork,
};

use super::{
    constraints::classify_db_error,
    snapshot::{save_snapshot, snapshot_record},
    Error,
};

/// Workspace-scoped complete-snapshot repository for the control aggregate.
pub struct ControlRepository<'a> {
    workspace: &'a WorkspaceUnitOfWork<'a>,
}

impl<'a> WorkspaceUnitOfWork<'a> {
    pub fn controls(&'a self) -> ControlRepository<'a> {
        ControlRepository { workspace: self }
    }
}

impl ControlRepository<'_> {
    pub async fn get(&self, id: ControlId) -> Result<Option<Control>, Error> {
        let Some(row) = self
            .workspace
            .transaction
            .query_opt(
                r#"
SELECT id, workspace_id, code, title, description, created_at, updated_at
FROM controls
WHERE id = $1 AND workspace_id = $2
FOR UPDATE
"#,
                &[&Uuid::from(id), &Uuid::from(self.workspace.workspace_id)],
            )
            .await?
        else {
            return Ok(None);
        };
        let record = ControlRecord::try_from_row(&row)?;
        let requirement_ids = self
            .workspace
            .transaction
            .query(
                r#"
SELECT framework_requirement_id
FROM control_framework_requirement_mappings
WHERE control_id = $1
ORDER BY framework_requirement_id
"#,
                &[&Uuid::from(id)],
            )
            .await?
            .into_iter()
            .map(|row| {
                row.try_get::<_, Uuid>("framework_requirement_id")
                    .map(FrameworkRequirementId::from)
                    .map_err(Error::from)
            })
            .collect::<Result<Vec<_>, _>>()?;
        record.into_domain(requirement_ids).map(Some)
    }

    /// Persists the aggregate's complete definition and reference snapshot.
    pub async fn save(&self, control: &Control) -> Result<(), Error> {
        let record = ControlRecord::from_domain(control)?;
        save_snapshot(self.workspace.transaction, record.as_snapshot())
            .await
            .map_err(|error| match error {
                Error::Database(error) => classify_db_error(error),
                other => other,
            })?;
        self.workspace
            .transaction
            .execute(
                "DELETE FROM control_framework_requirement_mappings WHERE control_id = $1",
                &[&Uuid::from(control.id())],
            )
            .await?;
        let requirement_ids = control
            .framework_requirement_ids()
            .iter()
            .copied()
            .map(Uuid::from)
            .collect::<Vec<_>>();
        self.workspace
            .transaction
            .execute(
                r#"
INSERT INTO control_framework_requirement_mappings (
    control_id,
    framework_requirement_id
)
SELECT $1, requested.framework_requirement_id
FROM unnest($2::uuid[]) AS requested(framework_requirement_id)
"#,
                &[&Uuid::from(control.id()), &requirement_ids],
            )
            .await?;
        Ok(())
    }
}

snapshot_record! {
    struct ControlRecord {
        id: Uuid,
        workspace_id: Uuid,
        code: String,
        title: String,
        description: String,
        created_at: DateTime<Utc>,
        updated_at: DateTime<Utc>,
    }
    table: controls,
    conflict: id,
}

impl ControlRecord {
    fn try_from_row(row: &Row) -> Result<Self, Error> {
        Ok(Self {
            id: row.try_get("id")?,
            workspace_id: row.try_get("workspace_id")?,
            code: row.try_get("code")?,
            title: row.try_get("title")?,
            description: row.try_get("description")?,
            created_at: row.try_get("created_at")?,
            updated_at: row.try_get("updated_at")?,
        })
    }
}

impl ControlRecord {
    fn from_domain(control: &Control) -> Result<Self, Error> {
        Ok(Self {
            id: control.id().into(),
            workspace_id: control.workspace_id().into(),
            code: control.code().to_owned(),
            title: control.title().to_owned(),
            description: control.description().to_owned(),
            created_at: control.created_at(),
            updated_at: control.updated_at(),
        })
    }

    fn into_domain(self, requirement_ids: Vec<FrameworkRequirementId>) -> Result<Control, Error> {
        let definition = ControlDefinition::new(self.code, self.title, self.description)
            .into_result()
            .map_err(|_| Error::InvariantViolation("persisted control definition is invalid"))?;
        Control::rehydrate(
            self.id.into(),
            self.workspace_id.into(),
            definition,
            requirement_ids,
            self.created_at,
            self.updated_at,
        )
        .map_err(|_| Error::InvariantViolation("persisted control snapshot is inconsistent"))
    }
}
