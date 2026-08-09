use std::collections::HashSet;

use chrono::{DateTime, Utc};
use tokio_postgres::Row;
use uuid::Uuid;

use crate::{
    domain::{
        Control, ControlDefinition, ControlId, EvidenceId, FrameworkId, FrameworkRequirementId,
        WorkspaceId,
    },
    projections::{
        ControlDetail, ControlSummary, EvidenceControlMapping, FrameworkDetail,
        FrameworkRequirementDetail,
    },
    repository::{Postgres, WorkspaceClient, WorkspaceRepositories},
};

use super::{
    constraints::classify_db_error,
    snapshot::{save_workspace_snapshot, workspace_snapshot_record},
    Error,
};

/// Workspace-scoped complete-snapshot repository for the control aggregate.
pub struct ControlRepository<'a> {
    context: &'a WorkspaceRepositories<'a>,
}

impl<'a> WorkspaceRepositories<'a> {
    pub fn controls(&'a self) -> ControlRepository<'a> {
        ControlRepository { context: self }
    }
}

impl ControlRepository<'_> {
    pub async fn get(&self, id: ControlId) -> Result<Option<Control>, Error> {
        let Some(row) = self
            .context
            .transaction
            .query_opt(
                r#"
SELECT id, workspace_id, code, title, description, created_at, updated_at
FROM controls
WHERE id = $1
  AND workspace_id = $2
FOR UPDATE
"#,
                &[&Uuid::from(id), &Uuid::from(self.context.workspace_id)],
            )
            .await?
        else {
            return Ok(None);
        };
        let record = ControlRecord::try_from(row)?;
        let requirement_ids = self
            .context
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

        record.into_aggregate(requirement_ids).map(Some)
    }

