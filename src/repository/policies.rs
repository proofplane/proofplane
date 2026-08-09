use std::str::FromStr;

use chrono::{DateTime, Utc};
use tokio_postgres::Row;
use uuid::Uuid;

use crate::{
    domain::{
        ControlId, Document, DocumentId, DocumentIdentity, DocumentUploadStatus, Policy,
        PolicyControlMappingState, PolicyDefinition, PolicyId, WorkspaceId,
    },
    projections::{
        ControlSummary, PolicyCatalogEntry, PolicyControlMapping, PolicyDetail,
        PolicyDocumentDetail, PolicyDocumentStatus,
    },
    repository::{
        DocumentDownloadCandidate, TransactionContext, WorkspaceReadContext,
        WorkspaceTransactionContext,
    },
};

impl TransactionContext<'_> {
    pub async fn policy_is_active(
        &self,
        workspace_id: WorkspaceId,
        policy_id: PolicyId,
    ) -> Result<bool, Error> {
        self.transaction
            .query_opt(
                r#"
SELECT id
FROM policies
WHERE id = $1
  AND workspace_id = $2
  AND archived_at IS NULL
FOR KEY SHARE
"#,
                &[&Uuid::from(policy_id), &Uuid::from(workspace_id)],
            )
            .await
            .map(|row| row.is_some())
            .map_err(Into::into)
    }
}

use super::{documents::document_from_row, Error};

use super::snapshot::{save_workspace_snapshot, workspace_snapshot_record};

/// Transaction-scoped complete-snapshot repository for the policy aggregate.
pub struct PolicyRepository<'a> {
    context: &'a WorkspaceTransactionContext<'a>,
}

impl<'a> WorkspaceTransactionContext<'a> {
    pub fn policies(&'a self) -> PolicyRepository<'a> {
        PolicyRepository { context: self }
    }
}

impl PolicyRepository<'_> {
    pub async fn get(&self, id: PolicyId) -> Result<Option<Policy>, Error> {
        let Some(row) = self.context.transaction.query_opt("SELECT id, workspace_id, name, description, created_at, updated_at, archived_at FROM policies WHERE id = $1 AND workspace_id = $2 FOR UPDATE", &[&Uuid::from(id), &Uuid::from(self.context.workspace_id)]).await? else { return Ok(None) };
        let record = PolicyRecord::try_from(row)?;
        let mappings = self.context.transaction.query("SELECT control_id, created_at FROM policy_control_mappings WHERE policy_id = $1 ORDER BY control_id", &[&Uuid::from(id)]).await?.into_iter().map(policy_mapping_from_row).collect::<Result<Vec<_>, _>>()?;
        record.into_aggregate(mappings).map(Some)
    }

