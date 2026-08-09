use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::{
    domain::{
        ControlId, Document, DocumentId, EvidenceId, EvidenceSubmissionId, FrameworkId, PolicyId,
        UserId, WorkspaceId,
    },
    projections::{
        AuditorAccessGrantSummary, AuditorPortalControl, AuditorPortalPolicy, ControlDetail,
        ControlEvidenceMapping, ControlPolicyMapping, DocumentDownloadCandidate,
        EvidenceControlMapping, EvidenceDetail, EvidenceSubmissionDetail, FrameworkDetail,
        FrameworkRequirementDetail, PolicyCatalogEntry, PolicyDetail, PolicySummary,
        UserAgentConnectionSummary, WorkspaceDetails, WorkspaceWithRole,
    },
};

use super::{Error, Postgres, WorkspaceRepositories};

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

    pub fn control_projections(
        &self,
        workspace_id: WorkspaceId,
    ) -> ControlProjectionRepository<'_> {
        ControlProjectionRepository {
            postgres: self,
            workspace_id,
        }
    }

    pub fn evidence_projections(
        &self,
        workspace_id: WorkspaceId,
    ) -> EvidenceProjectionRepository<'_> {
        EvidenceProjectionRepository {
            postgres: self,
            workspace_id,
        }
    }

    pub fn policy_projections(&self, workspace_id: WorkspaceId) -> PolicyProjectionRepository<'_> {
        PolicyProjectionRepository {
            postgres: self,
            workspace_id,
        }
    }

    pub fn evidence_submission_projections(
        &self,
        workspace_id: WorkspaceId,
    ) -> EvidenceSubmissionProjectionRepository<'_> {
        EvidenceSubmissionProjectionRepository {
            postgres: self,
            workspace_id,
        }
    }

    pub fn auditor_portal_projections(
        &self,
        workspace_id: WorkspaceId,
    ) -> AuditorPortalProjectionRepository<'_> {
        AuditorPortalProjectionRepository {
            postgres: self,
            workspace_id,
        }
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

pub struct ControlProjectionRepository<'a> {
    postgres: &'a Postgres,
    workspace_id: WorkspaceId,
}

pub struct EvidenceProjectionRepository<'a> {
    postgres: &'a Postgres,
    workspace_id: WorkspaceId,
}

pub struct PolicyProjectionRepository<'a> {
    postgres: &'a Postgres,
    workspace_id: WorkspaceId,
}

pub struct TransactionalControlProjectionRepository<'a> {
    workspace: &'a WorkspaceRepositories<'a>,
}

pub struct TransactionalEvidenceProjectionRepository<'a> {
    workspace: &'a WorkspaceRepositories<'a>,
}

pub struct TransactionalPolicyProjectionRepository<'a> {
    workspace: &'a WorkspaceRepositories<'a>,
}

pub struct EvidenceSubmissionProjectionRepository<'a> {
    postgres: &'a Postgres,
    workspace_id: WorkspaceId,
}

pub struct AuditorPortalProjectionRepository<'a> {
    postgres: &'a Postgres,
    workspace_id: WorkspaceId,
}

