use std::str::FromStr;

use uuid::Uuid;

use crate::{
    domain::{
        ControlId, Document, DocumentId, DocumentIdentity, DocumentUploadStatus, PolicyId,
        WorkspaceId,
    },
    persistence::Error,
    read_models::{
        ControlSummary, DocumentDownloadCandidate, PolicyCatalogEntry, PolicyControlMapping,
        PolicyDetail, PolicyDocumentDetail, PolicyDocumentStatus, PolicyDocumentUploadEligibility,
    },
};

use super::{documents::document_from_row, param, ReadExecutor, TransactionalReadExecutor};

pub(crate) struct PolicyReads<'a, E> {
    executor: &'a E,
    workspace_id: WorkspaceId,
}
impl<'a, E> PolicyReads<'a, E> {
    pub(crate) fn new(executor: &'a E, workspace_id: WorkspaceId) -> Self {
        Self {
            executor,
            workspace_id,
        }
    }
}

impl<E: ReadExecutor> PolicyReads<'_, E> {
    pub async fn get(&self, id: PolicyId) -> Result<Option<PolicyDetail>, Error> {
        policy_detail(
            self.executor
                .query(
                    DETAIL_SQL,
                    &[
                        param(&Uuid::from(id)),
                        param(&Uuid::from(self.workspace_id)),
                    ],
                )
                .await?,
        )
    }
    pub async fn list_catalog(&self) -> Result<Vec<PolicyCatalogEntry>, Error> {
        self.executor
            .query(CATALOG_SQL, &[param(&Uuid::from(self.workspace_id))])
            .await?
            .into_iter()
            .map(catalog_row)
            .collect()
    }
    pub async fn get_agent_upload_document(
        &self,
        policy_id: PolicyId,
        document_id: DocumentId,
    ) -> Result<Option<Document>, Error> {
        self.document(policy_id, Some(document_id), false).await
    }
    pub async fn get_document_for_download(
        &self,
        policy_id: PolicyId,
        document_id: DocumentId,
    ) -> Result<Option<DocumentDownloadCandidate>, Error> {
        Ok(self
            .document(policy_id, Some(document_id), true)
            .await?
            .map(|document| DocumentDownloadCandidate {
                workspace_id: self.workspace_id,
                document,
            }))
    }
    pub async fn get_current_document(
        &self,
        policy_id: PolicyId,
    ) -> Result<Option<Document>, Error> {
        self.document(policy_id, None, true).await
    }
    async fn document(
        &self,
        policy_id: PolicyId,
        document_id: Option<DocumentId>,
        active: bool,
    ) -> Result<Option<Document>, Error> {
        let sql = format!("SELECT d.*, d.owner_id AS policy_id FROM documents d JOIN policies p ON p.id = d.owner_id WHERE p.id = $1 AND p.workspace_id = $2 AND d.owner_type = 'policy' AND d.workspace_id = $2{}{}", if active { " AND p.archived_at IS NULL AND d.archived = false" } else { "" }, if document_id.is_some() { " AND d.id = $3" } else { "" });
        let row = match document_id {
            Some(id) => {
                self.executor
                    .query_opt(
                        &sql,
                        &[
                            param(&Uuid::from(policy_id)),
                            param(&Uuid::from(self.workspace_id)),
                            param(&Uuid::from(id)),
                        ],
                    )
                    .await?
            }
            None => {
                self.executor
                    .query_opt(
                        &sql,
                        &[
                            param(&Uuid::from(policy_id)),
                            param(&Uuid::from(self.workspace_id)),
                        ],
                    )
                    .await?
            }
        };
        row.map(|row| {
            let document_id = row.try_get::<_, Uuid>("id")?.into();
            document_from_row(
                &row,
                DocumentIdentity::Policy {
                    policy_id,
                    document_id,
                },
            )
        })
        .transpose()
    }
}

impl PolicyReads<'_, TransactionalReadExecutor<'_>> {
    pub async fn is_active(&self, id: PolicyId) -> Result<bool, Error> {
        Ok(self.executor.query_opt("SELECT id FROM policies WHERE id = $1 AND workspace_id = $2 AND archived_at IS NULL FOR KEY SHARE", &[param(&Uuid::from(id)), param(&Uuid::from(self.workspace_id))]).await?.is_some())
    }
    pub async fn document_in_progress(&self, id: PolicyId) -> Result<bool, Error> {
        Ok(self.executor.query_one("SELECT EXISTS (SELECT 1 FROM documents WHERE owner_type = 'policy' AND owner_id = $1 AND workspace_id = $2 AND archived = false AND upload_status IN ('pending', 'finalizing'))", &[param(&Uuid::from(id)), param(&Uuid::from(self.workspace_id))]).await?.try_get(0)?)
    }
    pub async fn lock_document_upload_eligibility(
        &self,
        id: PolicyId,
    ) -> Result<Option<PolicyDocumentUploadEligibility>, Error> {
        if self.executor.query_opt("SELECT id FROM policies p WHERE p.id = $1 AND p.workspace_id = $2 AND p.archived_at IS NULL FOR UPDATE OF p", &[param(&Uuid::from(id)), param(&Uuid::from(self.workspace_id))]).await?.is_none() { return Ok(None); }
        let current: bool = self.executor.query_one("SELECT EXISTS (SELECT 1 FROM documents d WHERE d.owner_type = 'policy' AND d.owner_id = $1 AND d.workspace_id = $2 AND d.archived = false)", &[param(&Uuid::from(id)), param(&Uuid::from(self.workspace_id))]).await?.try_get(0)?;
        Ok(Some(if current {
            PolicyDocumentUploadEligibility::CurrentDocument
        } else {
            PolicyDocumentUploadEligibility::Eligible
        }))
    }
    pub async fn get_control_mapping(
        &self,
        policy_id: PolicyId,
        control_id: ControlId,
    ) -> Result<Option<PolicyControlMapping>, Error> {
        self.executor
            .query_opt(
                MAPPING_SQL,
                &[
                    param(&Uuid::from(policy_id)),
                    param(&Uuid::from(control_id)),
                    param(&Uuid::from(self.workspace_id)),
                ],
            )
            .await?
            .map(mapping_row)
            .transpose()
    }
}