    /// Persists the aggregate's complete definition, archive lifecycle, and mapping snapshot.
    pub async fn save(&self, policy: &Policy) -> Result<(), Error> {
        if policy.workspace_id() != self.context.workspace_id {
            return Err(Error::InvariantViolation(
                "policy workspace must match its transaction",
            ));
        }
        let record = PolicyRecord::from(policy);
        save_workspace_snapshot(&self.context.transaction, record.as_workspace_snapshot()).await?;
        self.context
            .transaction
            .execute(
                "DELETE FROM policy_control_mappings WHERE policy_id = $1",
                &[&Uuid::from(policy.id())],
            )
            .await?;
        for mapping in policy.mappings() {
            self.context.transaction.execute("INSERT INTO policy_control_mappings (policy_id, control_id, created_at) VALUES ($1, $2, $3)", &[&Uuid::from(policy.id()), &Uuid::from(mapping.control_id()), &mapping.created_at()]).await?;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyDocumentUploadEligibility {
    Eligible,
    CurrentDocument,
}

impl WorkspaceTransactionContext<'_> {
    pub async fn policy_document_in_progress(&self, policy_id: PolicyId) -> Result<bool, Error> {
        Ok(self.transaction.query_one("SELECT EXISTS (SELECT 1 FROM documents WHERE owner_type = 'policy' AND owner_id = $1 AND workspace_id = $2 AND archived = false AND upload_status IN ('pending', 'finalizing'))", &[&Uuid::from(policy_id), &Uuid::from(self.workspace_id)]).await?.try_get(0)?)
    }

    pub async fn lock_policy_document_upload_eligibility(
        &self,
        policy_id: PolicyId,
    ) -> Result<Option<PolicyDocumentUploadEligibility>, Error> {
        let policy = self
            .transaction
            .query_opt(
                r#"
SELECT id
FROM policies p
WHERE p.id = $1
  AND p.workspace_id = $2
  AND p.archived_at IS NULL
FOR UPDATE OF p
"#,
                &[&Uuid::from(policy_id), &Uuid::from(self.workspace_id)],
            )
            .await?;
        if policy.is_none() {
            return Ok(None);
        }
        let current_document = self
            .transaction
            .query_one(
                r#"SELECT EXISTS (
    SELECT 1 FROM documents d
    WHERE d.owner_type = 'policy'
      AND d.owner_id = $1
      AND d.workspace_id = $2
      AND d.archived = false
) AS current_document"#,
                &[&Uuid::from(policy_id), &Uuid::from(self.workspace_id)],
            )
            .await?
            .try_get::<_, bool>("current_document")?;
        Ok(Some(if current_document {
            PolicyDocumentUploadEligibility::CurrentDocument
        } else {
            PolicyDocumentUploadEligibility::Eligible
        }))
    }

    pub(super) async fn load_policy_detail(
        &self,
        policy_id: PolicyId,
    ) -> Result<Option<PolicyDetail>, Error> {
        let rows = self
            .transaction
            .query(
                POLICY_READ_DETAIL_QUERY,
                &[&Uuid::from(policy_id), &Uuid::from(self.workspace_id)],
            )
            .await?;
        policy_detail_from_joined_rows(rows)
    }

    pub(super) async fn load_policy_control_mapping(
        &self,
        policy_id: PolicyId,
        control_id: ControlId,
    ) -> Result<Option<PolicyControlMapping>, Error> {
        self.transaction
            .query_opt(
                r#"
SELECT
    m.policy_id,
    c.id AS control_id,
    c.code AS control_code,
    c.title AS control_title,
    c.description AS control_description,
    m.created_at AS mapping_created_at
FROM policy_control_mappings m
JOIN policies p ON p.id = m.policy_id
JOIN controls c ON c.id = m.control_id
WHERE m.policy_id = $1
  AND m.control_id = $2
  AND p.workspace_id = $3
  AND c.workspace_id = $3
  AND p.archived_at IS NULL
"#,
                &[
                    &Uuid::from(policy_id),
                    &Uuid::from(control_id),
                    &Uuid::from(self.workspace_id),
                ],
            )
            .await?
            .map(policy_control_mapping_from_row)
            .transpose()
    }
}

impl WorkspaceReadContext {
    pub(crate) async fn get_policy_document_for_agent_upload(
        &self,
        policy_id: PolicyId,
        document_id: DocumentId,
    ) -> Result<Option<Document>, Error> {
        self.client
            .query_opt(
                r#"
SELECT d.*, d.owner_id AS policy_id
FROM documents d
JOIN policies p ON p.id = d.owner_id
WHERE p.id = $1
  AND p.workspace_id = $2
  AND d.owner_type = 'policy'
  AND d.workspace_id = $2
  AND d.id = $3
"#,
                &[
                    &Uuid::from(policy_id),
                    &Uuid::from(self.workspace_id),
                    &Uuid::from(document_id),
                ],
            )
            .await?
            .map(|row| policy_document_from_row(&row))
            .transpose()
    }

