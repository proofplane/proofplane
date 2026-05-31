use tokio_postgres::{error::SqlState, Row};
use uuid::Uuid;

use crate::{
    domain::{
        Control, ControlId, ControlSummary, CreateControlPayload,
        CreateEvidenceRequestControlMappingPayload, EvidenceRequestControlMapping,
        EvidenceRequestId, Framework, FrameworkId, FrameworkRequirement, FrameworkRequirementId,
        UpdateControlPayload, WorkspaceId,
    },
    repository::Postgres,
    services::{ReadServiceContext, ServiceContext},
};

use super::Error;

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
SELECT id, framework_id, code, title, description
FROM framework_requirements
WHERE framework_id = $1
ORDER BY code
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

impl ServiceContext<'_> {
    pub async fn create_control(
        &self,
        payload: &CreateControlPayload,
    ) -> Result<Option<Control>, Error> {
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
            .map_err(control_insert_error)?;
        let control_id = ControlId::from(row.try_get::<_, Uuid>("id")?);

        self.replace_control_framework_requirement_mappings(
            control_id,
            &payload.framework_requirement_ids,
        )
        .await?;
        self.get_control_in_transaction(control_id).await
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
            .map_err(control_insert_error)?;

        if rows.is_empty() {
            return Ok(None);
        }

        self.replace_control_framework_requirement_mappings(id, &payload.framework_requirement_ids)
            .await?;
        self.get_control_in_transaction(id).await
    }

    pub async fn create_evidence_request_control_mapping(
        &self,
        payload: &CreateEvidenceRequestControlMappingPayload,
    ) -> Result<Option<EvidenceRequestControlMapping>, Error> {
        let rows = self
            .transaction
            .query(
                r#"
INSERT INTO evidence_request_control_mappings (evidence_request_id, control_id, rationale)
SELECT er.id, c.id, $3
FROM evidence_requests er
JOIN controls c ON c.id = $2 AND c.workspace_id = $4
WHERE er.id = $1
  AND er.workspace_id = $4
RETURNING evidence_request_id, control_id
"#,
                &[
                    &Uuid::from(payload.evidence_request_id),
                    &Uuid::from(payload.control_id),
                    &payload.rationale,
                    &Uuid::from(self.workspace_id),
                ],
            )
            .await
            .map_err(mapping_insert_error)?;

        if rows.is_empty() {
            return Ok(None);
        }

        self.get_evidence_request_control_mapping_in_transaction(
            payload.evidence_request_id,
            payload.control_id,
        )
        .await
    }

    pub async fn delete_evidence_request_control_mapping(
        &self,
        evidence_request_id: EvidenceRequestId,
        control_id: ControlId,
    ) -> Result<bool, Error> {
        let rows = self
            .transaction
            .execute(
                r#"
DELETE FROM evidence_request_control_mappings m
USING evidence_requests er, controls c
WHERE m.evidence_request_id = er.id
  AND m.control_id = c.id
  AND er.id = $1
  AND c.id = $2
  AND er.workspace_id = $3
  AND c.workspace_id = $3
"#,
                &[
                    &Uuid::from(evidence_request_id),
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
SELECT id, workspace_id, code, title, description, created_at, updated_at
FROM controls
WHERE id = $1
  AND workspace_id = $2
"#,
                &[&Uuid::from(id), &Uuid::from(self.workspace_id)],
            )
            .await?;

        let Some(row) = rows.into_iter().next() else {
            return Ok(None);
        };
        let mut control = control_from_row(&row)?;
        control.framework_requirements = self
            .control_framework_requirements_in_transaction(id)
            .await?;

        Ok(Some(control))
    }

    async fn get_evidence_request_control_mapping_in_transaction(
        &self,
        evidence_request_id: EvidenceRequestId,
        control_id: ControlId,
    ) -> Result<Option<EvidenceRequestControlMapping>, Error> {
        let rows = self
            .transaction
            .query(
                r#"
SELECT
    m.evidence_request_id,
    c.id AS control_id,
    c.code AS control_code,
    c.title AS control_title,
    c.description AS control_description,
    m.rationale,
    m.created_at
FROM evidence_request_control_mappings m
JOIN controls c ON c.id = m.control_id
WHERE m.evidence_request_id = $1
  AND m.control_id = $2
  AND c.workspace_id = $3
"#,
                &[
                    &Uuid::from(evidence_request_id),
                    &Uuid::from(control_id),
                    &Uuid::from(self.workspace_id),
                ],
            )
            .await?;

        rows.into_iter()
            .next()
            .map(evidence_request_control_mapping_from_row)
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

    async fn control_framework_requirements_in_transaction(
        &self,
        control_id: ControlId,
    ) -> Result<Vec<FrameworkRequirement>, Error> {
        let rows = self
            .transaction
            .query(
                r#"
SELECT fr.id, fr.framework_id, fr.code, fr.title, fr.description
FROM control_framework_requirement_mappings m
JOIN framework_requirements fr ON fr.id = m.framework_requirement_id
WHERE m.control_id = $1
ORDER BY fr.code
"#,
                &[&Uuid::from(control_id)],
            )
            .await?;

        rows.into_iter()
            .map(framework_requirement_from_row)
            .collect()
    }
}

impl ReadServiceContext {
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
    fr.code AS framework_requirement_code,
    fr.title AS framework_requirement_title,
    fr.description AS framework_requirement_description
FROM controls c
LEFT JOIN control_framework_requirement_mappings m ON m.control_id = c.id
LEFT JOIN framework_requirements fr ON fr.id = m.framework_requirement_id
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
SELECT id, workspace_id, code, title, description, created_at, updated_at
FROM controls
WHERE id = $1
  AND workspace_id = $2
"#,
                &[&Uuid::from(id), &Uuid::from(self.workspace_id)],
            )
            .await?;

        let Some(row) = rows.into_iter().next() else {
            return Ok(None);
        };
        let mut control = control_from_row(&row)?;
        control.framework_requirements = self.control_framework_requirements(id).await?;

        Ok(Some(control))
    }

    pub async fn list_evidence_request_control_mappings(
        &self,
        evidence_request_id: EvidenceRequestId,
    ) -> Result<Option<Vec<EvidenceRequestControlMapping>>, Error> {
        if !self.evidence_request_exists(evidence_request_id).await? {
            return Ok(None);
        }

        let rows = self
            .client
            .query(
                r#"
SELECT
    m.evidence_request_id,
    c.id AS control_id,
    c.code AS control_code,
    c.title AS control_title,
    c.description AS control_description,
    m.rationale,
    m.created_at
FROM evidence_request_control_mappings m
JOIN controls c ON c.id = m.control_id
WHERE m.evidence_request_id = $1
  AND c.workspace_id = $2
ORDER BY c.code
"#,
                &[
                    &Uuid::from(evidence_request_id),
                    &Uuid::from(self.workspace_id),
                ],
            )
            .await?;

        rows.into_iter()
            .map(evidence_request_control_mapping_from_row)
            .collect::<Result<Vec<_>, _>>()
            .map(Some)
    }

    async fn control_framework_requirements(
        &self,
        control_id: ControlId,
    ) -> Result<Vec<FrameworkRequirement>, Error> {
        let rows = self
            .client
            .query(
                r#"
SELECT fr.id, fr.framework_id, fr.code, fr.title, fr.description
FROM control_framework_requirement_mappings m
JOIN framework_requirements fr ON fr.id = m.framework_requirement_id
WHERE m.control_id = $1
ORDER BY fr.code
"#,
                &[&Uuid::from(control_id)],
            )
            .await?;

        rows.into_iter()
            .map(framework_requirement_from_row)
            .collect()
    }

    async fn evidence_request_exists(&self, id: EvidenceRequestId) -> Result<bool, Error> {
        let row = self
            .client
            .query_one(
                r#"
SELECT EXISTS (
    SELECT 1
    FROM evidence_requests
    WHERE id = $1
      AND workspace_id = $2
) AS exists
"#,
                &[&Uuid::from(id), &Uuid::from(self.workspace_id)],
            )
            .await?;

        Ok(row.try_get("exists")?)
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
        code: row.try_get("framework_requirement_code")?,
        title: row.try_get("framework_requirement_title")?,
        description: row.try_get("framework_requirement_description")?,
    });

    Ok(())
}

fn evidence_request_control_mapping_from_row(
    row: Row,
) -> Result<EvidenceRequestControlMapping, Error> {
    Ok(EvidenceRequestControlMapping {
        evidence_request_id: EvidenceRequestId::from(
            row.try_get::<_, Uuid>("evidence_request_id")?,
        ),
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

fn control_insert_error(error: tokio_postgres::Error) -> Error {
    if error
        .as_db_error()
        .is_some_and(|db_error| db_error.code() == &SqlState::UNIQUE_VIOLATION)
    {
        return Error::Conflict("duplicate control code");
    }

    Error::Database(error)
}

fn mapping_insert_error(error: tokio_postgres::Error) -> Error {
    if error
        .as_db_error()
        .is_some_and(|db_error| db_error.code() == &SqlState::UNIQUE_VIOLATION)
    {
        return Error::Conflict("duplicate Evidence Request-control mapping");
    }

    Error::Database(error)
}
