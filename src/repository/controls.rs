use tokio_postgres::Row;
use uuid::Uuid;

use crate::{
    domain::{
        Control, ControlId, ControlSummary, CreateControlPayload,
        CreateEvidenceControlMappingPayload, EvidenceControlMapping, EvidenceId, Framework,
        FrameworkId, FrameworkRequirement, FrameworkRequirementId, UpdateControlPayload,
        WorkspaceId,
    },
    repository::{Postgres, WorkspaceReadContext, WorkspaceTransactionContext},
};

use super::{constraints::classify_db_error, Error};

impl Postgres {
    pub async fn list_frameworks(&self) -> Result<Vec<Framework>, Error> {
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

    pub async fn list_framework_requirements(
        &self,
        framework_id: FrameworkId,
    ) -> Result<Vec<FrameworkRequirement>, Error> {
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

        let client = self.get().await?;
        for id in ids {
            let exists = client
                .query_one(
                    "SELECT EXISTS (SELECT 1 FROM framework_requirements WHERE id = $1) AS exists",
                    &[&Uuid::from(*id)],
                )
                .await?
                .try_get::<_, bool>("exists")?;
            if !exists {
                return Ok(false);
            }
        }

        Ok(true)
    }
}

impl WorkspaceTransactionContext<'_> {
    pub async fn create_control(&self, payload: &CreateControlPayload) -> Result<Control, Error> {
        let row = self
            .transaction
            .query_one(
                r#"
INSERT INTO controls (workspace_id, code, title, description)
VALUES ($1, $2, $3, $4)
RETURNING id
"#,
                &[
                    &Uuid::from(self.workspace_id),
                    &payload.code,
                    &payload.title,
                    &payload.description,
                ],
            )
            .await
            .map_err(classify_db_error)?;
        let control_id = ControlId::from(row.try_get::<_, Uuid>("id")?);

        self.replace_control_framework_requirement_mappings(
            control_id,
            &payload.framework_requirement_ids,
        )
        .await?;
        self.get_control_in_transaction(control_id)
            .await?
            .ok_or(Error::InvariantViolation(
                "created control must be readable in transaction",
            ))
    }

    pub async fn replace_control(
        &self,
        id: ControlId,
        payload: &UpdateControlPayload,
    ) -> Result<Option<Control>, Error> {
        let rows = self
            .transaction
            .query(
                r#"
UPDATE controls
SET code = $2,
    title = $3,
    description = $4,
    updated_at = now()
WHERE id = $1
  AND workspace_id = $5
RETURNING id
"#,
                &[
                    &Uuid::from(id),
                    &payload.code,
                    &payload.title,
                    &payload.description,
                    &Uuid::from(self.workspace_id),
                ],
            )
            .await
            .map_err(classify_db_error)?;

        if rows.is_empty() {
            return Ok(None);
        }

        self.replace_control_framework_requirement_mappings(id, &payload.framework_requirement_ids)
            .await?;
        self.get_control_in_transaction(id)
            .await?
            .ok_or(Error::InvariantViolation(
                "updated control must be readable in transaction",
            ))
            .map(Some)
    }

    pub async fn create_evidence_control_mapping(
        &self,
        payload: &CreateEvidenceControlMappingPayload,
    ) -> Result<Option<EvidenceControlMapping>, Error> {
        let rows = self
            .transaction
            .query(
                r#"
INSERT INTO evidence_control_mappings (evidence_id, control_id, rationale)
SELECT er.id, c.id, $3
FROM evidence er
JOIN controls c ON c.id = $2 AND c.workspace_id = $4
WHERE er.id = $1
  AND er.workspace_id = $4
RETURNING evidence_id, control_id
"#,
                &[
                    &Uuid::from(payload.evidence_id),
                    &Uuid::from(payload.control_id),
                    &payload.rationale,
                    &Uuid::from(self.workspace_id),
                ],
            )
            .await
            .map_err(classify_db_error)?;

        if rows.is_empty() {
            return Ok(None);
        }

        self.get_evidence_control_mapping_in_transaction(payload.evidence_id, payload.control_id)
            .await?
            .ok_or(Error::InvariantViolation(
                "created evidence control mapping must be readable in transaction",
            ))
            .map(Some)
    }