    /// Persists the aggregate's complete definition and reference snapshot.
    pub async fn save(&self, control: &Control) -> Result<(), Error> {
        if control.workspace_id() != self.context.workspace_id {
            return Err(Error::InvariantViolation(
                "control workspace must match its repository scope",
            ));
        }
        let record = ControlRecord::from(control);
        save_workspace_snapshot(self.context.transaction, record.as_workspace_snapshot())
            .await
            .map_err(|error| match error {
                Error::Database(error) => classify_db_error(error),
                other => other,
            })?;

        self.context
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
        self.context
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

workspace_snapshot_record! {
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
    scope: workspace_id,
}

impl TryFrom<Row> for ControlRecord {
    type Error = Error;

    fn try_from(row: Row) -> Result<Self, Self::Error> {
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
    fn into_aggregate(
        self,
        requirement_ids: Vec<FrameworkRequirementId>,
    ) -> Result<Control, Error> {
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

impl From<&Control> for ControlRecord {
    fn from(control: &Control) -> Self {
        Self {
            id: control.id().into(),
            workspace_id: control.workspace_id().into(),
            code: control.code().to_owned(),
            title: control.title().to_owned(),
            description: control.description().to_owned(),
            created_at: control.created_at(),
            updated_at: control.updated_at(),
        }
    }
}

impl Postgres {
    pub(super) async fn load_frameworks(&self) -> Result<Vec<FrameworkDetail>, Error> {
        let client = self.get().await?;
        let rows = client
            .query(
                r#"
SELECT id, code, name, description
FROM frameworks
ORDER BY code
"#,
                &[],
            )
            .await?;

        rows.into_iter().map(framework_from_row).collect()
    }

    pub(super) async fn load_framework_requirements(
        &self,
        framework_id: FrameworkId,
    ) -> Result<Vec<FrameworkRequirementDetail>, Error> {
        let client = self.get().await?;
        let rows = client
            .query(
                r#"
SELECT
    fr.id,
    fr.framework_id,
    f.code AS framework_code,
    f.name AS framework_name,
    fr.code,
    fr.title,
    fr.description
FROM framework_requirements fr
JOIN frameworks f ON f.id = fr.framework_id
WHERE fr.framework_id = $1
ORDER BY fr.code
"#,
                &[&Uuid::from(framework_id)],
            )
            .await?;

        rows.into_iter()
            .map(framework_requirement_from_row)
            .collect()
    }

    pub async fn framework_requirements_exist(
        &self,
        ids: &[FrameworkRequirementId],
    ) -> Result<bool, Error> {
        if ids.is_empty() {
            return Ok(true);
        }

        // Counted distinctly on both sides: duplicates are rejected upstream at
        // the MCP parse layer, so this only keeps a repeated id from being
        // reported as unknown if one ever reaches here.
        let requested = ids.iter().copied().map(Uuid::from).collect::<HashSet<_>>();
        let found = self
            .get()
            .await?
            .query_one(
                r#"
SELECT count(DISTINCT id) AS found
FROM framework_requirements
WHERE id = ANY($1)
"#,
                &[&requested.iter().copied().collect::<Vec<_>>()],
            )
            .await?
            .try_get::<_, i64>("found")?;

        Ok(found == requested.len() as i64)
    }
}

impl WorkspaceRepositories<'_> {
    pub async fn framework_requirements_exist(
        &self,
        ids: &[FrameworkRequirementId],
    ) -> Result<bool, Error> {
        if ids.is_empty() {
            return Ok(true);
        }
        let requested = ids.iter().copied().map(Uuid::from).collect::<HashSet<_>>();
        let found = self
            .transaction
            .query_one(
                r#"
SELECT count(DISTINCT id) AS found
FROM framework_requirements
WHERE id = ANY($1)
"#,
                &[&requested.iter().copied().collect::<Vec<_>>()],
            )
            .await?
            .try_get::<_, i64>("found")?;
        Ok(found == requested.len() as i64)
    }

    pub(super) async fn load_control_detail(
        &self,
        id: ControlId,
    ) -> Result<Option<ControlDetail>, Error> {
        let rows = self
            .transaction
            .query(
                r#"
SELECT
    c.id,
    c.workspace_id,
    c.code,
    c.title,
    c.description,
    c.created_at,
    c.updated_at,
    fr.id AS framework_requirement_id,
    fr.framework_id AS framework_requirement_framework_id,
    f.code AS framework_requirement_framework_code,
    f.name AS framework_requirement_framework_name,
    fr.code AS framework_requirement_code,
    fr.title AS framework_requirement_title,
    fr.description AS framework_requirement_description
FROM controls c
LEFT JOIN control_framework_requirement_mappings m ON m.control_id = c.id
LEFT JOIN framework_requirements fr ON fr.id = m.framework_requirement_id
LEFT JOIN frameworks f ON f.id = fr.framework_id
WHERE c.id = $1
  AND c.workspace_id = $2
ORDER BY fr.code
"#,
                &[&Uuid::from(id), &Uuid::from(self.workspace_id)],
            )
            .await?;

        Ok(controls_from_joined_rows(rows)?.into_iter().next())
    }
}

impl WorkspaceClient {
    pub(super) async fn load_control_details(&self) -> Result<Vec<ControlDetail>, Error> {
        let rows = self
            .client
            .query(
                r#"
SELECT
    c.id,
    c.workspace_id,
    c.code,
    c.title,
    c.description,
    c.created_at,
    c.updated_at,
    fr.id AS framework_requirement_id,
    fr.framework_id AS framework_requirement_framework_id,
    f.code AS framework_requirement_framework_code,
    f.name AS framework_requirement_framework_name,
    fr.code AS framework_requirement_code,
    fr.title AS framework_requirement_title,
    fr.description AS framework_requirement_description
FROM controls c
LEFT JOIN control_framework_requirement_mappings m ON m.control_id = c.id
LEFT JOIN framework_requirements fr ON fr.id = m.framework_requirement_id
LEFT JOIN frameworks f ON f.id = fr.framework_id
WHERE c.workspace_id = $1
ORDER BY c.code, fr.code
"#,
                &[&Uuid::from(self.workspace_id)],
            )
            .await?;

        controls_from_joined_rows(rows)
    }

    pub(super) async fn load_control_detail(
        &self,
        id: ControlId,
    ) -> Result<Option<ControlDetail>, Error> {
        let rows = self
            .client
            .query(
                r#"
SELECT
    c.id,
    c.workspace_id,
    c.code,
    c.title,
    c.description,
    c.created_at,
    c.updated_at,
    fr.id AS framework_requirement_id,
    fr.framework_id AS framework_requirement_framework_id,
    f.code AS framework_requirement_framework_code,
    f.name AS framework_requirement_framework_name,
    fr.code AS framework_requirement_code,
    fr.title AS framework_requirement_title,
    fr.description AS framework_requirement_description
FROM controls c
LEFT JOIN control_framework_requirement_mappings m ON m.control_id = c.id
LEFT JOIN framework_requirements fr ON fr.id = m.framework_requirement_id
LEFT JOIN frameworks f ON f.id = fr.framework_id
WHERE c.id = $1
  AND c.workspace_id = $2
ORDER BY fr.code
"#,
                &[&Uuid::from(id), &Uuid::from(self.workspace_id)],
            )
            .await?;

        Ok(controls_from_joined_rows(rows)?.into_iter().next())
    }

    pub(super) async fn load_evidence_control_mappings(
        &self,
        evidence_id: EvidenceId,
    ) -> Result<Option<Vec<EvidenceControlMapping>>, Error> {
        let rows = self
            .client
            .query(
                r#"
SELECT
    er.id AS evidence_id,
    c.id AS control_id,
    c.code AS control_code,
    c.title AS control_title,
    c.description AS control_description,
    m.rationale,
    m.created_at
FROM evidence er
LEFT JOIN evidence_control_mappings m ON m.evidence_id = er.id
LEFT JOIN controls c ON c.id = m.control_id AND c.workspace_id = er.workspace_id
WHERE er.id = $1
  AND er.workspace_id = $2
ORDER BY c.code
"#,
                &[&Uuid::from(evidence_id), &Uuid::from(self.workspace_id)],
            )
            .await?;

        if rows.is_empty() {
            return Ok(None);
        }

        rows.into_iter()
            .filter_map(evidence_control_mapping_from_joined_row)
            .collect::<Result<Vec<_>, _>>()
            .map(Some)
    }
}

fn framework_from_row(row: Row) -> Result<FrameworkDetail, Error> {
    Ok(FrameworkDetail {
        id: FrameworkId::from(row.try_get::<_, Uuid>("id")?),
        code: row.try_get("code")?,
        name: row.try_get("name")?,
        description: row.try_get("description")?,
    })
}

fn framework_requirement_from_row(row: Row) -> Result<FrameworkRequirementDetail, Error> {
    Ok(FrameworkRequirementDetail {
        id: FrameworkRequirementId::from(row.try_get::<_, Uuid>("id")?),
        framework_id: FrameworkId::from(row.try_get::<_, Uuid>("framework_id")?),
        framework_code: row.try_get("framework_code")?,
        framework_name: row.try_get("framework_name")?,
        code: row.try_get("code")?,
        title: row.try_get("title")?,
        description: row.try_get("description")?,
    })
}

fn control_from_row(row: &Row) -> Result<ControlDetail, Error> {
    Ok(ControlDetail {
        id: ControlId::from(row.try_get::<_, Uuid>("id")?),
        workspace_id: WorkspaceId::from(row.try_get::<_, Uuid>("workspace_id")?),
        code: row.try_get("code")?,
        title: row.try_get("title")?,
        description: row.try_get("description")?,
        framework_requirements: Vec::new(),
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

fn controls_from_joined_rows(rows: Vec<Row>) -> Result<Vec<ControlDetail>, Error> {
    let mut controls = Vec::new();
    let mut current_control_id = None;
    let mut current_control_index = None;

    for row in rows {
        let control_id = ControlId::from(row.try_get::<_, Uuid>("id")?);
        if current_control_id != Some(control_id) {
            controls.push(control_from_row(&row)?);
            current_control_id = Some(control_id);
            current_control_index = controls.len().checked_sub(1);
        }

        if let Some(control) = current_control_index.and_then(|index| controls.get_mut(index)) {
            push_joined_framework_requirement(control, &row)?;
        }
    }

    Ok(controls)
}

fn push_joined_framework_requirement(control: &mut ControlDetail, row: &Row) -> Result<(), Error> {
    let Some(requirement_id) = row.try_get::<_, Option<Uuid>>("framework_requirement_id")? else {
        return Ok(());
    };

    control
        .framework_requirements
        .push(FrameworkRequirementDetail {
            id: FrameworkRequirementId::from(requirement_id),
            framework_id: FrameworkId::from(
                row.try_get::<_, Uuid>("framework_requirement_framework_id")?,
            ),
            framework_code: row.try_get("framework_requirement_framework_code")?,
            framework_name: row.try_get("framework_requirement_framework_name")?,
            code: row.try_get("framework_requirement_code")?,
            title: row.try_get("framework_requirement_title")?,
            description: row.try_get("framework_requirement_description")?,
        });

    Ok(())
}

fn evidence_control_mapping_from_row(row: Row) -> Result<EvidenceControlMapping, Error> {
    Ok(EvidenceControlMapping {
        evidence_id: EvidenceId::from(row.try_get::<_, Uuid>("evidence_id")?),
        control: ControlSummary {
            id: ControlId::from(row.try_get::<_, Uuid>("control_id")?),
            code: row.try_get("control_code")?,
            title: row.try_get("control_title")?,
            description: row.try_get("control_description")?,
        },
        rationale: row.try_get("rationale")?,
        created_at: row.try_get("created_at")?,
    })
}

fn evidence_control_mapping_from_joined_row(
    row: Row,
) -> Option<Result<EvidenceControlMapping, Error>> {
    match row.try_get::<_, Option<Uuid>>("control_id") {
        Ok(Some(_)) => Some(evidence_control_mapping_from_row(row)),
        Ok(None) => None,
        Err(error) => Some(Err(Error::Database(error))),
    }
}
