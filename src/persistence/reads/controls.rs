use std::collections::HashSet;

use tokio_postgres::Row;
use uuid::Uuid;

use crate::{
    domain::{ControlId, EvidenceId, FrameworkId, WorkspaceId},
    persistence::Error,
    read_models::{
        ControlDetail, ControlEvidenceMapping, ControlPolicyMapping, ControlSummary,
        EvidenceControlMapping, EvidenceDetail, FrameworkRequirementDetail, PolicySummary,
    },
};

use super::{param, ReadExecutor, TransactionalReadExecutor};

pub(crate) struct ControlReads<'a, E> {
    executor: &'a E,
    workspace_id: WorkspaceId,
}

impl<'a, E> ControlReads<'a, E> {
    pub(crate) fn new(executor: &'a E, workspace_id: WorkspaceId) -> Self {
        Self {
            executor,
            workspace_id,
        }
    }
}

impl<E: ReadExecutor> ControlReads<'_, E> {
    pub async fn get(&self, id: ControlId) -> Result<Option<ControlDetail>, Error> {
        let rows = self
            .executor
            .query(
                CONTROL_DETAIL_SQL,
                &[
                    param(&Uuid::from(id)),
                    param(&Uuid::from(self.workspace_id)),
                ],
            )
            .await?;
        Ok(controls_from_joined_rows(rows)?.into_iter().next())
    }

    pub async fn list(&self) -> Result<Vec<ControlDetail>, Error> {
        let rows = self
            .executor
            .query(CONTROL_LIST_SQL, &[param(&Uuid::from(self.workspace_id))])
            .await?;
        controls_from_joined_rows(rows)
    }

    pub async fn list_evidence_mappings(
        &self,
        evidence_id: EvidenceId,
    ) -> Result<Option<Vec<EvidenceControlMapping>>, Error> {
        let rows = self
            .executor
            .query(
                EVIDENCE_CONTROL_MAPPINGS_SQL,
                &[
                    param(&Uuid::from(evidence_id)),
                    param(&Uuid::from(self.workspace_id)),
                ],
            )
            .await?;
        if rows.is_empty() {
            return Ok(None);
        }
        rows.into_iter()
            .filter_map(evidence_control_mapping_from_joined_row)
            .collect::<Result<Vec<_>, _>>()
            .map(Some)
    }

    pub async fn list_evidence_for_control(
        &self,
        control_id: ControlId,
    ) -> Result<Option<Vec<ControlEvidenceMapping>>, Error> {
        if self
            .executor
            .query_opt(
                "SELECT 1 FROM controls WHERE id = $1 AND workspace_id = $2",
                &[
                    param(&Uuid::from(control_id)),
                    param(&Uuid::from(self.workspace_id)),
                ],
            )
            .await?
            .is_none()
        {
            return Ok(None);
        }
        self.executor
            .query(
                CONTROL_EVIDENCE_MAPPINGS_SQL,
                &[
                    param(&Uuid::from(control_id)),
                    param(&Uuid::from(self.workspace_id)),
                ],
            )
            .await?
            .into_iter()
            .map(|row| {
                Ok(ControlEvidenceMapping {
                    evidence: EvidenceDetail {
                        id: row.try_get::<_, Uuid>("id")?.into(),
                        workspace_id: row.try_get::<_, Uuid>("workspace_id")?.into(),
                        title: row.try_get("title")?,
                        description: row.try_get("description")?,
                        collection_instructions: row.try_get("collection_instructions")?,
                        status: row.try_get::<_, String>("status")?.parse()?,
                        created_at: row.try_get("evidence_created_at")?,
                        updated_at: row.try_get("updated_at")?,
                    },
                    rationale: row.try_get("rationale")?,
                    created_at: row.try_get("mapping_created_at")?,
                })
            })
            .collect::<Result<Vec<_>, Error>>()
            .map(Some)
    }

    pub async fn list_policies_for_control(
        &self,
        control_id: ControlId,
    ) -> Result<Option<Vec<ControlPolicyMapping>>, Error> {
        if self
            .executor
            .query_opt(
                "SELECT 1 FROM controls WHERE id = $1 AND workspace_id = $2",
                &[
                    param(&Uuid::from(control_id)),
                    param(&Uuid::from(self.workspace_id)),
                ],
            )
            .await?
            .is_none()
        {
            return Ok(None);
        }
        self.executor
            .query(
                CONTROL_POLICY_MAPPINGS_SQL,
                &[
                    param(&Uuid::from(control_id)),
                    param(&Uuid::from(self.workspace_id)),
                ],
            )
            .await?
            .into_iter()
            .map(|row| {
                Ok(ControlPolicyMapping {
                    policy: PolicySummary {
                        id: row.try_get::<_, Uuid>("id")?.into(),
                        workspace_id: row.try_get::<_, Uuid>("workspace_id")?.into(),
                        name: row.try_get("name")?,
                        description: row.try_get("description")?,
                        created_at: row.try_get("policy_created_at")?,
                        updated_at: row.try_get("updated_at")?,
                    },
                    created_at: row.try_get("mapping_created_at")?,
                })
            })
            .collect::<Result<Vec<_>, Error>>()
            .map(Some)
    }
}

