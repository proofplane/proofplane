use std::str::FromStr;

use chrono::{DateTime, Utc};
use tokio_postgres::Row;
use uuid::Uuid;

use crate::{
    domain::{
        ControlId, ControlSummary, CreateControlPolicyMappingsPayload,
        CreatePolicyControlMappingsPayload, CreatePolicyPayload,
        DeleteControlPolicyMappingsPayload, DeletePolicyControlMappingsPayload, Document,
        DocumentId, DocumentIdentity, DocumentUploadStatus, Policy, PolicyAggregate,
        PolicyControlMapping, PolicyControlMappingState, PolicyDefinition, PolicyId,
        UpdatePolicyPayload, WorkspaceId,
    },
    projections::policy_projection::{
        PolicyCatalogEntry, PolicyDetail, PolicyDocumentDetail, PolicyDocumentStatus,
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

use super::{
    constraints::classify_db_error, controls::ids_present_in, documents::document_from_row,
    BatchMapRejection, BatchUnmapRejection, Error,
};

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
    pub async fn get(&self, id: PolicyId) -> Result<Option<PolicyAggregate>, Error> {
        let Some(row) = self.context.transaction.query_opt("SELECT id, workspace_id, name, description, created_at, updated_at, archived_at FROM policies WHERE id = $1 AND workspace_id = $2 FOR UPDATE", &[&Uuid::from(id), &Uuid::from(self.context.workspace_id)]).await? else { return Ok(None) };
        let record = PolicyRecord::try_from(row)?;
        let mappings = self.context.transaction.query("SELECT control_id, created_at FROM policy_control_mappings WHERE policy_id = $1 ORDER BY control_id", &[&Uuid::from(id)]).await?.into_iter().map(policy_mapping_from_row).collect::<Result<Vec<_>, _>>()?;
        record.into_aggregate(mappings).map(Some)
    }

    /// Persists the aggregate's complete definition, archive lifecycle, and mapping snapshot.
    pub async fn save(&self, policy: &PolicyAggregate) -> Result<(), Error> {
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
    fn into_aggregate(
        self,
        mappings: Vec<PolicyControlMappingState>,
    ) -> Result<PolicyAggregate, Error> {
        let definition = PolicyDefinition::new(self.name, self.description)
            .into_result()
            .map_err(|_| Error::InvariantViolation("persisted policy definition is invalid"))?;
        PolicyAggregate::rehydrate(
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
impl From<&PolicyAggregate> for PolicyRecord {
    fn from(policy: &PolicyAggregate) -> Self {
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

    pub async fn create_policy(&self, payload: &CreatePolicyPayload) -> Result<Policy, Error> {
        let row = self
            .transaction
            .query_one(
                r#"
INSERT INTO policies (workspace_id, name, description)
VALUES ($1, $2, $3)
RETURNING id
"#,
                &[
                    &Uuid::from(self.workspace_id),
                    &payload.name,
                    &payload.description,
                ],
            )
            .await
            .map_err(classify_db_error)?;
        let policy_id = PolicyId::from(row.try_get::<_, Uuid>("id")?);

        self.insert_policy_control_mappings(policy_id, &payload.control_ids)
            .await?;

        self.get_active_policy(policy_id)
            .await?
            .ok_or(Error::InvariantViolation(
                "created policy must be readable in transaction",
            ))
    }

    pub async fn update_policy(
        &self,
        policy_id: PolicyId,
        payload: &UpdatePolicyPayload,
    ) -> Result<Option<Policy>, Error> {
        let updated = self
            .transaction
            .execute(
                r#"
UPDATE policies
SET name = $2,
    description = $3,
    updated_at = now()
WHERE id = $1
  AND workspace_id = $4
  AND archived_at IS NULL
"#,
                &[
                    &Uuid::from(policy_id),
                    &payload.name,
                    &payload.description,
                    &Uuid::from(self.workspace_id),
                ],
            )
            .await
            .map_err(classify_db_error)?;

        if updated == 0 {
            return Ok(None);
        }

        self.get_active_policy(policy_id)
            .await?
            .ok_or(Error::InvariantViolation(
                "updated policy must be readable in transaction",
            ))
            .map(Some)
    }

    pub async fn attach_policy_to_control(
        &self,
        policy_id: PolicyId,
        control_id: ControlId,
    ) -> Result<Option<PolicyControlMapping>, Error> {
        let rows = self
            .transaction
            .query(
                r#"
INSERT INTO policy_control_mappings (policy_id, control_id)
SELECT p.id, c.id
FROM policies p
JOIN controls c ON c.id = $2 AND c.workspace_id = $3
WHERE p.id = $1
  AND p.workspace_id = $3
  AND p.archived_at IS NULL
RETURNING policy_id, control_id
"#,
                &[
                    &Uuid::from(policy_id),
                    &Uuid::from(control_id),
                    &Uuid::from(self.workspace_id),
                ],
            )
            .await
            .map_err(classify_db_error)?;

        if rows.is_empty() {
            return Ok(None);
        }

        self.get_policy_control_mapping(policy_id, control_id)
            .await?
            .ok_or(Error::InvariantViolation(
                "created policy control mapping must be readable in transaction",
            ))
            .map(Some)
    }

    pub async fn attach_policy_to_controls(
        &self,
        payload: &CreatePolicyControlMappingsPayload,
    ) -> Result<Option<Vec<ControlId>>, Error> {
        let workspace_id = Uuid::from(self.workspace_id);
        let policy_id = Uuid::from(payload.policy_id);

        let anchor = self
            .transaction
            .query_opt(
                r#"
SELECT 1
FROM policies
WHERE id = $1
  AND workspace_id = $2
  AND archived_at IS NULL
"#,
                &[&policy_id, &workspace_id],
            )
            .await?;

        if anchor.is_none() {
            return Ok(None);
        }

        // Resolve which controls exist in the workspace and which are already
        // attached to this policy before inserting, so a rejection names every
        // unknown and every already-attached id at once — a conflicting insert
        // would abort the transaction and leave us nothing to report.
        let control_ids = payload
            .control_ids
            .iter()
            .copied()
            .map(Uuid::from)
            .collect::<Vec<_>>();
        let in_workspace = self
            .transaction
            .query(
                r#"
SELECT id
FROM controls
WHERE id = ANY($1)
  AND workspace_id = $2
"#,
                &[&control_ids, &workspace_id],
            )
            .await?;
        let existing = self
            .transaction
            .query(
                r#"
SELECT control_id
FROM policy_control_mappings
WHERE policy_id = $1
  AND control_id = ANY($2)
"#,
                &[&policy_id, &control_ids],
            )
            .await?;

        let known = ids_present_in(&in_workspace, "id")?;
        let already = ids_present_in(&existing, "control_id")?;
        let mut rejection = BatchMapRejection::default();
        for id in &control_ids {
            if !known.contains(id) {
                rejection.unknown.push(*id);
            } else if already.contains(id) {
                rejection.already_mapped.push(*id);
            }
        }
        if !rejection.is_empty() {
            return Err(Error::BatchMapRejected(rejection));
        }

        self.transaction
            .execute(
                r#"
INSERT INTO policy_control_mappings (policy_id, control_id)
SELECT $1, unnest($2::uuid[])
"#,
                &[&policy_id, &control_ids],
            )
            .await
            .map_err(classify_db_error)?;

        Ok(Some(payload.control_ids.clone()))
    }

    pub async fn attach_control_to_policies(
        &self,
        payload: &CreateControlPolicyMappingsPayload,
    ) -> Result<Option<Vec<PolicyId>>, Error> {
        let workspace_id = Uuid::from(self.workspace_id);
        let control_id = Uuid::from(payload.control_id);

        let anchor = self
            .transaction
            .query_opt(
                r#"
SELECT 1
FROM controls
WHERE id = $1
  AND workspace_id = $2
"#,
                &[&control_id, &workspace_id],
            )
            .await?;

        if anchor.is_none() {
            return Ok(None);
        }

        // Resolve every requested policy and every existing mapping before
        // inserting so an unknown, archived, or already-attached id can be named
        // precisely; a failing insert would abort the transaction and leave us no
        // way to report which ids were at fault.
        let policy_ids = payload
            .policy_ids
            .iter()
            .copied()
            .map(Uuid::from)
            .collect::<Vec<_>>();
        let in_workspace = self
            .transaction
            .query(
                r#"
SELECT id, archived_at IS NOT NULL AS archived
FROM policies
WHERE id = ANY($1)
  AND workspace_id = $2
"#,
                &[&policy_ids, &workspace_id],
            )
            .await?;
        let existing = self
            .transaction
            .query(
                r#"
SELECT policy_id
FROM policy_control_mappings
WHERE control_id = $1
  AND policy_id = ANY($2)
"#,
                &[&control_id, &policy_ids],
            )
            .await?;

        let known = ids_present_in(&in_workspace, "id")?;
        let archived_ids = in_workspace
            .iter()
            .filter(|row| row.try_get::<_, bool>("archived").unwrap_or(false))
            .map(|row| row.try_get::<_, Uuid>("id"))
            .collect::<Result<std::collections::HashSet<_>, _>>()?;
        let already = ids_present_in(&existing, "policy_id")?;
        // Classify in request order; unknown precedes archived precedes
        // already-attached for a single id, but every id lands in some bucket so
        // the whole batch's failures come back in one response.
        let mut rejection = BatchMapRejection::default();
        for id in &policy_ids {
            if !known.contains(id) {
                rejection.unknown.push(*id);
            } else if archived_ids.contains(id) {
                rejection.archived.push(*id);
            } else if already.contains(id) {
                rejection.already_mapped.push(*id);
            }
        }
        if !rejection.is_empty() {
            return Err(Error::BatchMapRejected(rejection));
        }

        self.transaction
            .execute(
                r#"
INSERT INTO policy_control_mappings (policy_id, control_id)
SELECT unnest($1::uuid[]), $2
"#,
                &[&policy_ids, &control_id],
            )
            .await
            .map_err(classify_db_error)?;

        Ok(Some(payload.policy_ids.clone()))
    }

    pub async fn detach_policy_from_controls(
        &self,
        payload: &DeletePolicyControlMappingsPayload,
    ) -> Result<Option<Vec<ControlId>>, Error> {
        let workspace_id = Uuid::from(self.workspace_id);
        let policy_id = Uuid::from(payload.policy_id);

        let anchor = self
            .transaction
            .query_opt(
                r#"
SELECT 1
FROM policies
WHERE id = $1
  AND workspace_id = $2
  AND archived_at IS NULL
"#,
                &[&policy_id, &workspace_id],
            )
            .await?;

        if anchor.is_none() {
            return Ok(None);
        }

        let control_ids = payload
            .control_ids
            .iter()
            .map(|id| Uuid::from(*id))
            .collect::<Vec<_>>();
        let rows = self
            .transaction
            .query(
                r#"
WITH requested AS (
    SELECT unnest($2::uuid[]) AS control_id
),
removed AS (
    DELETE FROM policy_control_mappings m
    USING policies p, controls c
    WHERE m.policy_id = p.id
      AND m.control_id = c.id
      AND p.id = $1
      AND c.id IN (SELECT control_id FROM requested)
      AND p.workspace_id = $3
      AND c.workspace_id = $3
      AND p.archived_at IS NULL
    RETURNING m.control_id
)
SELECT
    r.control_id,
    EXISTS (
        SELECT 1
        FROM controls c
        WHERE c.id = r.control_id
          AND c.workspace_id = $3
    ) AS control_exists,
    EXISTS (
        SELECT 1
        FROM removed
        WHERE removed.control_id = r.control_id
    ) AS was_removed
FROM requested r
"#,
                &[&policy_id, &control_ids, &workspace_id],
            )
            .await?;

        let mut rejection = BatchUnmapRejection::default();
        for row in &rows {
            let control_id = row.try_get::<_, Uuid>("control_id")?;
            if !row.try_get::<_, bool>("control_exists")? {
                rejection.unknown.push(control_id);
            } else if !row.try_get::<_, bool>("was_removed")? {
                rejection.not_mapped.push(control_id);
            }
        }

        // An id the workspace does not have and an id it has but never mapped
        // read alike here yet call for opposite corrections, so they are reported
        // together in one rejection. Any bucket rolls the whole batch back.
        if !rejection.is_empty() {
            return Err(Error::BatchUnmapRejected(rejection));
        }

        Ok(Some(payload.control_ids.clone()))
    }

    pub async fn detach_control_from_policies(
        &self,
        payload: &DeleteControlPolicyMappingsPayload,
    ) -> Result<Option<Vec<PolicyId>>, Error> {
        let workspace_id = Uuid::from(self.workspace_id);
        let control_id = Uuid::from(payload.control_id);

        let anchor = self
            .transaction
            .query_opt(
                r#"
SELECT 1
FROM controls
WHERE id = $1
  AND workspace_id = $2
"#,
                &[&control_id, &workspace_id],
            )
            .await?;

        if anchor.is_none() {
            return Ok(None);
        }

        let policy_ids = payload
            .policy_ids
            .iter()
            .map(|id| Uuid::from(*id))
            .collect::<Vec<_>>();
        let rows = self
            .transaction
            .query(
                r#"
WITH requested AS (
    SELECT unnest($2::uuid[]) AS policy_id
),
removed AS (
    DELETE FROM policy_control_mappings m
    USING policies p, controls c
    WHERE m.policy_id = p.id
      AND m.control_id = c.id
      AND c.id = $1
      AND p.id IN (SELECT policy_id FROM requested)
      AND p.workspace_id = $3
      AND c.workspace_id = $3
      AND p.archived_at IS NULL
    RETURNING m.policy_id
)
SELECT
    r.policy_id,
    EXISTS (
        SELECT 1
        FROM policies p
        WHERE p.id = r.policy_id
          AND p.workspace_id = $3
    ) AS policy_exists,
    EXISTS (
        SELECT 1
        FROM policies p
        WHERE p.id = r.policy_id
          AND p.workspace_id = $3
          AND p.archived_at IS NOT NULL
    ) AS policy_archived,
    EXISTS (
        SELECT 1
        FROM removed
        WHERE removed.policy_id = r.policy_id
    ) AS was_removed
FROM requested r
"#,
                &[&control_id, &policy_ids, &workspace_id],
            )
            .await?;

        let mut rejection = BatchUnmapRejection::default();
        for row in &rows {
            let policy_id = row.try_get::<_, Uuid>("policy_id")?;
            if !row.try_get::<_, bool>("policy_exists")? {
                rejection.unknown.push(policy_id);
            } else if row.try_get::<_, bool>("policy_archived")? {
                rejection.archived.push(policy_id);
            } else if !row.try_get::<_, bool>("was_removed")? {
                rejection.not_mapped.push(policy_id);
            }
        }

        // Unknown, archived, and not-mapped ids read alike here but each calls
        // for a different correction, so they are reported together in one
        // rejection. Any bucket rolls the whole batch back.
        if !rejection.is_empty() {
            return Err(Error::BatchUnmapRejected(rejection));
        }

        Ok(Some(payload.policy_ids.clone()))
    }

    pub async fn detach_policy_from_control(
        &self,
        policy_id: PolicyId,
        control_id: ControlId,
    ) -> Result<bool, Error> {
        let deleted = self
            .transaction
            .execute(
                r#"
DELETE FROM policy_control_mappings m
USING policies p, controls c
WHERE m.policy_id = p.id
  AND m.control_id = c.id
  AND p.id = $1
  AND c.id = $2
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
            .await?;

        Ok(deleted > 0)
    }

    pub async fn archive_policy(&self, policy_id: PolicyId) -> Result<ArchivePolicyResult, Error> {
        let row = self
            .transaction
            .query_one(
                r#"
WITH scoped AS (
    SELECT
        p.id,
        EXISTS (
            SELECT 1
            FROM documents a
            WHERE a.owner_type = 'policy'
              AND a.owner_id = p.id
              AND a.archived = false
              AND a.upload_status IN ('pending', 'finalizing')
        ) AS document_in_progress
    FROM policies p
    WHERE p.id = $1
      AND p.workspace_id = $2
      AND p.archived_at IS NULL
    FOR UPDATE
),
updated AS (
    UPDATE policies p
    SET archived_at = now(),
        updated_at = now()
    FROM scoped
    WHERE p.id = scoped.id
      AND scoped.document_in_progress = false
    RETURNING p.archived_at
)
SELECT
    EXISTS (SELECT 1 FROM scoped) AS found,
    COALESCE((SELECT document_in_progress FROM scoped), false) AS document_in_progress,
    (SELECT archived_at FROM updated) AS archived_at
"#,
                &[&Uuid::from(policy_id), &Uuid::from(self.workspace_id)],
            )
            .await?;

        let found = row.try_get::<_, bool>("found")?;
        let document_in_progress = row.try_get::<_, bool>("document_in_progress")?;
        let archived_at = row.try_get::<_, Option<DateTime<Utc>>>("archived_at")?;

        match (found, document_in_progress, archived_at) {
            (false, _, _) => Ok(ArchivePolicyResult::NotFound),
            (true, true, _) => Ok(ArchivePolicyResult::DocumentInProgress),
            (true, false, Some(archived_at)) => Ok(ArchivePolicyResult::Archived {
                policy_id,
                archived_at,
            }),
            (true, false, None) => Err(Error::InvariantViolation(
                "archivable policy must return an archived timestamp",
            )),
        }
    }

    async fn insert_policy_control_mappings(
        &self,
        policy_id: PolicyId,
        control_ids: &[ControlId],
    ) -> Result<(), Error> {
        if control_ids.is_empty() {
            return Ok(());
        }

        let ids = control_ids
            .iter()
            .copied()
            .map(Uuid::from)
            .collect::<Vec<_>>();
        let rows = self
            .transaction
            .query(
                r#"
INSERT INTO policy_control_mappings (policy_id, control_id)
SELECT $1, c.id
FROM unnest($2::uuid[]) AS requested(control_id)
JOIN controls c ON c.id = requested.control_id
WHERE c.workspace_id = $3
RETURNING control_id
"#,
                &[&Uuid::from(policy_id), &ids, &Uuid::from(self.workspace_id)],
            )
            .await
            .map_err(classify_db_error)?;

        if rows.len() != control_ids.len() {
            return Err(Error::InvalidPolicyControlReferences);
        }

        Ok(())
    }

    async fn get_active_policy(&self, policy_id: PolicyId) -> Result<Option<Policy>, Error> {
        let rows = self
            .transaction
            .query(
                POLICY_ENTITY_DETAIL_QUERY,
                &[&Uuid::from(policy_id), &Uuid::from(self.workspace_id)],
            )
            .await?;

        Ok(policies_from_joined_rows(rows)?.into_iter().next())
    }

    pub async fn get_policy_detail(
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

    pub async fn get_policy_control_mapping(
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

    pub async fn list_policy_catalog(&self) -> Result<Vec<PolicyCatalogEntry>, Error> {
        let rows = self
            .client
            .query(POLICY_CATALOG_QUERY, &[&Uuid::from(self.workspace_id)])
            .await?;
        rows.into_iter()
            .map(policy_catalog_entry_from_row)
            .collect()
    }

    pub async fn get_policy_detail(
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

const POLICY_ENTITY_DETAIL_QUERY: &str = r#"
SELECT
    p.id,
    p.id AS policy_id,
    p.workspace_id,
    p.name,
    p.description,
    p.created_at,
    p.updated_at,
    p.archived_at,
    c.id AS control_id,
    c.code AS control_code,
    c.title AS control_title,
    c.description AS control_description,
    m.created_at AS mapping_created_at
FROM policies p
LEFT JOIN policy_control_mappings m ON m.policy_id = p.id
LEFT JOIN controls c ON c.id = m.control_id AND c.workspace_id = p.workspace_id
WHERE p.id = $1
  AND p.workspace_id = $2
  AND p.archived_at IS NULL
ORDER BY lower(c.code), c.id
"#;

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

fn policies_from_joined_rows(rows: Vec<Row>) -> Result<Vec<Policy>, Error> {
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

fn policy_from_row(row: &Row) -> Result<Policy, Error> {
    Ok(Policy {
        id: PolicyId::from(row.try_get::<_, Uuid>("id")?),
        workspace_id: WorkspaceId::from(row.try_get::<_, Uuid>("workspace_id")?),
        name: row.try_get("name")?,
        description: row.try_get("description")?,
        control_mappings: Vec::new(),
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
        archived_at: row.try_get("archived_at")?,
    })
}

fn policy_detail_from_joined_rows(rows: Vec<Row>) -> Result<Option<PolicyDetail>, Error> {
    let document = rows
        .first()
        .map(policy_document_detail_from_row)
        .transpose()?
        .flatten();
    let policy = policies_from_joined_rows(rows)?.into_iter().next();

    Ok(policy.map(|policy| PolicyDetail { policy, document }))
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
