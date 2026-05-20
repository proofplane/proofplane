use async_trait::async_trait;
use chrono::{DateTime, Utc};
use deadpool_postgres::{Object, Pool, Transaction};
use tokio_postgres::Row;
use uuid::Uuid;

use crate::domain::{
    EvidenceRequest, EvidenceRequestCadence, EvidenceRequestId, EvidenceRequestStatus,
    EvidenceRequestUpdate, NewEvidenceRequest, WorkspaceId,
};

pub mod error;

pub use error::Error;

#[async_trait]
pub trait EvidenceRequestRepository {
    async fn create_evidence_request(
        &self,
        request: &NewEvidenceRequest,
    ) -> Result<EvidenceRequest, Error>;

    async fn get_evidence_request(
        &self,
        id: EvidenceRequestId,
    ) -> Result<Option<EvidenceRequest>, Error>;

    async fn list_evidence_requests_by_workspace(
        &self,
        workspace_id: &WorkspaceId,
    ) -> Result<Vec<EvidenceRequest>, Error>;

    async fn replace_evidence_request(
        &self,
        id: EvidenceRequestId,
        update: &EvidenceRequestUpdate,
    ) -> Result<Option<EvidenceRequest>, Error>;

    async fn list_due_evidence_requests(
        &self,
        now: DateTime<Utc>,
    ) -> Result<Vec<EvidenceRequest>, Error>;
}

pub struct Client {
    client: Object,
}

impl Client {
    pub async fn txn(&mut self) -> Result<Transaction<'_>, Error> {
        Ok(self.client.transaction().await?)
    }
}

pub struct Postgres {
    pool: Pool,
}

impl Postgres {
    pub fn new(pool: Pool) -> Self {
        Self { pool }
    }

    pub async fn get(&self) -> Result<Object, deadpool_postgres::PoolError> {
        self.pool.get().await
    }

    pub async fn get_client(&self) -> Result<Client, Error> {
        let client = self.pool.get().await?;

        Ok(Client { client })
    }
}

#[async_trait]
impl EvidenceRequestRepository for Postgres {
    async fn create_evidence_request(
        &self,
        request: &NewEvidenceRequest,
    ) -> Result<EvidenceRequest, Error> {
        let client = self.get().await?;
        let row = client
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
                    &Uuid::from(request.workspace_id),
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

    async fn get_evidence_request(
        &self,
        id: EvidenceRequestId,
    ) -> Result<Option<EvidenceRequest>, Error> {
        let client = self.get().await?;
        let rows = client
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
"#,
                &[&Uuid::from(id)],
            )
            .await?;

        rows.into_iter()
            .next()
            .map(evidence_request_from_row)
            .transpose()
    }

    async fn list_evidence_requests_by_workspace(
        &self,
        workspace_id: &WorkspaceId,
    ) -> Result<Vec<EvidenceRequest>, Error> {
        let client = self.get().await?;
        let rows = client
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
                &[&Uuid::from(*workspace_id)],
            )
            .await?;

        rows.into_iter().map(evidence_request_from_row).collect()
    }

    async fn replace_evidence_request(
        &self,
        id: EvidenceRequestId,
        update: &EvidenceRequestUpdate,
    ) -> Result<Option<EvidenceRequest>, Error> {
        let client = self.get().await?;
        let rows = client
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
                ],
            )
            .await?;

        rows.into_iter()
            .next()
            .map(evidence_request_from_row)
            .transpose()
    }

    async fn list_due_evidence_requests(
        &self,
        now: DateTime<Utc>,
    ) -> Result<Vec<EvidenceRequest>, Error> {
        let client = self.get().await?;
        let rows = client
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
ORDER BY due_at, title
"#,
                &[&now],
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