impl ControlReads<'_, TransactionalReadExecutor<'_>> {
    pub async fn get_evidence_mapping(
        &self,
        evidence_id: EvidenceId,
        control_id: ControlId,
    ) -> Result<Option<EvidenceControlMapping>, Error> {
        self.executor.query_opt(
            "SELECT e.id AS evidence_id, c.id AS control_id, c.code AS control_code, c.title AS control_title, c.description AS control_description, m.rationale, m.created_at FROM evidence_control_mappings m JOIN evidence e ON e.id = m.evidence_id AND e.workspace_id = $3 JOIN controls c ON c.id = m.control_id AND c.workspace_id = $3 WHERE m.evidence_id = $1 AND m.control_id = $2",
            &[param(&Uuid::from(evidence_id)), param(&Uuid::from(control_id)), param(&Uuid::from(self.workspace_id))],
        ).await?.map(evidence_control_mapping_from_row).transpose()
    }

    pub async fn existing_ids(&self, ids: &[ControlId]) -> Result<HashSet<ControlId>, Error> {
        if ids.is_empty() {
            return Ok(HashSet::new());
        }
        let requested = ids.iter().copied().map(Uuid::from).collect::<Vec<_>>();
        self.executor
            .query(
                "SELECT id FROM controls WHERE workspace_id = $1 AND id = ANY($2) FOR KEY SHARE",
                &[param(&Uuid::from(self.workspace_id)), param(&requested)],
            )
            .await?
            .into_iter()
            .map(|row| {
                row.try_get::<_, Uuid>("id")
                    .map(ControlId::from)
                    .map_err(Error::from)
            })
            .collect()
    }

    pub async fn ids_exist(&self, ids: &[ControlId]) -> Result<bool, Error> {
        Ok(
            self.existing_ids(ids).await?.len()
                == ids.iter().copied().collect::<HashSet<_>>().len(),
        )
    }
}

const CONTROL_DETAIL_SQL: &str = "SELECT c.id, c.workspace_id, c.code, c.title, c.description, c.created_at, c.updated_at, fr.id AS framework_requirement_id, fr.framework_id AS framework_requirement_framework_id, f.code AS framework_requirement_framework_code, f.name AS framework_requirement_framework_name, fr.code AS framework_requirement_code, fr.title AS framework_requirement_title, fr.description AS framework_requirement_description FROM controls c LEFT JOIN control_framework_requirement_mappings m ON m.control_id = c.id LEFT JOIN framework_requirements fr ON fr.id = m.framework_requirement_id LEFT JOIN frameworks f ON f.id = fr.framework_id WHERE c.id = $1 AND c.workspace_id = $2 ORDER BY fr.code";
const CONTROL_LIST_SQL: &str = "SELECT c.id, c.workspace_id, c.code, c.title, c.description, c.created_at, c.updated_at, fr.id AS framework_requirement_id, fr.framework_id AS framework_requirement_framework_id, f.code AS framework_requirement_framework_code, f.name AS framework_requirement_framework_name, fr.code AS framework_requirement_code, fr.title AS framework_requirement_title, fr.description AS framework_requirement_description FROM controls c LEFT JOIN control_framework_requirement_mappings m ON m.control_id = c.id LEFT JOIN framework_requirements fr ON fr.id = m.framework_requirement_id LEFT JOIN frameworks f ON f.id = fr.framework_id WHERE c.workspace_id = $1 ORDER BY c.code, fr.code";
const EVIDENCE_CONTROL_MAPPINGS_SQL: &str = "SELECT er.id AS evidence_id, c.id AS control_id, c.code AS control_code, c.title AS control_title, c.description AS control_description, m.rationale, m.created_at FROM evidence er LEFT JOIN evidence_control_mappings m ON m.evidence_id = er.id LEFT JOIN controls c ON c.id = m.control_id AND c.workspace_id = er.workspace_id WHERE er.id = $1 AND er.workspace_id = $2 ORDER BY c.code";
const CONTROL_EVIDENCE_MAPPINGS_SQL: &str = "SELECT e.id, e.workspace_id, e.title, e.description, e.collection_instructions, e.status, e.created_at AS evidence_created_at, e.updated_at, m.rationale, m.created_at AS mapping_created_at FROM evidence_control_mappings m JOIN evidence e ON e.id = m.evidence_id AND e.workspace_id = $2 WHERE m.control_id = $1 ORDER BY e.title, e.id";
const CONTROL_POLICY_MAPPINGS_SQL: &str = "SELECT p.id, p.workspace_id, p.name, p.description, p.created_at AS policy_created_at, p.updated_at, m.created_at AS mapping_created_at FROM policy_control_mappings m JOIN policies p ON p.id = m.policy_id AND p.workspace_id = $2 WHERE m.control_id = $1 AND p.archived_at IS NULL ORDER BY lower(p.name), p.id";