    pub async fn delete_evidence_control_mapping(
        &self,
        evidence_id: EvidenceId,
        control_id: ControlId,
    ) -> Result<bool, Error> {
        let rows = self
            .transaction
            .execute(
                r#"
DELETE FROM evidence_control_mappings m
USING evidence er, controls c
WHERE m.evidence_id = er.id
  AND m.control_id = c.id
  AND er.id = $1
  AND c.id = $2
  AND er.workspace_id = $3
  AND c.workspace_id = $3
"#,
                &[
                    &Uuid::from(evidence_id),
                    &Uuid::from(control_id),
                    &Uuid::from(self.workspace_id),
                ],
            )
            .await?;

        Ok(rows > 0)
    }

    async fn get_control_in_transaction(&self, id: ControlId) -> Result<Option<Control>, Error> {
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

    async fn get_evidence_control_mapping_in_transaction(
        &self,
        evidence_id: EvidenceId,
        control_id: ControlId,
    ) -> Result<Option<EvidenceControlMapping>, Error> {
        let rows = self
            .transaction
            .query(
                r#"
SELECT
    m.evidence_id,
    c.id AS control_id,
    c.code AS control_code,
    c.title AS control_title,
    c.description AS control_description,
    m.rationale,
    m.created_at
FROM evidence_control_mappings m
JOIN controls c ON c.id = m.control_id
WHERE m.evidence_id = $1
  AND m.control_id = $2
  AND c.workspace_id = $3
"#,
                &[
                    &Uuid::from(evidence_id),
                    &Uuid::from(control_id),
                    &Uuid::from(self.workspace_id),
                ],
            )
            .await?;

        rows.into_iter()
            .next()
            .map(evidence_control_mapping_from_row)
            .transpose()
    }

    async fn replace_control_framework_requirement_mappings(
        &self,
        control_id: ControlId,
        requirement_ids: &[FrameworkRequirementId],
    ) -> Result<(), Error> {
        self.transaction
            .execute(
                r#"
DELETE FROM control_framework_requirement_mappings m
USING controls c
WHERE m.control_id = c.id
  AND c.id = $1
  AND c.workspace_id = $2
"#,
                &[&Uuid::from(control_id), &Uuid::from(self.workspace_id)],
            )
            .await?;

        for requirement_id in requirement_ids {
            self.transaction
                .execute(
                    r#"
INSERT INTO control_framework_requirement_mappings (control_id, framework_requirement_id)
SELECT c.id, $2
FROM controls c
WHERE c.id = $1
  AND c.workspace_id = $3
ON CONFLICT DO NOTHING
"#,
                    &[
                        &Uuid::from(control_id),
                        &Uuid::from(*requirement_id),
                        &Uuid::from(self.workspace_id),
                    ],
                )
                .await?;
        }

        Ok(())
    }
}

impl WorkspaceReadContext {
    pub async fn list_controls(&self) -> Result<Vec<Control>, Error> {
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

    pub async fn get_control(&self, id: ControlId) -> Result<Option<Control>, Error> {
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

    pub async fn list_evidence_control_mappings(
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

fn framework_from_row(row: Row) -> Result<Framework, Error> {
    Ok(Framework {
        id: FrameworkId::from(row.try_get::<_, Uuid>("id")?),
        code: row.try_get("code")?,
        name: row.try_get("name")?,
        description: row.try_get("description")?,
    })
}

fn framework_requirement_from_row(row: Row) -> Result<FrameworkRequirement, Error> {
    Ok(FrameworkRequirement {
        id: FrameworkRequirementId::from(row.try_get::<_, Uuid>("id")?),
        framework_id: FrameworkId::from(row.try_get::<_, Uuid>("framework_id")?),
        framework_code: row.try_get("framework_code")?,
        framework_name: row.try_get("framework_name")?,
        code: row.try_get("code")?,
        title: row.try_get("title")?,
        description: row.try_get("description")?,
    })
}

fn control_from_row(row: &Row) -> Result<Control, Error> {
    Ok(Control {
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

fn controls_from_joined_rows(rows: Vec<Row>) -> Result<Vec<Control>, Error> {
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

fn push_joined_framework_requirement(control: &mut Control, row: &Row) -> Result<(), Error> {
    let Some(requirement_id) = row.try_get::<_, Option<Uuid>>("framework_requirement_id")? else {
        return Ok(());
    };

    control.framework_requirements.push(FrameworkRequirement {
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