const DETAIL_SQL: &str = "SELECT p.id, p.id AS policy_id, p.workspace_id, p.name, p.description, p.created_at, p.updated_at, p.archived_at, d.id AS document_id, d.created_by_user_id AS document_created_by_user_id, d.filename AS document_filename, d.content_type AS document_content_type, d.content_length AS document_content_length, d.checksum_sha256 AS document_checksum_sha256, d.checksum_crc32c AS document_checksum_crc32c, d.upload_status AS document_upload_status, d.created_at AS document_created_at, c.id AS control_id, c.code AS control_code, c.title AS control_title, c.description AS control_description, m.created_at AS mapping_created_at FROM policies p LEFT JOIN documents d ON d.owner_id = p.id AND d.owner_type = 'policy' AND d.workspace_id = p.workspace_id AND d.archived = false LEFT JOIN policy_control_mappings m ON m.policy_id = p.id LEFT JOIN controls c ON c.id = m.control_id AND c.workspace_id = p.workspace_id WHERE p.id = $1 AND p.workspace_id = $2 AND p.archived_at IS NULL ORDER BY lower(c.code), c.id";
const CATALOG_SQL: &str = "SELECT p.id, p.name, p.description, count(m.control_id) AS mapped_control_count, d.upload_status AS document_upload_status FROM policies p LEFT JOIN policy_control_mappings m ON m.policy_id = p.id LEFT JOIN documents d ON d.owner_id = p.id AND d.owner_type = 'policy' AND d.workspace_id = p.workspace_id AND d.archived = false WHERE p.workspace_id = $1 AND p.archived_at IS NULL GROUP BY p.id, d.upload_status ORDER BY lower(p.name), p.id";
const MAPPING_SQL: &str = "SELECT m.policy_id, c.id AS control_id, c.code AS control_code, c.title AS control_title, c.description AS control_description, m.created_at AS mapping_created_at FROM policy_control_mappings m JOIN policies p ON p.id = m.policy_id JOIN controls c ON c.id = m.control_id WHERE m.policy_id = $1 AND m.control_id = $2 AND p.workspace_id = $3 AND c.workspace_id = $3 AND p.archived_at IS NULL";

fn policy_detail(rows: Vec<tokio_postgres::Row>) -> Result<Option<PolicyDetail>, Error> {
    let document = rows.first().map(document_detail).transpose()?.flatten();
    let mut result = None;
    for row in rows {
        if result.is_none() {
            result = Some(PolicyDetail {
                id: row.try_get::<_, Uuid>("id")?.into(),
                workspace_id: row.try_get::<_, Uuid>("workspace_id")?.into(),
                name: row.try_get("name")?,
                description: row.try_get("description")?,
                control_mappings: vec![],
                created_at: row.try_get("created_at")?,
                updated_at: row.try_get("updated_at")?,
                archived_at: row.try_get("archived_at")?,
                document: document.clone(),
            });
        }
        if row.try_get::<_, Option<Uuid>>("control_id")?.is_some() {
            if let Some(policy) = &mut result {
                policy.control_mappings.push(mapping_row(row)?);
            }
        }
    }
    Ok(result)
}
fn document_detail(row: &tokio_postgres::Row) -> Result<Option<PolicyDocumentDetail>, Error> {
    let Some(id) = row.try_get::<_, Option<Uuid>>("document_id")? else {
        return Ok(None);
    };
    Ok(Some(PolicyDocumentDetail {
        id: id.into(),
        created_by_user_id: row
            .try_get::<_, Uuid>("document_created_by_user_id")?
            .into(),
        filename: row.try_get("document_filename")?,
        content_type: row.try_get("document_content_type")?,
        content_length: row.try_get("document_content_length")?,
        checksum_sha256: row.try_get("document_checksum_sha256")?,
        checksum_crc32c: row.try_get("document_checksum_crc32c")?,
        upload_status: DocumentUploadStatus::from_str(
            &row.try_get::<_, String>("document_upload_status")?,
        )?,
        created_at: row.try_get("document_created_at")?,
    }))
}
fn catalog_row(row: tokio_postgres::Row) -> Result<PolicyCatalogEntry, Error> {
    let document = row
        .try_get::<_, Option<String>>("document_upload_status")?
        .map(|s| {
            DocumentUploadStatus::from_str(&s)
                .map(|upload_status| PolicyDocumentStatus { upload_status })
        })
        .transpose()?;
    Ok(PolicyCatalogEntry {
        id: row.try_get::<_, Uuid>("id")?.into(),
        name: row.try_get("name")?,
        description: row.try_get("description")?,
        mapped_control_count: row.try_get("mapped_control_count")?,
        document,
    })
}
fn mapping_row(row: tokio_postgres::Row) -> Result<PolicyControlMapping, Error> {
    Ok(PolicyControlMapping {
        policy_id: row.try_get::<_, Uuid>("policy_id")?.into(),
        control: ControlSummary {
            id: row.try_get::<_, Uuid>("control_id")?.into(),
            code: row.try_get("control_code")?,
            title: row.try_get("control_title")?,
            description: row.try_get("control_description")?,
        },
        created_at: row.try_get("mapping_created_at")?,
    })
}