impl<'a> WorkspaceRepositories<'a> {
    pub fn control_projections(&'a self) -> TransactionalControlProjectionRepository<'a> {
        TransactionalControlProjectionRepository { workspace: self }
    }

    pub fn evidence_projections(&'a self) -> TransactionalEvidenceProjectionRepository<'a> {
        TransactionalEvidenceProjectionRepository { workspace: self }
    }

    pub fn policy_projections(&'a self) -> TransactionalPolicyProjectionRepository<'a> {
        TransactionalPolicyProjectionRepository { workspace: self }
    }
}

impl ControlProjectionRepository<'_> {
    pub async fn get(&self, id: ControlId) -> Result<Option<ControlDetail>, Error> {
        self.postgres
            .with_workspace_client(self.workspace_id, async move |client| {
                client.load_control_detail(id).await
            })
            .await
    }

    pub async fn list(&self) -> Result<Vec<ControlDetail>, Error> {
        self.postgres
            .with_workspace_client(self.workspace_id, async |client| {
                client.load_control_details().await
            })
            .await
    }

    pub async fn list_evidence_mappings(
        &self,
        evidence_id: EvidenceId,
    ) -> Result<Option<Vec<EvidenceControlMapping>>, Error> {
        self.postgres
            .with_workspace_client(self.workspace_id, async move |client| {
                client.load_evidence_control_mappings(evidence_id).await
            })
            .await
    }

    pub async fn list_evidence_for_control(
        &self,
        control_id: ControlId,
    ) -> Result<Option<Vec<ControlEvidenceMapping>>, Error> {
        let client = self.postgres.get().await?;
        if client
            .query_opt(
                "SELECT 1 FROM controls WHERE id = $1 AND workspace_id = $2",
                &[&Uuid::from(control_id), &Uuid::from(self.workspace_id)],
            )
            .await?
            .is_none()
        {
            return Ok(None);
        }
        client
            .query(
                CONTROL_EVIDENCE_MAPPINGS_SQL,
                &[&Uuid::from(control_id), &Uuid::from(self.workspace_id)],
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
        let client = self.postgres.get().await?;
        if client
            .query_opt(
                "SELECT 1 FROM controls WHERE id = $1 AND workspace_id = $2",
                &[&Uuid::from(control_id), &Uuid::from(self.workspace_id)],
            )
            .await?
            .is_none()
        {
            return Ok(None);
        }
        client
            .query(
                CONTROL_POLICY_MAPPINGS_SQL,
                &[&Uuid::from(control_id), &Uuid::from(self.workspace_id)],
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

impl TransactionalControlProjectionRepository<'_> {
    pub async fn get(&self, id: ControlId) -> Result<Option<ControlDetail>, Error> {
        self.workspace.load_control_detail(id).await
    }

    pub async fn get_evidence_mapping(
        &self,
        evidence_id: EvidenceId,
        control_id: ControlId,
    ) -> Result<Option<EvidenceControlMapping>, Error> {
        self.workspace
            .load_evidence_control_mapping(evidence_id, control_id)
            .await
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
        self.postgres
            .with_workspace_client(self.workspace_id, async move |client| {
                client.load_evidence_detail(id).await
            })
            .await
    }

    pub async fn list(&self) -> Result<Vec<EvidenceDetail>, Error> {
        self.postgres
            .with_workspace_client(self.workspace_id, async |client| {
                client.load_evidence_details().await
            })
            .await
    }
}

impl TransactionalEvidenceProjectionRepository<'_> {
    pub async fn get(&self, id: EvidenceId) -> Result<Option<EvidenceDetail>, Error> {
        self.workspace.load_evidence_detail(id).await
    }
}

impl PolicyProjectionRepository<'_> {
    pub async fn get(&self, id: PolicyId) -> Result<Option<PolicyDetail>, Error> {
        self.postgres
            .with_workspace_client(self.workspace_id, async move |client| {
                client.load_policy_detail(id).await
            })
            .await
    }

    pub async fn list_catalog(&self) -> Result<Vec<PolicyCatalogEntry>, Error> {
        self.postgres
            .with_workspace_client(self.workspace_id, async |client| {
                client.load_policy_catalog().await
            })
            .await
    }

    pub async fn get_agent_upload_document(
        &self,
        policy_id: PolicyId,
        document_id: DocumentId,
    ) -> Result<Option<Document>, Error> {
        self.postgres
            .with_workspace_client(self.workspace_id, async move |context| {
                context
                    .get_policy_document_for_agent_upload(policy_id, document_id)
                    .await
            })
            .await
    }

    pub async fn get_document_for_download(
        &self,
        policy_id: PolicyId,
        document_id: DocumentId,
    ) -> Result<Option<DocumentDownloadCandidate>, Error> {
        self.postgres
            .with_workspace_client(self.workspace_id, async move |context| {
                context
                    .get_policy_document_for_download(policy_id, document_id)
                    .await
            })
            .await
    }

    pub async fn get_current_document(
        &self,
        policy_id: PolicyId,
    ) -> Result<Option<Document>, Error> {
        self.postgres
            .with_workspace_client(self.workspace_id, async move |context| {
                context.get_current_policy_document(policy_id).await
            })
            .await
    }
}

impl TransactionalPolicyProjectionRepository<'_> {
    pub async fn get(&self, id: PolicyId) -> Result<Option<PolicyDetail>, Error> {
        self.workspace.load_policy_detail(id).await
    }

    pub async fn get_control_mapping(
        &self,
        policy_id: PolicyId,
        control_id: ControlId,
    ) -> Result<Option<crate::projections::PolicyControlMapping>, Error> {
        self.workspace
            .load_policy_control_mapping(policy_id, control_id)
            .await
    }
}

impl EvidenceSubmissionProjectionRepository<'_> {
    pub async fn get_agent_upload_document(
        &self,
        submission_id: EvidenceSubmissionId,
        document_id: DocumentId,
    ) -> Result<Option<Document>, Error> {
        self.postgres
            .with_workspace_client(self.workspace_id, async move |context| {
                context
                    .get_agent_upload_document(submission_id, document_id)
                    .await
            })
            .await
    }

    pub async fn get_document_for_download(
        &self,
        submission_id: EvidenceSubmissionId,
        document_id: DocumentId,
    ) -> Result<Option<DocumentDownloadCandidate>, Error> {
        self.postgres
            .with_workspace_client(self.workspace_id, async move |context| {
                context
                    .get_document_for_download_grant(submission_id, document_id)
                    .await
            })
            .await
    }

    pub async fn get_document_for_download_within_period(
        &self,
        submission_id: EvidenceSubmissionId,
        document_id: DocumentId,
        period_start: DateTime<Utc>,
        period_end: DateTime<Utc>,
    ) -> Result<Option<DocumentDownloadCandidate>, Error> {
        self.postgres
            .with_workspace_client(self.workspace_id, async move |context| {
                context
                    .get_document_for_download_grant_within_period(
                        submission_id,
                        document_id,
                        period_start,
                        period_end,
                    )
                    .await
            })
            .await
    }

    pub async fn get(
        &self,
        id: EvidenceSubmissionId,
    ) -> Result<Option<EvidenceSubmissionDetail>, Error> {
        self.postgres
            .with_workspace_client(self.workspace_id, async move |context| {
                context.load_evidence_submission_detail(id).await
            })
            .await
    }

    pub async fn list_for_evidence(
        &self,
        evidence_id: EvidenceId,
    ) -> Result<Vec<EvidenceSubmissionDetail>, Error> {
        self.postgres
            .with_workspace_client(self.workspace_id, async move |context| {
                context.load_evidence_submission_details(evidence_id).await
            })
            .await
    }

    pub async fn list_for_coverage(
        &self,
        evidence_id: EvidenceId,
        coverage: crate::domain::CoverageWindow,
    ) -> Result<Vec<EvidenceSubmissionDetail>, Error> {
        self.postgres
            .with_workspace_client(self.workspace_id, async move |context| {
                context
                    .load_evidence_submission_details_for_coverage(evidence_id, coverage)
                    .await
            })
            .await
    }

    pub async fn latest_for_evidence(
        &self,
        evidence_id: EvidenceId,
    ) -> Result<Option<EvidenceSubmissionDetail>, Error> {
        self.postgres
            .with_workspace_client(self.workspace_id, async move |context| {
                context
                    .load_latest_evidence_submission_detail(evidence_id)
                    .await
            })
            .await
    }
}

impl AuditorPortalProjectionRepository<'_> {
    pub async fn controls(
        &self,
        period_start: DateTime<Utc>,
        period_end: DateTime<Utc>,
    ) -> Result<Vec<AuditorPortalControl>, Error> {
        self.postgres
            .with_workspace_client(self.workspace_id, async move |context| {
                context
                    .load_auditor_portal_controls(period_start, period_end)
                    .await
            })
            .await
    }

    pub async fn policies(&self) -> Result<Vec<AuditorPortalPolicy>, Error> {
        self.postgres
            .with_workspace_client(self.workspace_id, async |context| {
                context.load_auditor_portal_policies().await
            })
            .await
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
