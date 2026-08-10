use chrono::{DateTime, Utc};
use tokio_postgres::Row;
use uuid::Uuid;

use crate::{
    domain::{
        ControlId, Evidence, EvidenceControlMappingState, EvidenceDefinition, EvidenceId,
        EvidenceStatus,
    },
    persistence::WorkspaceUnitOfWork,
};

use super::{
    snapshot::{save_snapshot, snapshot_record},
    Error,
};

/// Workspace-scoped complete-snapshot repository for the evidence aggregate.
pub struct EvidenceRepository<'a> {
    workspace: &'a WorkspaceUnitOfWork<'a>,
}

impl<'a> WorkspaceUnitOfWork<'a> {
    pub fn evidence(&'a self) -> EvidenceRepository<'a> {
        EvidenceRepository { workspace: self }
    }
}

impl EvidenceRepository<'_> {
    pub async fn get(&self, id: EvidenceId) -> Result<Option<Evidence>, Error> {
        let Some(row) = self.workspace.transaction.query_opt(
            "SELECT id, workspace_id, title, description, collection_instructions, status, created_at, updated_at FROM evidence WHERE id = $1 AND workspace_id = $2 FOR UPDATE",
            &[&Uuid::from(id), &Uuid::from(self.workspace.workspace_id)],
        ).await? else { return Ok(None); };
        let record = EvidenceRecord::try_from_row(&row)?;
        let mappings = self.workspace.transaction.query(
            "SELECT control_id, rationale, created_at FROM evidence_control_mappings WHERE evidence_id = $1 ORDER BY control_id",
            &[&Uuid::from(id)],
        ).await?.into_iter().map(mapping_from_row).collect::<Result<Vec<_>, _>>()?;
        record.into_domain(mappings).map(Some)
    }

    /// Persists the aggregate's complete definition, status, and mapping snapshot.
    pub async fn save(&self, evidence: &Evidence) -> Result<(), Error> {
        let record = EvidenceRecord::from_domain(evidence)?;
        save_snapshot(self.workspace.transaction, record.as_snapshot()).await?;
        self.workspace
            .transaction
            .execute(
                "DELETE FROM evidence_control_mappings WHERE evidence_id = $1",
                &[&Uuid::from(evidence.id())],
            )
            .await?;
        for mapping in evidence
            .mappings()
            .iter()
            .map(|mapping| EvidenceControlMappingRecord::from_domain(record.id, mapping))
        {
            self.workspace.transaction.execute(
                "INSERT INTO evidence_control_mappings (evidence_id, control_id, rationale, created_at) VALUES ($1, $2, $3, $4)",
                &[&mapping.evidence_id, &mapping.control_id, &mapping.rationale, &mapping.created_at],
            ).await?;
        }
        Ok(())
    }
}

struct EvidenceControlMappingRecord {
    evidence_id: Uuid,
    control_id: Uuid,
    rationale: String,
    created_at: DateTime<Utc>,
}

impl EvidenceControlMappingRecord {
    fn from_domain(evidence_id: Uuid, mapping: &EvidenceControlMappingState) -> Self {
        Self {
            evidence_id,
            control_id: mapping.control_id().into(),
            rationale: mapping.rationale().to_owned(),
            created_at: mapping.created_at(),
        }
    }
}

snapshot_record! {
    struct EvidenceRecord {
        id: Uuid,
        workspace_id: Uuid,
        title: String,
        description: String,
        collection_instructions: String,
        status: String,
        created_at: DateTime<Utc>,
        updated_at: DateTime<Utc>,
    }
    table: evidence,
    conflict: id,
}

impl EvidenceRecord {
    fn try_from_row(row: &Row) -> Result<Self, Error> {
        Ok(Self {
            id: row.try_get("id")?,
            workspace_id: row.try_get("workspace_id")?,
            title: row.try_get("title")?,
            description: row.try_get("description")?,
            collection_instructions: row.try_get("collection_instructions")?,
            status: row.try_get("status")?,
            created_at: row.try_get("created_at")?,
            updated_at: row.try_get("updated_at")?,
        })
    }
}

impl EvidenceRecord {
    fn from_domain(evidence: &Evidence) -> Result<Self, Error> {
        Ok(Self {
            id: evidence.id().into(),
            workspace_id: evidence.workspace_id().into(),
            title: evidence.title().to_owned(),
            description: evidence.description().to_owned(),
            collection_instructions: evidence.collection_instructions().to_owned(),
            status: evidence.status().as_str().to_owned(),
            created_at: evidence.created_at(),
            updated_at: evidence.updated_at(),
        })
    }

    fn into_domain(self, mappings: Vec<EvidenceControlMappingState>) -> Result<Evidence, Error> {
        let definition =
            EvidenceDefinition::new(self.title, self.description, self.collection_instructions)
                .into_result()
                .map_err(|_| {
                    Error::InvariantViolation("persisted evidence definition is invalid")
                })?;
        let status = self.status.parse::<EvidenceStatus>()?;
        Evidence::rehydrate(
            self.id.into(),
            self.workspace_id.into(),
            definition,
            status,
            mappings,
            self.created_at,
            self.updated_at,
        )
        .map_err(|_| Error::InvariantViolation("persisted evidence snapshot is inconsistent"))
    }
}

fn mapping_from_row(row: Row) -> Result<EvidenceControlMappingState, Error> {
    EvidenceControlMappingState::new(
        ControlId::from(row.try_get::<_, Uuid>("control_id")?),
        row.try_get("rationale")?,
        row.try_get("created_at")?,
    )
    .into_result()
    .map_err(|_| Error::InvariantViolation("persisted evidence mapping is invalid"))
}
