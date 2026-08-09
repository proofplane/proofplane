use chrono::{DateTime, Utc};
use tokio_postgres::Row;
use uuid::Uuid;

use crate::{
    domain::{
        ControlId, CreateEvidencePayload, Evidence, EvidenceAggregate, EvidenceControlMappingState,
        EvidenceDefinition, EvidenceId, EvidenceStatus, UpdateEvidencePayload, WorkspaceId,
    },
    repository::{WorkspaceReadContext, WorkspaceTransactionContext},
};

use super::{
    snapshot::{save_workspace_snapshot, workspace_snapshot_record},
    Error,
};

/// Transaction-scoped complete-snapshot repository for the evidence aggregate.
pub struct EvidenceRepository<'a> {
    context: &'a WorkspaceTransactionContext<'a>,
}

impl<'a> WorkspaceTransactionContext<'a> {
    pub fn evidence(&'a self) -> EvidenceRepository<'a> {
        EvidenceRepository { context: self }
    }
}

impl EvidenceRepository<'_> {
    pub async fn get(&self, id: EvidenceId) -> Result<Option<EvidenceAggregate>, Error> {
        let Some(row) = self.context.transaction.query_opt(
            "SELECT id, workspace_id, title, description, collection_instructions, status, created_at, updated_at FROM evidence WHERE id = $1 AND workspace_id = $2 FOR UPDATE",
            &[&Uuid::from(id), &Uuid::from(self.context.workspace_id)],
        ).await? else { return Ok(None); };
        let record = EvidenceRecord::try_from(row)?;
        let mappings = self.context.transaction.query(
            "SELECT control_id, rationale, created_at FROM evidence_control_mappings WHERE evidence_id = $1 ORDER BY control_id",
            &[&Uuid::from(id)],
        ).await?.into_iter().map(mapping_from_row).collect::<Result<Vec<_>, _>>()?;
        record.into_aggregate(mappings).map(Some)
    }

    /// Persists the aggregate's complete definition, status, and mapping snapshot.
    pub async fn save(&self, evidence: &EvidenceAggregate) -> Result<(), Error> {
        if evidence.workspace_id() != self.context.workspace_id {
            return Err(Error::InvariantViolation(
                "evidence workspace must match its transaction",
            ));
        }
        let record = EvidenceRecord::from(evidence);
        save_workspace_snapshot(&self.context.transaction, record.as_workspace_snapshot()).await?;
        self.context
            .transaction
            .execute(
                "DELETE FROM evidence_control_mappings WHERE evidence_id = $1",
                &[&Uuid::from(evidence.id())],
            )
            .await?;
        for mapping in evidence.mappings() {
            self.context.transaction.execute(
                "INSERT INTO evidence_control_mappings (evidence_id, control_id, rationale, created_at) VALUES ($1, $2, $3, $4)",
                &[&Uuid::from(evidence.id()), &Uuid::from(mapping.control_id()), &mapping.rationale(), &mapping.created_at()],
            ).await?;
        }
        Ok(())
    }
}

impl WorkspaceTransactionContext<'_> {
    pub async fn controls_exist(&self, ids: &[ControlId]) -> Result<bool, Error> {
        if ids.is_empty() {
            return Ok(true);
        }
        let requested = ids
            .iter()
            .copied()
            .map(Uuid::from)
            .collect::<std::collections::HashSet<_>>();
        let row = self
            .transaction
            .query_one(
                "SELECT count(DISTINCT id) FROM controls WHERE workspace_id = $1 AND id = ANY($2)",
                &[
                    &Uuid::from(self.workspace_id),
                    &requested.iter().copied().collect::<Vec<_>>(),
                ],
            )
            .await?;
        Ok(row.try_get::<_, i64>(0)? == requested.len() as i64)
    }
}

workspace_snapshot_record! {
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
    scope: workspace_id,
}

impl TryFrom<Row> for EvidenceRecord {
    type Error = Error;
    fn try_from(row: Row) -> Result<Self, Self::Error> {
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
    fn into_aggregate(
        self,
        mappings: Vec<EvidenceControlMappingState>,
    ) -> Result<EvidenceAggregate, Error> {
        let definition =
            EvidenceDefinition::new(self.title, self.description, self.collection_instructions)
                .into_result()
                .map_err(|_| {
                    Error::InvariantViolation("persisted evidence definition is invalid")
                })?;
        let status = self.status.parse::<EvidenceStatus>()?;
        EvidenceAggregate::rehydrate(
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

impl From<&EvidenceAggregate> for EvidenceRecord {
    fn from(evidence: &EvidenceAggregate) -> Self {
        Self {
            id: evidence.id().into(),
            workspace_id: evidence.workspace_id().into(),
            title: evidence.title().to_owned(),
            description: evidence.description().to_owned(),
            collection_instructions: evidence.collection_instructions().to_owned(),
            status: evidence.status().as_str().to_owned(),
            created_at: evidence.created_at(),
            updated_at: evidence.updated_at(),
        }
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

impl WorkspaceTransactionContext<'_> {
    pub async fn get_evidence(&self, id: EvidenceId) -> Result<Option<Evidence>, Error> {
        let rows = self
            .transaction
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
FOR KEY SHARE
"#,
                &[&Uuid::from(id), &Uuid::from(self.workspace_id)],
            )
            .await?;

        rows.into_iter().next().map(evidence_from_row).transpose()
    }

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

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};
    use uuid::Uuid;

    use crate::{
        domain::{
            ControlId, EvidenceAggregate, EvidenceControlMappingState, EvidenceDefinition,
            EvidenceId, EvidenceStatus,
        },
        repository::test_support,
    };

    #[tokio::test]
    async fn snapshot_save_rehydrates_complete_mapping_state_and_preserves_mapping_audit_time() {
        let postgres = test_support::database().await;
        let workspace = test_support::workspace(&postgres, "evidence snapshot").await;
        let control_id = Uuid::new_v4();
        let client = postgres.get().await.unwrap();
        client
            .execute(
                "INSERT INTO controls (id, workspace_id, code, title, description) VALUES ($1, $2, 'C1', 'Control', 'Description')",
                &[&control_id, &Uuid::from(workspace.workspace_id)],
            )
            .await
            .unwrap();
        let created_at = Utc.with_ymd_and_hms(2026, 8, 9, 12, 0, 0).unwrap();
        let evidence_id = EvidenceId::from(Uuid::new_v4());
        let mut aggregate = EvidenceAggregate::define(
            evidence_id,
            workspace.workspace_id,
            EvidenceDefinition::new("Evidence".into(), "Description".into(), "Collect".into())
                .into_result()
                .unwrap(),
            EvidenceStatus::Paused,
            created_at,
        );
        aggregate
            .replace_mappings(vec![EvidenceControlMappingState::new(
                ControlId::from(control_id),
                "Rationale".into(),
                created_at,
            )
            .into_result()
            .unwrap()])
            .unwrap();

        postgres
            .in_agent_connection_workspace_context(
                workspace.workspace_id,
                workspace.user_id,
                workspace.agent_connection_id,
                async |context| {
                    context.evidence().save(&aggregate).await?;
                    let rehydrated = context.evidence().get(evidence_id).await?.unwrap();
                    assert_eq!(rehydrated, aggregate);
                    Ok(())
                },
            )
            .await
            .unwrap();
    }
}