    pub async fn get_policy_document_for_download(
        &self,
        policy_id: PolicyId,
        document_id: DocumentId,
    ) -> Result<Option<DocumentDownloadCandidate>, Error> {
        self.client
            .query_opt(
                r#"
SELECT a.*, a.owner_id AS policy_id
FROM documents a
JOIN policies p ON p.id = a.owner_id
WHERE p.id = $1
  AND p.workspace_id = $2
  AND p.archived_at IS NULL
  AND a.owner_type = 'policy'
  AND a.workspace_id = $2
  AND a.id = $3
  AND a.archived = false
"#,
                &[
                    &Uuid::from(policy_id),
                    &Uuid::from(self.workspace_id),
                    &Uuid::from(document_id),
                ],
            )
            .await?
            .map(|row| {
                Ok(DocumentDownloadCandidate {
                    workspace_id: self.workspace_id,
                    document: policy_document_from_row(&row)?,
                })
            })
            .transpose()
    }

    pub async fn get_current_policy_document(
        &self,
        policy_id: PolicyId,
    ) -> Result<Option<Document>, Error> {
        self.client
            .query_opt(
                r#"
SELECT a.*, a.owner_id AS policy_id
FROM documents a
JOIN policies p ON p.id = a.owner_id
WHERE p.id = $1
  AND p.workspace_id = $2
  AND p.archived_at IS NULL
  AND a.owner_type = 'policy'
  AND a.workspace_id = $2
  AND a.archived = false
"#,
                &[&Uuid::from(policy_id), &Uuid::from(self.workspace_id)],
            )
            .await?
            .map(|row| policy_document_from_row(&row))
            .transpose()
    }

    pub(super) async fn load_policy_catalog(&self) -> Result<Vec<PolicyCatalogEntry>, Error> {
        let rows = self
            .client
            .query(POLICY_CATALOG_QUERY, &[&Uuid::from(self.workspace_id)])
            .await?;
        rows.into_iter()
            .map(policy_catalog_entry_from_row)
            .collect()
    }

