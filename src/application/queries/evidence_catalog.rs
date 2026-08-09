use std::sync::Arc;

use chrono::{DateTime, Utc};

use crate::{
    authentication::AgentConnectionContext,
    domain::{ControlId, Evidence, EvidenceControlMapping, EvidenceId, WorkspacePermission},
    repository::{Error as RepositoryError, Postgres},
};

#[derive(Debug, Clone, Copy)]
pub struct ListEvidence {
    pub connection: AgentConnectionContext,
}

#[derive(Clone)]
pub struct ListEvidenceHandler {
    repository: Arc<Postgres>,
}

impl ListEvidenceHandler {
    pub fn new(repository: Arc<Postgres>) -> Self {
        Self { repository }
    }

    pub async fn handle(&self, query: ListEvidence) -> Result<Vec<Evidence>, EvidenceCatalogError> {
        authorize_evidence(query.connection)?;
        Ok(self
            .repository
            .in_workspace_context_read(query.connection.workspace_id, async |context| {
                context.list_evidence().await
            })
            .await?)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct GetEvidence {
    pub connection: AgentConnectionContext,
    pub evidence_id: EvidenceId,
}

#[derive(Clone)]
pub struct GetEvidenceHandler {
    repository: Arc<Postgres>,
}

impl GetEvidenceHandler {
    pub fn new(repository: Arc<Postgres>) -> Self {
        Self { repository }
    }

    pub async fn handle(
        &self,
        query: GetEvidence,
    ) -> Result<Option<Evidence>, EvidenceCatalogError> {
        authorize_evidence(query.connection)?;
        Ok(self
            .repository
            .in_workspace_context_read(query.connection.workspace_id, async move |context| {
                context.get_evidence(query.evidence_id).await
            })
            .await?)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ListEvidenceControlMappings {
    pub connection: AgentConnectionContext,
    pub evidence_id: EvidenceId,
}

#[derive(Clone)]
pub struct ListEvidenceControlMappingsHandler {
    repository: Arc<Postgres>,
}

impl ListEvidenceControlMappingsHandler {
    pub fn new(repository: Arc<Postgres>) -> Self {
        Self { repository }
    }

    pub async fn handle(
        &self,
        query: ListEvidenceControlMappings,
    ) -> Result<Option<Vec<EvidenceControlMapping>>, EvidenceCatalogError> {
        if !query
            .connection
            .permissions
            .has(WorkspacePermission::ReadControls)
        {
            return Err(EvidenceCatalogError::Unavailable);
        }
        Ok(self
            .repository
            .in_workspace_context_read(query.connection.workspace_id, async move |context| {
                context
                    .list_evidence_control_mappings(query.evidence_id)
                    .await
            })
            .await?)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ListControlEvidenceMappings {
    pub connection: AgentConnectionContext,
    pub control_id: ControlId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlEvidenceMappingProjection {
    pub evidence: Evidence,
    pub rationale: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone)]
pub struct ListControlEvidenceMappingsHandler {
    repository: Arc<Postgres>,
}

impl ListControlEvidenceMappingsHandler {
    pub fn new(repository: Arc<Postgres>) -> Self {
        Self { repository }
    }

    pub async fn handle(
        &self,
        query: ListControlEvidenceMappings,
    ) -> Result<Option<Vec<ControlEvidenceMappingProjection>>, EvidenceCatalogError> {
        if !query
            .connection
            .permissions
            .has(WorkspacePermission::ReadControls)
        {
            return Err(EvidenceCatalogError::Unavailable);
        }
        let client = self.repository.get().await.map_err(RepositoryError::from)?;
        if client
            .query_opt(
                "SELECT 1 FROM controls WHERE id = $1 AND workspace_id = $2",
                &[
                    &uuid::Uuid::from(query.control_id),
                    &uuid::Uuid::from(query.connection.workspace_id),
                ],
            )
            .await
            .map_err(RepositoryError::from)?
            .is_none()
        {
            return Ok(None);
        }
        client
            .query(
                REVERSE_MAPPINGS_SQL,
                &[
                    &uuid::Uuid::from(query.control_id),
                    &uuid::Uuid::from(query.connection.workspace_id),
                ],
            )
            .await
            .map_err(RepositoryError::from)?
            .into_iter()
            .map(|row| {
                Ok(ControlEvidenceMappingProjection {
                    evidence: Evidence {
                        id: row.try_get::<_, uuid::Uuid>("id")?.into(),
                        workspace_id: row.try_get::<_, uuid::Uuid>("workspace_id")?.into(),
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
            .collect::<Result<Vec<_>, RepositoryError>>()
            .map(Some)
            .map_err(Into::into)
    }
}

const REVERSE_MAPPINGS_SQL: &str = r#"
SELECT e.id, e.workspace_id, e.title, e.description, e.collection_instructions,
       e.status, e.created_at AS evidence_created_at, e.updated_at,
       m.rationale, m.created_at AS mapping_created_at
FROM evidence_control_mappings m
JOIN evidence e ON e.id = m.evidence_id AND e.workspace_id = $2
WHERE m.control_id = $1
ORDER BY e.title, e.id
"#;

fn authorize_evidence(connection: AgentConnectionContext) -> Result<(), EvidenceCatalogError> {
    if connection
        .permissions
        .has(WorkspacePermission::ReadEvidence)
    {
        Ok(())
    } else {
        Err(EvidenceCatalogError::Unavailable)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum EvidenceCatalogError {
    #[error("evidence is unavailable")]
    Unavailable,
    #[error("repository error")]
    Repository(#[from] RepositoryError),
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use deadpool_postgres::{Manager, Pool};
    use tokio_postgres::{Config, NoTls};
    use uuid::Uuid;

    use crate::{
        authentication::AgentConnectionContext,
        domain::{AgentConnectionId, EvidenceId, UserId, WorkspaceId, WorkspacePermissions},
        repository::Postgres,
    };

    use super::{
        EvidenceCatalogError, GetEvidence, GetEvidenceHandler, ListEvidence, ListEvidenceHandler,
    };

    #[tokio::test]
    async fn evidence_catalog_queries_conceal_data_without_read_evidence_permission() {
        let connection = connection();
        let list = ListEvidenceHandler::new(repository())
            .handle(ListEvidence { connection })
            .await;
        let detail = GetEvidenceHandler::new(repository())
            .handle(GetEvidence {
                connection,
                evidence_id: EvidenceId::from(Uuid::new_v4()),
            })
            .await;

        assert!(matches!(list, Err(EvidenceCatalogError::Unavailable)));
        assert!(matches!(detail, Err(EvidenceCatalogError::Unavailable)));
    }

    #[test]
    fn reverse_mapping_query_is_read_only_workspace_scoped_and_ordered() {
        assert!(!super::REVERSE_MAPPINGS_SQL.contains("UPDATE"));
        assert!(super::REVERSE_MAPPINGS_SQL.contains("e.workspace_id = $2"));
        assert!(super::REVERSE_MAPPINGS_SQL.contains("ORDER BY e.title, e.id"));
    }

    fn repository() -> Arc<Postgres> {
        let pool = Pool::builder(Manager::new(Config::new(), NoTls))
            .build()
            .unwrap();
        Arc::new(Postgres::new(pool))
    }

    fn connection() -> AgentConnectionContext {
        AgentConnectionContext {
            user_id: UserId::from(Uuid::new_v4()),
            connection_id: AgentConnectionId::from(Uuid::new_v4()),
            workspace_id: WorkspaceId::from(Uuid::new_v4()),
            permissions: WorkspacePermissions::none(),
        }
    }
}
