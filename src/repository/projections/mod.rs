use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::{
    domain::{
        ControlId, EvidenceId, EvidenceSubmissionId, FrameworkId, PolicyId, UserId, WorkspaceId,
    },
    projections::{
        AuditorAccessGrantSummary, AuditorPortalControl, AuditorPortalPolicy, ControlDetail,
        ControlEvidenceMapping, ControlPolicyMapping, EvidenceControlMapping, EvidenceDetail,
        EvidenceSubmissionDetail, FrameworkDetail, FrameworkRequirementDetail, PolicyCatalogEntry,
        PolicyDetail, PolicySummary, UserAgentConnectionSummary, WorkspaceDetails,
        WorkspaceWithRole,
    },
};

use super::{Error, Postgres, WorkspaceReadContext, WorkspaceTransactionContext};

pub struct FrameworkProjectionRepository<'a> {
    postgres: &'a Postgres,
}

impl Postgres {
    pub fn framework_projections(&self) -> FrameworkProjectionRepository<'_> {
        FrameworkProjectionRepository { postgres: self }
    }

    pub fn workspace_projections(&self) -> WorkspaceProjectionRepository<'_> {
        WorkspaceProjectionRepository { postgres: self }
    }

    pub fn auditor_access_grant_projections(&self) -> AuditorAccessGrantProjectionRepository<'_> {
        AuditorAccessGrantProjectionRepository { postgres: self }
    }

    pub fn agent_connection_projections(&self) -> AgentConnectionProjectionRepository<'_> {
        AgentConnectionProjectionRepository { postgres: self }
    }
}

impl FrameworkProjectionRepository<'_> {
    pub async fn list(&self) -> Result<Vec<FrameworkDetail>, Error> {
        self.postgres.load_frameworks().await
    }

    pub async fn list_requirements(
        &self,
        framework_id: FrameworkId,
    ) -> Result<Vec<FrameworkRequirementDetail>, Error> {
        self.postgres
            .load_framework_requirements(framework_id)
            .await
    }
}

pub struct WorkspaceProjectionRepository<'a> {
    postgres: &'a Postgres,
}

impl WorkspaceProjectionRepository<'_> {
    pub async fn get(&self, id: WorkspaceId) -> Result<Option<WorkspaceDetails>, Error> {
        self.postgres.load_workspace_details(id).await
    }

    pub async fn list(&self) -> Result<Vec<WorkspaceDetails>, Error> {
        self.postgres.load_workspace_details_list().await
    }

    pub async fn get_for_user(&self, user_id: UserId) -> Result<Option<WorkspaceWithRole>, Error> {
        self.postgres
            .load_workspace_with_role_for_user(user_id)
            .await
    }
}

enum WorkspaceProjectionConnection<'a> {
    Read(&'a WorkspaceReadContext),
    Transaction(&'a WorkspaceTransactionContext<'a>),
}

pub struct ControlProjectionRepository<'a> {
    connection: WorkspaceProjectionConnection<'a>,
}

pub struct EvidenceProjectionRepository<'a> {
    connection: WorkspaceProjectionConnection<'a>,
}

pub struct PolicyProjectionRepository<'a> {
    connection: WorkspaceProjectionConnection<'a>,
}

pub struct EvidenceSubmissionProjectionRepository<'a> {
    context: &'a WorkspaceReadContext,
}

pub struct AuditorPortalProjectionRepository<'a> {
    context: &'a WorkspaceReadContext,
}