    pub(super) async fn load_policy_detail(
        &self,
        policy_id: PolicyId,
    ) -> Result<Option<PolicyDetail>, Error> {
        let rows = self
            .client
            .query(
                POLICY_READ_DETAIL_QUERY,
                &[&Uuid::from(policy_id), &Uuid::from(self.workspace_id)],
            )
            .await?;
        policy_detail_from_joined_rows(rows)
    }
}

fn policy_document_from_row(row: &Row) -> Result<Document, Error> {
    let policy_id = PolicyId::from(row.try_get::<_, Uuid>("policy_id")?);
    let document_id = DocumentId::from(row.try_get::<_, Uuid>("id")?);
    document_from_row(
        row,
        DocumentIdentity::Policy {
            policy_id,
            document_id,
        },
    )
}

const POLICY_READ_DETAIL_QUERY: &str = r#"
SELECT
    p.id,
    p.id AS policy_id,
    p.workspace_id,
    p.name,
    p.description,
    p.created_at,
    p.updated_at,
    p.archived_at,
    a.id AS document_id,
    a.created_by_user_id AS document_created_by_user_id,
    a.filename AS document_filename,
    a.content_type AS document_content_type,
    a.content_length AS document_content_length,
    a.checksum_sha256 AS document_checksum_sha256,
    a.checksum_crc32c AS document_checksum_crc32c,
    a.upload_status AS document_upload_status,
    a.created_at AS document_created_at,
    c.id AS control_id,
    c.code AS control_code,
    c.title AS control_title,
    c.description AS control_description,
    m.created_at AS mapping_created_at
FROM policies p
LEFT JOIN documents a ON a.owner_id = p.id
    AND a.owner_type = 'policy'
    AND a.workspace_id = p.workspace_id
    AND a.archived = false
LEFT JOIN policy_control_mappings m ON m.policy_id = p.id
LEFT JOIN controls c ON c.id = m.control_id AND c.workspace_id = p.workspace_id
WHERE p.id = $1
  AND p.workspace_id = $2
  AND p.archived_at IS NULL
ORDER BY lower(c.code), c.id
"#;

const POLICY_CATALOG_QUERY: &str = r#"
SELECT
    p.id,
    p.name,
    p.description,
    count(m.control_id) AS mapped_control_count,
    a.upload_status AS document_upload_status
FROM policies p
LEFT JOIN policy_control_mappings m ON m.policy_id = p.id
LEFT JOIN documents a ON a.owner_id = p.id
    AND a.owner_type = 'policy'
    AND a.workspace_id = p.workspace_id
    AND a.archived = false
WHERE p.workspace_id = $1
  AND p.archived_at IS NULL
GROUP BY p.id, a.upload_status
ORDER BY lower(p.name), p.id
"#;

fn policies_from_joined_rows(rows: Vec<Row>) -> Result<Vec<PolicyDetail>, Error> {
    let mut policies = Vec::new();
    let mut current_policy_id = None;

    for row in rows {
        let policy_id = PolicyId::from(row.try_get::<_, Uuid>("id")?);
        if current_policy_id != Some(policy_id) {
            policies.push(policy_from_row(&row)?);
            current_policy_id = Some(policy_id);
        }

        if let Some(policy) = policies.last_mut() {
            if row.try_get::<_, Option<Uuid>>("control_id")?.is_some() {
                policy
                    .control_mappings
                    .push(policy_control_mapping_from_row(row)?);
            }
        }
    }

    Ok(policies)
}

fn policy_from_row(row: &Row) -> Result<PolicyDetail, Error> {
    Ok(PolicyDetail {
        id: PolicyId::from(row.try_get::<_, Uuid>("id")?),
        workspace_id: WorkspaceId::from(row.try_get::<_, Uuid>("workspace_id")?),
        name: row.try_get("name")?,
        description: row.try_get("description")?,
        control_mappings: Vec::new(),
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
        archived_at: row.try_get("archived_at")?,
        document: None,
    })
}

fn policy_detail_from_joined_rows(rows: Vec<Row>) -> Result<Option<PolicyDetail>, Error> {
    let document = rows
        .first()
        .map(policy_document_detail_from_row)
        .transpose()?
        .flatten();
    let policy = policies_from_joined_rows(rows)?.into_iter().next();

    Ok(policy.map(|mut policy| {
        policy.document = document;
        policy
    }))
}

fn policy_document_detail_from_row(row: &Row) -> Result<Option<PolicyDocumentDetail>, Error> {
    let Some(id) = row.try_get::<_, Option<Uuid>>("document_id")? else {
        return Ok(None);
    };
    let upload_status = row.try_get::<_, String>("document_upload_status")?;

    Ok(Some(PolicyDocumentDetail {
        id: DocumentId::from(id),
        created_by_user_id: row
            .try_get::<_, Uuid>("document_created_by_user_id")?
            .into(),
        filename: row.try_get("document_filename")?,
        content_type: row.try_get("document_content_type")?,
        content_length: row.try_get("document_content_length")?,
        checksum_sha256: row.try_get("document_checksum_sha256")?,
        checksum_crc32c: row.try_get("document_checksum_crc32c")?,
        upload_status: DocumentUploadStatus::from_str(&upload_status)?,
        created_at: row.try_get("document_created_at")?,
    }))
}

fn policy_catalog_entry_from_row(row: Row) -> Result<PolicyCatalogEntry, Error> {
    let document = row
        .try_get::<_, Option<String>>("document_upload_status")?
        .map(|status| {
            DocumentUploadStatus::from_str(&status)
                .map(|upload_status| PolicyDocumentStatus { upload_status })
        })
        .transpose()?;

    Ok(PolicyCatalogEntry {
        id: PolicyId::from(row.try_get::<_, Uuid>("id")?),
        name: row.try_get("name")?,
        description: row.try_get("description")?,
        mapped_control_count: row.try_get("mapped_control_count")?,
        document,
    })
}

fn policy_control_mapping_from_row(row: Row) -> Result<PolicyControlMapping, Error> {
    Ok(PolicyControlMapping {
        policy_id: PolicyId::from(row.try_get::<_, Uuid>("policy_id")?),
        control: ControlSummary {
            id: ControlId::from(row.try_get::<_, Uuid>("control_id")?),
            code: row.try_get("control_code")?,
            title: row.try_get("control_title")?,
            description: row.try_get("control_description")?,
        },
        created_at: row.try_get("mapping_created_at")?,
    })
}
