use std::collections::HashSet;

use uuid::Uuid;

use crate::{
    domain::{FrameworkId, FrameworkRequirementId},
    persistence::Error,
    read_models::{FrameworkDetail, FrameworkRequirementDetail},
};

use super::{ReadExecutor, TransactionalReadExecutor};

pub(crate) struct FrameworkReads<'a, E> {
    executor: &'a E,
}

impl<'a, E> FrameworkReads<'a, E> {
    pub(crate) fn new(executor: &'a E) -> Self {
        Self { executor }
    }
}

impl<E: ReadExecutor> FrameworkReads<'_, E> {
    pub async fn list(&self) -> Result<Vec<FrameworkDetail>, Error> {
        self.executor
            .query(
                "SELECT id, code, name, description FROM frameworks ORDER BY code",
                &[],
            )
            .await?
            .into_iter()
            .map(|row| {
                Ok(FrameworkDetail {
                    id: FrameworkId::from(row.try_get::<_, Uuid>("id")?),
                    code: row.try_get("code")?,
                    name: row.try_get("name")?,
                    description: row.try_get("description")?,
                })
            })
            .collect()
    }

    pub async fn list_requirements(
        &self,
        framework_id: FrameworkId,
    ) -> Result<Vec<FrameworkRequirementDetail>, Error> {
        self.executor
            .query(
                r#"
SELECT fr.id, fr.framework_id, f.code AS framework_code,
       f.name AS framework_name, fr.code, fr.title, fr.description
FROM framework_requirements fr
JOIN frameworks f ON f.id = fr.framework_id
WHERE fr.framework_id = $1
ORDER BY fr.code
"#,
                &[&Uuid::from(framework_id)],
            )
            .await?
            .into_iter()
            .map(|row| {
                Ok(FrameworkRequirementDetail {
                    id: FrameworkRequirementId::from(row.try_get::<_, Uuid>("id")?),
                    framework_id: FrameworkId::from(row.try_get::<_, Uuid>("framework_id")?),
                    framework_code: row.try_get("framework_code")?,
                    framework_name: row.try_get("framework_name")?,
                    code: row.try_get("code")?,
                    title: row.try_get("title")?,
                    description: row.try_get("description")?,
                })
            })
            .collect()
    }
}

impl FrameworkReads<'_, TransactionalReadExecutor<'_>> {
    pub async fn requirements_exist(&self, ids: &[FrameworkRequirementId]) -> Result<bool, Error> {
        if ids.is_empty() {
            return Ok(true);
        }
        let requested = ids.iter().copied().map(Uuid::from).collect::<HashSet<_>>();
        let found = self
            .executor
            .query_one(
                "SELECT count(DISTINCT id) AS found FROM framework_requirements WHERE id = ANY($1)",
                &[&requested.iter().copied().collect::<Vec<_>>()],
            )
            .await?
            .try_get::<_, i64>("found")?;
        Ok(found == requested.len() as i64)
    }
}
