use tokio_postgres::Row;
use uuid::Uuid;

use crate::{
    domain::{
        CreateEvidencePayload, Evidence, EvidenceId, EvidenceStatus, UpdateEvidencePayload,
        WorkspaceId,
    },
    repository::{WorkspaceReadContext, WorkspaceTransactionContext},
};

use super::Error;

impl WorkspaceTransactionContext<'_> {
    pub async fn create_evidence(
        &self,
        payload: &CreateEvidencePayload,
    ) -> Result<Evidence, Error> {
        let row = self
            .transaction
            .query_one(
                r#"
INSERT INTO evidence (
    workspace_id,
    title,
    description,
    collection_instructions,
    status
)
VALUES ($1, $2, $3, $4, $5)
RETURNING
    id,
    workspace_id,
    title,
    description,
    collection_instructions,
    status,
    created_at,
    updated_at
"#,
                &[
                    &Uuid::from(self.workspace_id),
                    &payload.title,
                    &payload.description,
                    &payload.collection_instructions,
                    &payload.status.as_str(),
                ],
            )
            .await?;

        evidence_from_row(row)
    }

    pub async fn replace_evidence(
        &self,
        id: EvidenceId,
        update: &UpdateEvidencePayload,
    ) -> Result<Option<Evidence>, Error> {
        let rows = self
            .transaction
            .query(
                r#"
UPDATE evidence
SET
    title = $2,
    description = $3,
    collection_instructions = $4,
    status = $5,
    updated_at = now()
WHERE id = $1
  AND workspace_id = $6
RETURNING
    id,
    workspace_id,
    title,
    description,
    collection_instructions,
    status,
    created_at,
    updated_at
"#,
                &[
                    &Uuid::from(id),
                    &update.title,
                    &update.description,
                    &update.collection_instructions,
                    &update.status.as_str(),
                    &Uuid::from(self.workspace_id),
                ],
            )
            .await?;

        rows.into_iter().next().map(evidence_from_row).transpose()
    }
}

impl WorkspaceReadContext {
    pub async fn get_evidence(&self, id: EvidenceId) -> Result<Option<Evidence>, Error> {
        let rows = self
            .client
            .query(
                r#"
SELECT
    id,
    workspace_id,
    title,
    description,
    collection_instructions,
    status,
    created_at,
    updated_at
FROM evidence
WHERE id = $1
  AND workspace_id = $2
"#,
                &[&Uuid::from(id), &Uuid::from(self.workspace_id)],
            )
            .await?;

        rows.into_iter().next().map(evidence_from_row).transpose()
    }

    pub async fn list_evidence(&self) -> Result<Vec<Evidence>, Error> {
        let rows = self
            .client
            .query(
                r#"
SELECT
    id,
    workspace_id,
    title,
    description,
    collection_instructions,
    status,
    created_at,
    updated_at
FROM evidence
WHERE workspace_id = $1
ORDER BY title
"#,
                &[&Uuid::from(self.workspace_id)],
            )
            .await?;

        rows.into_iter().map(evidence_from_row).collect()
    }
}

fn evidence_from_row(row: Row) -> Result<Evidence, Error> {
    let status = row
        .try_get::<_, String>("status")?
        .parse::<EvidenceStatus>()?;
    let workspace_id = WorkspaceId::from(row.try_get::<_, Uuid>("workspace_id")?);

    Ok(Evidence {
        id: EvidenceId::from(row.try_get::<_, Uuid>("id")?),
        workspace_id,
        title: row.try_get("title")?,
        description: row.try_get("description")?,
        collection_instructions: row.try_get("collection_instructions")?,
        status,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}
