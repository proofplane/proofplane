use chrono::{DateTime, Utc};
use tokio_postgres::Row;
use uuid::Uuid;

use crate::{
    domain::{
        CreateEvidenceRequestPayload, EvidenceRequest, EvidenceRequestCadence, EvidenceRequestId,
        EvidenceRequestStatus, UpdateEvidenceRequestPayload, WorkspaceId,
    },
    repository::{ActorReadContext, ActorTransactionContext},
};

use super::Error;

impl ActorTransactionContext<'_> {
    pub async fn create_evidence_request(
        &self,
        request: &CreateEvidenceRequestPayload,
    ) -> Result<EvidenceRequest, Error> {
        let row = self
            .transaction
            .query_one(
                r#"
INSERT INTO evidence_requests (
    workspace_id,
    title,
    description,
    collection_instructions,
    cadence,
    due_at,
    schedule_anchor_at,
    freshness_window_days,
    status
)
VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
RETURNING
    id,
    workspace_id,
    title,
    description,
    collection_instructions,
    cadence,
    due_at,
    schedule_anchor_at,
    freshness_window_days,
    status,
    created_at,
    updated_at
"#,
                &[
                    &Uuid::from(self.workspace_id),
                    &request.title,
                    &request.description,
                    &request.collection_instructions,
                    &request.cadence.as_str(),
                    &request.due_at,
                    &request.schedule_anchor_at,
                    &request.freshness_window_days,
                    &request.status.as_str(),
                ],
            )
            .await?;

        evidence_request_from_row(row)
    }

    pub async fn replace_evidence_request(
        &self,
        id: EvidenceRequestId,
        update: &UpdateEvidenceRequestPayload,
    ) -> Result<Option<EvidenceRequest>, Error> {
        let rows = self
            .transaction
            .query(
                r#"
UPDATE evidence_requests
SET
    title = $2,
    description = $3,
    collection_instructions = $4,
    cadence = $5,
    due_at = $6,
    schedule_anchor_at = $7,
    freshness_window_days = $8,
    status = $9,
    updated_at = now()
WHERE id = $1
  AND workspace_id = $10
RETURNING
    id,
    workspace_id,
    title,
    description,
    collection_instructions,
    cadence,
    due_at,
    schedule_anchor_at,
    freshness_window_days,
    status,
    created_at,
    updated_at
"#,
                &[
                    &Uuid::from(id),
                    &update.title,
                    &update.description,
                    &update.collection_instructions,
                    &update.cadence.as_str(),
                    &update.due_at,
                    &update.schedule_anchor_at,
                    &update.freshness_window_days,
                    &update.status.as_str(),
                    &Uuid::from(self.workspace_id),
                ],
            )
            .await?;

        rows.into_iter()
            .next()
            .map(evidence_request_from_row)
            .transpose()
    }
}

impl ActorReadContext {
    pub async fn get_evidence_request(
        &self,
        id: EvidenceRequestId,
    ) -> Result<Option<EvidenceRequest>, Error> {
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
    cadence,
    due_at,
    schedule_anchor_at,
    freshness_window_days,
    status,
    created_at,
    updated_at
FROM evidence_requests
WHERE id = $1
  AND workspace_id = $2
"#,
                &[&Uuid::from(id), &Uuid::from(self.workspace_id)],
            )
            .await?;

        rows.into_iter()
            .next()
            .map(evidence_request_from_row)
            .transpose()
    }

    pub async fn list_evidence_requests(&self) -> Result<Vec<EvidenceRequest>, Error> {
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
    cadence,
    due_at,
    schedule_anchor_at,
    freshness_window_days,
    status,
    created_at,
    updated_at
FROM evidence_requests
WHERE workspace_id = $1
ORDER BY due_at, title
"#,
                &[&Uuid::from(self.workspace_id)],
            )
            .await?;

        rows.into_iter().map(evidence_request_from_row).collect()
    }

    pub async fn list_due_evidence_requests(
        &self,
        now: DateTime<Utc>,
    ) -> Result<Vec<EvidenceRequest>, Error> {
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
    cadence,
    due_at,
    schedule_anchor_at,
    freshness_window_days,
    status,
    created_at,
    updated_at
FROM evidence_requests
WHERE status = 'active'
  AND due_at <= $1
  AND workspace_id = $2
ORDER BY due_at, title
"#,
                &[&now, &Uuid::from(self.workspace_id)],
            )
            .await?;

        rows.into_iter().map(evidence_request_from_row).collect()
    }
}

fn evidence_request_from_row(row: Row) -> Result<EvidenceRequest, Error> {
    let cadence = row
        .try_get::<_, String>("cadence")?
        .parse::<EvidenceRequestCadence>()?;
    let status = row
        .try_get::<_, String>("status")?
        .parse::<EvidenceRequestStatus>()?;
    let workspace_id = WorkspaceId::from(row.try_get::<_, Uuid>("workspace_id")?);

    Ok(EvidenceRequest {
        id: EvidenceRequestId::from(row.try_get::<_, Uuid>("id")?),
        workspace_id,
        title: row.try_get("title")?,
        description: row.try_get("description")?,
        collection_instructions: row.try_get("collection_instructions")?,
        cadence,
        due_at: row.try_get("due_at")?,
        schedule_anchor_at: row.try_get("schedule_anchor_at")?,
        freshness_window_days: row.try_get("freshness_window_days")?,
        status,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}