fn control_from_row(row: &Row) -> Result<ControlDetail, Error> {
    Ok(ControlDetail {
        id: row.try_get::<_, Uuid>("id")?.into(),
        workspace_id: row.try_get::<_, Uuid>("workspace_id")?.into(),
        code: row.try_get("code")?,
        title: row.try_get("title")?,
        description: row.try_get("description")?,
        framework_requirements: Vec::new(),
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}
fn controls_from_joined_rows(rows: Vec<Row>) -> Result<Vec<ControlDetail>, Error> {
    let mut controls = Vec::new();
    let mut current = None;
    let mut index = None;
    for row in rows {
        let id = ControlId::from(row.try_get::<_, Uuid>("id")?);
        if current != Some(id) {
            controls.push(control_from_row(&row)?);
            current = Some(id);
            index = controls.len().checked_sub(1);
        }
        if let Some(control) = index.and_then(|i| controls.get_mut(i)) {
            push_requirement(control, &row)?;
        }
    }
    Ok(controls)
}
fn push_requirement(control: &mut ControlDetail, row: &Row) -> Result<(), Error> {
    let Some(id) = row.try_get::<_, Option<Uuid>>("framework_requirement_id")? else {
        return Ok(());
    };
    control
        .framework_requirements
        .push(FrameworkRequirementDetail {
            id: id.into(),
            framework_id: FrameworkId::from(
                row.try_get::<_, Uuid>("framework_requirement_framework_id")?,
            ),
            framework_code: row.try_get("framework_requirement_framework_code")?,
            framework_name: row.try_get("framework_requirement_framework_name")?,
            code: row.try_get("framework_requirement_code")?,
            title: row.try_get("framework_requirement_title")?,
            description: row.try_get("framework_requirement_description")?,
        });
    Ok(())
}
fn evidence_control_mapping_from_row(row: Row) -> Result<EvidenceControlMapping, Error> {
    Ok(EvidenceControlMapping {
        evidence_id: EvidenceId::from(row.try_get::<_, Uuid>("evidence_id")?),
        control: ControlSummary {
            id: ControlId::from(row.try_get::<_, Uuid>("control_id")?),
            code: row.try_get("control_code")?,
            title: row.try_get("control_title")?,
            description: row.try_get("control_description")?,
        },
        rationale: row.try_get("rationale")?,
        created_at: row.try_get("created_at")?,
    })
}
fn evidence_control_mapping_from_joined_row(
    row: Row,
) -> Option<Result<EvidenceControlMapping, Error>> {
    match row.try_get::<_, Option<Uuid>>("control_id") {
        Ok(Some(_)) => Some(evidence_control_mapping_from_row(row)),
        Ok(None) => None,
        Err(error) => Some(Err(Error::Database(error))),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn reverse_mapping_queries_are_read_only_workspace_scoped_and_ordered() {
        assert!(!super::CONTROL_EVIDENCE_MAPPINGS_SQL.contains("UPDATE"));
        assert!(super::CONTROL_EVIDENCE_MAPPINGS_SQL.contains("e.workspace_id = $2"));
        assert!(super::CONTROL_EVIDENCE_MAPPINGS_SQL.contains("ORDER BY e.title, e.id"));
        assert!(!super::CONTROL_POLICY_MAPPINGS_SQL.contains("UPDATE"));
        assert!(super::CONTROL_POLICY_MAPPINGS_SQL.contains("p.workspace_id = $2"));
        assert!(super::CONTROL_POLICY_MAPPINGS_SQL.contains("ORDER BY lower(p.name), p.id"));
    }
}
