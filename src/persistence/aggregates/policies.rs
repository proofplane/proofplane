use chrono::{DateTime, Utc};
use tokio_postgres::Row;
use uuid::Uuid;

use crate::{
    domain::{Document, Policy, PolicyControlMappingState, PolicyDefinition, PolicyId},
    persistence::WorkspaceUnitOfWork,
};

use super::Error;

use super::snapshot::{save_workspace_snapshot, workspace_snapshot_record};

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
        let record = PolicyRecord::try_from(row)?;
        let mappings = self.workspace.transaction.query("SELECT control_id, created_at FROM policy_control_mappings WHERE policy_id = $1 ORDER BY control_id", &[&Uuid::from(id)]).await?.into_iter().map(policy_mapping_from_row).collect::<Result<Vec<_>, _>>()?;
        record.into_aggregate(mappings).map(Some)
    }

    /// Persists the aggregate's complete definition, archive lifecycle, and mapping snapshot.
    pub async fn save(&self, policy: &Policy) -> Result<(), Error> {
        if policy.workspace_id() != self.workspace.workspace_id {
            return Err(Error::InvariantViolation(
                "policy workspace must match its repository scope",
            ));
        }
        let record = PolicyRecord::from(policy);
        save_workspace_snapshot(self.workspace.transaction, record.as_workspace_snapshot()).await?;
        self.workspace
            .transaction
            .execute(
                "DELETE FROM policy_control_mappings WHERE policy_id = $1",
                &[&Uuid::from(policy.id())],
            )
            .await?;
        for mapping in policy.mappings() {
            self.workspace.transaction.execute("INSERT INTO policy_control_mappings (policy_id, control_id, created_at) VALUES ($1, $2, $3)", &[&Uuid::from(policy.id()), &Uuid::from(mapping.control_id()), &mapping.created_at()]).await?;
        }
        Ok(())
    }
}

workspace_snapshot_record! {
    struct PolicyRecord { id: Uuid, workspace_id: Uuid, name: String, description: Option<String>, created_at: DateTime<Utc>, updated_at: DateTime<Utc>, archived_at: Option<DateTime<Utc>>, }
    table: policies,
    conflict: id,
    scope: workspace_id,
}
impl TryFrom<Row> for PolicyRecord {
    type Error = Error;
    fn try_from(row: Row) -> Result<Self, Self::Error> {
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
    fn into_aggregate(self, mappings: Vec<PolicyControlMappingState>) -> Result<Policy, Error> {
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
impl From<&Policy> for PolicyRecord {
    fn from(policy: &Policy) -> Self {
        Self {
            id: policy.id().into(),
            workspace_id: policy.workspace_id().into(),
            name: policy.name().to_owned(),
            description: policy.description().map(str::to_owned),
            created_at: policy.created_at(),
            updated_at: policy.updated_at(),
            archived_at: policy.archived_at(),
        }
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
pub enum CreatePolicyDocumentResult {
    Created(Document),
    PolicyNotFound,
    DocumentExists,
}
