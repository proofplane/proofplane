use uuid::Uuid;

use crate::{
    domain::{EvidenceId, EvidenceStatus, WorkspaceId},
    persistence::Error,
    read_models::EvidenceDetail,
};

use super::{param, ReadExecutor};

pub(crate) struct EvidenceReads<'a, E> {
    executor: &'a E,
    workspace_id: WorkspaceId,
}
impl<'a, E> EvidenceReads<'a, E> {
    pub(crate) fn new(executor: &'a E, workspace_id: WorkspaceId) -> Self {
        Self {
            executor,
            workspace_id,
        }
    }
}

impl<E: ReadExecutor> EvidenceReads<'_, E> {
    pub async fn get(&self, id: EvidenceId) -> Result<Option<EvidenceDetail>, Error> {
        self.executor
            .query_opt(
                &format!("{EVIDENCE_SELECT} WHERE id = $1 AND workspace_id = $2"),
                &[
                    param(&Uuid::from(id)),
                    param(&Uuid::from(self.workspace_id)),
                ],
            )
            .await?
            .map(evidence_from_row)
            .transpose()
    }
    pub async fn list(&self) -> Result<Vec<EvidenceDetail>, Error> {
        self.executor
            .query(
                &format!("{EVIDENCE_SELECT} WHERE workspace_id = $1 ORDER BY title"),
                &[param(&Uuid::from(self.workspace_id))],
            )
            .await?
            .into_iter()
            .map(evidence_from_row)
            .collect()
    }
}

const EVIDENCE_SELECT: &str = "SELECT id, workspace_id, title, description, collection_instructions, status, created_at, updated_at FROM evidence";
fn evidence_from_row(row: tokio_postgres::Row) -> Result<EvidenceDetail, Error> {
    Ok(EvidenceDetail {
        id: EvidenceId::from(row.try_get::<_, Uuid>("id")?),
        workspace_id: WorkspaceId::from(row.try_get::<_, Uuid>("workspace_id")?),
        title: row.try_get("title")?,
        description: row.try_get("description")?,
        collection_instructions: row.try_get("collection_instructions")?,
        status: row
            .try_get::<_, String>("status")?
            .parse::<EvidenceStatus>()?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}
