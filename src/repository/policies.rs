use std::str::FromStr;

use chrono::{DateTime, Utc};
use tokio_postgres::Row;
use uuid::Uuid;

use crate::{
    domain::{
        AttachmentUploadStatus, ControlId, ControlSummary, CreatePolicyPayload, Policy,
        PolicyAttachmentId, PolicyControlMapping, PolicyId, UpdatePolicyPayload, WorkspaceId,
    },
    projections::policy_projection::{
        PolicyAttachmentDetail, PolicyAttachmentStatus, PolicyCatalogEntry, PolicyDetail,
    },
    repository::{WorkspaceReadContext, WorkspaceTransactionContext},
};

use super::{constraints::classify_db_error, Error};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArchivePolicyResult {
    Archived {
        policy_id: PolicyId,
        archived_at: DateTime<Utc>,
    },
    NotFound,
    AttachmentInProgress,
}

impl WorkspaceTransactionContext<'_> {
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
            FROM policy_attachments a
            WHERE a.policy_id = p.id
              AND a.archived = false
              AND a.upload_status IN ('pending', 'finalizing')
        ) AS attachment_in_progress
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
      AND scoped.attachment_in_progress = false
    RETURNING p.archived_at
)
SELECT
    EXISTS (SELECT 1 FROM scoped) AS found,
    COALESCE((SELECT attachment_in_progress FROM scoped), false) AS attachment_in_progress,
    (SELECT archived_at FROM updated) AS archived_at
"#,
                &[&Uuid::from(policy_id), &Uuid::from(self.workspace_id)],
            )
            .await?;

        let found = row.try_get::<_, bool>("found")?;
        let attachment_in_progress = row.try_get::<_, bool>("attachment_in_progress")?;
        let archived_at = row.try_get::<_, Option<DateTime<Utc>>>("archived_at")?;

        match (found, attachment_in_progress, archived_at) {
            (false, _, _) => Ok(ArchivePolicyResult::NotFound),
            (true, true, _) => Ok(ArchivePolicyResult::AttachmentInProgress),
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

    async fn get_policy_control_mapping(
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
    a.id AS attachment_id,
    a.filename AS attachment_filename,
    a.content_type AS attachment_content_type,
    a.content_length AS attachment_content_length,
    a.checksum_sha256 AS attachment_checksum_sha256,
    a.checksum_crc32c AS attachment_checksum_crc32c,
    a.upload_status AS attachment_upload_status,
    a.created_at AS attachment_created_at,
    c.id AS control_id,
    c.code AS control_code,
    c.title AS control_title,
    c.description AS control_description,
    m.created_at AS mapping_created_at
FROM policies p
LEFT JOIN policy_attachments a ON a.policy_id = p.id AND a.archived = false
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
    a.upload_status AS attachment_upload_status
FROM policies p
LEFT JOIN policy_control_mappings m ON m.policy_id = p.id
LEFT JOIN policy_attachments a ON a.policy_id = p.id AND a.archived = false
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
    let attachment = rows
        .first()
        .map(policy_attachment_detail_from_row)
        .transpose()?
        .flatten();
    let policy = policies_from_joined_rows(rows)?.into_iter().next();

    Ok(policy.map(|policy| PolicyDetail { policy, attachment }))
}

fn policy_attachment_detail_from_row(row: &Row) -> Result<Option<PolicyAttachmentDetail>, Error> {
    let Some(id) = row.try_get::<_, Option<Uuid>>("attachment_id")? else {
        return Ok(None);
    };
    let upload_status = row.try_get::<_, String>("attachment_upload_status")?;

    Ok(Some(PolicyAttachmentDetail {
        id: PolicyAttachmentId::from(id),
        filename: row.try_get("attachment_filename")?,
        content_type: row.try_get("attachment_content_type")?,
        content_length: row.try_get("attachment_content_length")?,
        checksum_sha256: row.try_get("attachment_checksum_sha256")?,
        checksum_crc32c: row.try_get("attachment_checksum_crc32c")?,
        upload_status: AttachmentUploadStatus::from_str(&upload_status)?,
        created_at: row.try_get("attachment_created_at")?,
    }))
}

fn policy_catalog_entry_from_row(row: Row) -> Result<PolicyCatalogEntry, Error> {
    let attachment = row
        .try_get::<_, Option<String>>("attachment_upload_status")?
        .map(|status| {
            AttachmentUploadStatus::from_str(&status)
                .map(|upload_status| PolicyAttachmentStatus { upload_status })
        })
        .transpose()?;

    Ok(PolicyCatalogEntry {
        id: PolicyId::from(row.try_get::<_, Uuid>("id")?),
        name: row.try_get("name")?,
        description: row.try_get("description")?,
        mapped_control_count: row.try_get("mapped_control_count")?,
        attachment,
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
