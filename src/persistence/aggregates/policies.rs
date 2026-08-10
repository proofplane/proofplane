use chrono::{DateTime, Utc};
use tokio_postgres::Row;
use uuid::Uuid;

use crate::{
    domain::{Document, Policy, PolicyControlMappingState, PolicyDefinition, PolicyId},
    persistence::WorkspaceUnitOfWork,
};

use super::Error;

use super::snapshot::{save_snapshot, snapshot_record};

/// Workspace-scoped complete-snapshot repository for the policy aggregate.
pub struct PolicyRepository<'a> {
    workspace: &'a WorkspaceUnitOfWork<'a>,
}

impl<'a> WorkspaceUnitOfWork<'a> {
    pub fn policies(&'a self) -> PolicyRepository<'a> {
        PolicyRepository { workspace: self }
    }
}

impl PolicyRepository<'_> {
    pub async fn get(&self, id: PolicyId) -> Result<Option<Policy>, Error> {
        let Some(row) = self.workspace.transaction.query_opt("SELECT id, workspace_id, name, description, created_at, updated_at, archived_at FROM policies WHERE id = $1 AND workspace_id = $2 FOR UPDATE", &[&Uuid::from(id), &Uuid::from(self.workspace.workspace_id)]).await? else { return Ok(None) };
        let record = PolicyRecord::try_from_row(&row)?;
        let mappings = self.workspace.transaction.query("SELECT control_id, created_at FROM policy_control_mappings WHERE policy_id = $1 ORDER BY control_id", &[&Uuid::from(id)]).await?.into_iter().map(policy_mapping_from_row).collect::<Result<Vec<_>, _>>()?;
        record.into_domain(mappings).map(Some)
    }

    /// Persists the aggregate's complete definition, archive lifecycle, and mapping snapshot.
    pub async fn save(&self, policy: &Policy) -> Result<(), Error> {
        let record = PolicyRecord::from_domain(policy)?;
        save_snapshot(self.workspace.transaction, record.as_snapshot()).await?;
        self.workspace
            .transaction
            .execute(
                "DELETE FROM policy_control_mappings WHERE policy_id = $1",
                &[&Uuid::from(policy.id())],
            )
            .await?;
        for mapping in policy
            .mappings()
            .iter()
            .map(|mapping| PolicyControlMappingRecord::from_domain(record.id, mapping))
        {
            self.workspace.transaction.execute("INSERT INTO policy_control_mappings (policy_id, control_id, created_at) VALUES ($1, $2, $3)", &[&mapping.policy_id, &mapping.control_id, &mapping.created_at]).await?;
        }
        Ok(())
    }
}

struct PolicyControlMappingRecord {
    policy_id: Uuid,
    control_id: Uuid,
    created_at: DateTime<Utc>,
}

impl PolicyControlMappingRecord {
    fn from_domain(policy_id: Uuid, mapping: &PolicyControlMappingState) -> Self {
        Self {
            policy_id,
            control_id: mapping.control_id().into(),
            created_at: mapping.created_at(),
        }
    }
}

snapshot_record! {
    struct PolicyRecord { id: Uuid, workspace_id: Uuid, name: String, description: Option<String>, created_at: DateTime<Utc>, updated_at: DateTime<Utc>, archived_at: Option<DateTime<Utc>>, }
    table: policies,
    conflict: id,
}
impl PolicyRecord {
    fn try_from_row(row: &Row) -> Result<Self, Error> {
        Ok(Self {
            id: row.try_get("id")?,
            workspace_id: row.try_get("workspace_id")?,
            name: row.try_get("name")?,
            description: row.try_get("description")?,
            created_at: row.try_get("created_at")?,
            updated_at: row.try_get("updated_at")?,
            archived_at: row.try_get("archived_at")?,
        })
    }
}
impl PolicyRecord {
    fn from_domain(policy: &Policy) -> Result<Self, Error> {
        Ok(Self {
            id: policy.id().into(),
            workspace_id: policy.workspace_id().into(),
            name: policy.name().to_owned(),
            description: policy.description().map(str::to_owned),
            created_at: policy.created_at(),
            updated_at: policy.updated_at(),
            archived_at: policy.archived_at(),
        })
    }

    fn into_domain(self, mappings: Vec<PolicyControlMappingState>) -> Result<Policy, Error> {
        let definition = PolicyDefinition::new(self.name, self.description)
            .into_result()
            .map_err(|_| Error::InvariantViolation("persisted policy definition is invalid"))?;
        Policy::rehydrate(
            self.id.into(),
            self.workspace_id.into(),
            definition,
            mappings,
            self.created_at,
            self.updated_at,
            self.archived_at,
        )
        .map_err(|_| Error::InvariantViolation("persisted policy snapshot is inconsistent"))
    }
}
fn policy_mapping_from_row(row: Row) -> Result<PolicyControlMappingState, Error> {
    Ok(PolicyControlMappingState::new(
        row.try_get::<_, Uuid>("control_id")?.into(),
        row.try_get("created_at")?,
    ))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArchivePolicyResult {
    Archived {
        policy_id: PolicyId,
        archived_at: DateTime<Utc>,
    },
    NotFound,
    DocumentInProgress,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(clippy::large_enum_variant)]
pub enum CreatePolicyDocumentResult {
    Created(Document),
    PolicyNotFound,
    DocumentExists,
}