impl<'a> WorkspaceReadContext {
    pub fn control_projections(&'a self) -> ControlProjectionRepository<'a> {
        ControlProjectionRepository {
            connection: WorkspaceProjectionConnection::Read(self),
        }
    }

    pub fn evidence_projections(&'a self) -> EvidenceProjectionRepository<'a> {
        EvidenceProjectionRepository {
            connection: WorkspaceProjectionConnection::Read(self),
        }
    }

    pub fn policy_projections(&'a self) -> PolicyProjectionRepository<'a> {
        PolicyProjectionRepository {
            connection: WorkspaceProjectionConnection::Read(self),
        }
    }

    pub fn evidence_submission_projections(&'a self) -> EvidenceSubmissionProjectionRepository<'a> {
        EvidenceSubmissionProjectionRepository { context: self }
    }

    pub fn auditor_portal_projections(&'a self) -> AuditorPortalProjectionRepository<'a> {
        AuditorPortalProjectionRepository { context: self }
    }
}

impl<'a> WorkspaceTransactionContext<'a> {
    pub fn control_projections(&'a self) -> ControlProjectionRepository<'a> {
        ControlProjectionRepository {
            connection: WorkspaceProjectionConnection::Transaction(self),
        }
    }

    pub fn evidence_projections(&'a self) -> EvidenceProjectionRepository<'a> {
        EvidenceProjectionRepository {
            connection: WorkspaceProjectionConnection::Transaction(self),
        }
    }

    pub fn policy_projections(&'a self) -> PolicyProjectionRepository<'a> {
        PolicyProjectionRepository {
            connection: WorkspaceProjectionConnection::Transaction(self),
        }
    }
}

impl ControlProjectionRepository<'_> {
    pub async fn get(&self, id: ControlId) -> Result<Option<ControlDetail>, Error> {
        match self.connection {
            WorkspaceProjectionConnection::Read(context) => context.load_control_detail(id).await,
            WorkspaceProjectionConnection::Transaction(context) => {
                context.load_control_detail(id).await
            }
        }
    }

    pub async fn list(&self) -> Result<Vec<ControlDetail>, Error> {
        match self.connection {
            WorkspaceProjectionConnection::Read(context) => context.load_control_details().await,
            WorkspaceProjectionConnection::Transaction(_) => Err(Error::InvariantViolation(
                "control projection lists require a read context",
            )),
        }
    }

    pub async fn list_evidence_mappings(
        &self,
        evidence_id: EvidenceId,
    ) -> Result<Option<Vec<EvidenceControlMapping>>, Error> {
        match self.connection {
            WorkspaceProjectionConnection::Read(context) => {
                context.load_evidence_control_mappings(evidence_id).await
            }
            WorkspaceProjectionConnection::Transaction(_) => Err(Error::InvariantViolation(
                "control mapping projections require a read context",
            )),
        }
    }

    pub async fn get_evidence_mapping(
        &self,
        evidence_id: EvidenceId,
        control_id: ControlId,
    ) -> Result<Option<EvidenceControlMapping>, Error> {
        match self.connection {
            WorkspaceProjectionConnection::Transaction(context) => {
                context
                    .load_evidence_control_mapping(evidence_id, control_id)
                    .await
            }
            WorkspaceProjectionConnection::Read(_) => Err(Error::InvariantViolation(
                "transactional control mapping projection requires a transaction",
            )),
        }
    }

    pub async fn list_evidence_for_control(
        &self,
        control_id: ControlId,
    ) -> Result<Option<Vec<ControlEvidenceMapping>>, Error> {
        let WorkspaceProjectionConnection::Read(context) = self.connection else {
            return Err(Error::InvariantViolation(
                "reverse evidence projections require a read context",
            ));
        };
        if context
            .client
            .query_opt(
                "SELECT 1 FROM controls WHERE id = $1 AND workspace_id = $2",
                &[&Uuid::from(control_id), &Uuid::from(context.workspace_id)],
            )
            .await?
            .is_none()
        {
            return Ok(None);
        }
        context
            .client
            .query(
                CONTROL_EVIDENCE_MAPPINGS_SQL,
                &[&Uuid::from(control_id), &Uuid::from(context.workspace_id)],
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
        let WorkspaceProjectionConnection::Read(context) = self.connection else {
            return Err(Error::InvariantViolation(
                "reverse policy projections require a read context",
            ));
        };
        if context
            .client
            .query_opt(
                "SELECT 1 FROM controls WHERE id = $1 AND workspace_id = $2",
                &[&Uuid::from(control_id), &Uuid::from(context.workspace_id)],
            )
            .await?
            .is_none()
        {
            return Ok(None);
        }
        context
            .client
            .query(
                CONTROL_POLICY_MAPPINGS_SQL,
                &[&Uuid::from(control_id), &Uuid::from(context.workspace_id)],
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

const CONTROL_EVIDENCE_MAPPINGS_SQL: &str = r#"
SELECT e.id, e.workspace_id, e.title, e.description, e.collection_instructions,
       e.status, e.created_at AS evidence_created_at, e.updated_at,
       m.rationale, m.created_at AS mapping_created_at
FROM evidence_control_mappings m
JOIN evidence e ON e.id = m.evidence_id AND e.workspace_id = $2
WHERE m.control_id = $1
ORDER BY e.title, e.id
"#;

const CONTROL_POLICY_MAPPINGS_SQL: &str = "SELECT p.id, p.workspace_id, p.name, p.description, p.created_at AS policy_created_at, p.updated_at, m.created_at AS mapping_created_at FROM policy_control_mappings m JOIN policies p ON p.id = m.policy_id AND p.workspace_id = $2 WHERE m.control_id = $1 AND p.archived_at IS NULL ORDER BY lower(p.name), p.id";

impl EvidenceProjectionRepository<'_> {
    pub async fn get(&self, id: EvidenceId) -> Result<Option<EvidenceDetail>, Error> {
        match self.connection {
            WorkspaceProjectionConnection::Read(context) => context.load_evidence_detail(id).await,
            WorkspaceProjectionConnection::Transaction(context) => {
                context.load_evidence_detail(id).await
            }
        }
    }

    pub async fn list(&self) -> Result<Vec<EvidenceDetail>, Error> {
        match self.connection {
            WorkspaceProjectionConnection::Read(context) => context.load_evidence_details().await,
            WorkspaceProjectionConnection::Transaction(_) => Err(Error::InvariantViolation(
                "evidence projection lists require a read context",
            )),
        }
    }
}

impl PolicyProjectionRepository<'_> {
    pub async fn get(&self, id: PolicyId) -> Result<Option<PolicyDetail>, Error> {
        match self.connection {
            WorkspaceProjectionConnection::Read(context) => context.load_policy_detail(id).await,
            WorkspaceProjectionConnection::Transaction(context) => {
                context.load_policy_detail(id).await
            }
        }
    }

    pub async fn list_catalog(&self) -> Result<Vec<PolicyCatalogEntry>, Error> {
        match self.connection {
            WorkspaceProjectionConnection::Read(context) => context.load_policy_catalog().await,
            WorkspaceProjectionConnection::Transaction(_) => Err(Error::InvariantViolation(
                "policy catalog projections require a read context",
            )),
        }
    }

    pub async fn get_control_mapping(
        &self,
        policy_id: PolicyId,
        control_id: ControlId,
    ) -> Result<Option<crate::projections::PolicyControlMapping>, Error> {
        match self.connection {
            WorkspaceProjectionConnection::Transaction(context) => {
                context
                    .load_policy_control_mapping(policy_id, control_id)
                    .await
            }
            WorkspaceProjectionConnection::Read(_) => Err(Error::InvariantViolation(
                "transactional policy mapping projection requires a transaction",
            )),
        }
    }
}

impl EvidenceSubmissionProjectionRepository<'_> {
    pub async fn get(
        &self,
        id: EvidenceSubmissionId,
    ) -> Result<Option<EvidenceSubmissionDetail>, Error> {
        self.context.load_evidence_submission_detail(id).await
    }

    pub async fn list_for_evidence(
        &self,
        evidence_id: EvidenceId,
    ) -> Result<Vec<EvidenceSubmissionDetail>, Error> {
        self.context
            .load_evidence_submission_details(evidence_id)
            .await
    }

    pub async fn list_for_coverage(
        &self,
        evidence_id: EvidenceId,
        coverage: crate::domain::CoverageWindow,
    ) -> Result<Vec<EvidenceSubmissionDetail>, Error> {
        self.context
            .load_evidence_submission_details_for_coverage(evidence_id, coverage)
            .await
    }

    pub async fn latest_for_evidence(
        &self,
        evidence_id: EvidenceId,
    ) -> Result<Option<EvidenceSubmissionDetail>, Error> {
        self.context
            .load_latest_evidence_submission_detail(evidence_id)
            .await
    }
}

impl AuditorPortalProjectionRepository<'_> {
    pub async fn controls(
        &self,
        period_start: DateTime<Utc>,
        period_end: DateTime<Utc>,
    ) -> Result<Vec<AuditorPortalControl>, Error> {
        self.context
            .load_auditor_portal_controls(period_start, period_end)
            .await
    }

    pub async fn policies(&self) -> Result<Vec<AuditorPortalPolicy>, Error> {
        self.context.load_auditor_portal_policies().await
    }
}

pub struct AuditorAccessGrantProjectionRepository<'a> {
    postgres: &'a Postgres,
}

impl AuditorAccessGrantProjectionRepository<'_> {
    pub async fn list(
        &self,
        workspace_id: WorkspaceId,
    ) -> Result<Vec<AuditorAccessGrantSummary>, Error> {
        self.postgres
            .load_auditor_access_grant_summaries(workspace_id)
            .await
    }
}

pub struct AgentConnectionProjectionRepository<'a> {
    postgres: &'a Postgres,
}

impl AgentConnectionProjectionRepository<'_> {
    pub async fn list_for_user(
        &self,
        user_id: UserId,
    ) -> Result<Vec<UserAgentConnectionSummary>, Error> {
        self.postgres
            .load_user_agent_connection_summaries(user_id)
            .await
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
