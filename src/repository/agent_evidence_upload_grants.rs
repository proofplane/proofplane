use chrono::{DateTime, Utc};
use tokio_postgres::Row;
use uuid::Uuid;

use crate::domain::{
    AgentConnectionId, AgentEvidenceUploadDeclaration, AgentEvidenceUploadGrant,
    AgentEvidenceUploadGrantId, CoverageWindow, DocumentId, Sha256Digest, WorkspaceId,
};

use super::{
    snapshot::{save_workspace_snapshot, workspace_snapshot_record},
    Error, Postgres, WorkspaceRepositories,
};

enum RepositoryConnection<'a> {
    Postgres(&'a Postgres),
    Transaction(&'a WorkspaceRepositories<'a>),
}

/// Persistence boundary for the machine-upload grant aggregate.
///
/// A transaction-backed instance keeps `FOR UPDATE` locks until its surrounding
/// workspace transaction commits. A Postgres-backed verification read uses an
/// autocommit statement, so its lock is released immediately.
pub struct AgentEvidenceUploadGrantRepository<'a> {
    connection: RepositoryConnection<'a>,
}

impl Postgres {
    pub fn agent_evidence_upload_grants(&self) -> AgentEvidenceUploadGrantRepository<'_> {
        AgentEvidenceUploadGrantRepository {
            connection: RepositoryConnection::Postgres(self),
        }
    }
}

impl<'a> WorkspaceRepositories<'a> {
    pub fn agent_evidence_upload_grants(&'a self) -> AgentEvidenceUploadGrantRepository<'a> {
        AgentEvidenceUploadGrantRepository {
            connection: RepositoryConnection::Transaction(self),
        }
    }
}

impl AgentEvidenceUploadGrantRepository<'_> {
    pub async fn get(
        &self,
        upload_id: AgentEvidenceUploadGrantId,
        workspace_id: WorkspaceId,
    ) -> Result<Option<AgentEvidenceUploadGrant>, Error> {
        let parameters: [&(dyn tokio_postgres::types::ToSql + Sync); 2] =
            [&Uuid::from(upload_id), &Uuid::from(workspace_id)];
        let rows = match self.connection {
            RepositoryConnection::Postgres(postgres) => {
                postgres.get().await?.query(GET_SQL, &parameters).await?
            }
            RepositoryConnection::Transaction(context) => {
                context
                    .transaction
                    .query(GET_FOR_UPDATE_SQL, &parameters)
                    .await?
            }
        };
        rows.into_iter()
            .next()
            .map(|row| GrantRecord::try_from(row).and_then(AgentEvidenceUploadGrant::try_from))
            .transpose()
    }

    /// Persists the aggregate's complete current snapshot.
    pub async fn save(&self, grant: &AgentEvidenceUploadGrant) -> Result<(), Error> {
        let RepositoryConnection::Transaction(context) = self.connection else {
            return Err(Error::InvariantViolation(
                "machine upload grants must be saved in a workspace transaction",
            ));
        };
        if grant.workspace_id() != context.workspace_id {
            return Err(Error::InvariantViolation(
                "machine upload grant workspace must match its repository scope",
            ));
        }
        let record = GrantRecord::try_from(grant)?;
        save_workspace_snapshot(context.transaction, record.as_workspace_snapshot()).await
    }
}

const GET_SQL: &str = r#"
SELECT
    id, submission_id, workspace_id, evidence_id, valid_from, valid_until,
    filename, content_type, expected_content_length, expected_sha256,
    issued_by_user_id, issued_via_agent_connection_id, issued_at, expires_at,
    completed_at, document_id
FROM agent_evidence_upload_grants
WHERE id = $1 AND workspace_id = $2
"#;

const GET_FOR_UPDATE_SQL: &str = concat!(
    r#"
SELECT
    id, submission_id, workspace_id, evidence_id, valid_from, valid_until,
    filename, content_type, expected_content_length, expected_sha256,
    issued_by_user_id, issued_via_agent_connection_id, issued_at, expires_at,
    completed_at, document_id
FROM agent_evidence_upload_grants
WHERE id = $1 AND workspace_id = $2
"#,
    "FOR UPDATE"
);

workspace_snapshot_record! {
    struct GrantRecord {
        id: Uuid,
        submission_id: Uuid,
        workspace_id: Uuid,
        evidence_id: Uuid,
        valid_from: DateTime<Utc>,
        valid_until: DateTime<Utc>,
        filename: String,
        content_type: String,
        expected_content_length: i64,
        expected_sha256: Option<Vec<u8>>,
        issued_by_user_id: Uuid,
        issued_via_agent_connection_id: Uuid,
        issued_at: DateTime<Utc>,
        expires_at: DateTime<Utc>,
        completed_at: Option<DateTime<Utc>>,
        document_id: Option<Uuid>,
    }
    table: agent_evidence_upload_grants,
    conflict: id,
    scope: workspace_id,
}

impl TryFrom<Row> for GrantRecord {
    type Error = Error;

    fn try_from(row: Row) -> Result<Self, Self::Error> {
        Ok(Self {
            id: row.try_get("id")?,
            submission_id: row.try_get("submission_id")?,
            workspace_id: row.try_get("workspace_id")?,
            evidence_id: row.try_get("evidence_id")?,
            valid_from: row.try_get("valid_from")?,
            valid_until: row.try_get("valid_until")?,
            filename: row.try_get("filename")?,
            content_type: row.try_get("content_type")?,
            expected_content_length: row.try_get("expected_content_length")?,
            expected_sha256: row.try_get("expected_sha256")?,
            issued_by_user_id: row.try_get("issued_by_user_id")?,
            issued_via_agent_connection_id: row.try_get("issued_via_agent_connection_id")?,
            issued_at: row.try_get("issued_at")?,
            expires_at: row.try_get("expires_at")?,
            completed_at: row.try_get("completed_at")?,
            document_id: row.try_get("document_id")?,
        })
    }
}

impl TryFrom<GrantRecord> for AgentEvidenceUploadGrant {
    type Error = Error;

    fn try_from(record: GrantRecord) -> Result<Self, Self::Error> {
        let expected_content_length =
            u64::try_from(record.expected_content_length).map_err(|_| {
                Error::InvariantViolation("persisted machine upload length is negative")
            })?;
        let expected_sha256 = record
            .expected_sha256
            .map(|bytes| {
                bytes.try_into().map(Sha256Digest::from_bytes).map_err(|_| {
                    Error::InvariantViolation("persisted machine upload SHA-256 is invalid")
                })
            })
            .transpose()?;
        let declaration = AgentEvidenceUploadDeclaration::rehydrate(
            record.filename,
            record.content_type,
            expected_content_length,
            expected_sha256,
        )
        .map_err(|_| {
            Error::InvariantViolation("persisted machine upload declaration is invalid")
        })?;
        AgentEvidenceUploadGrant::rehydrate(
            record.id.into(),
            record.submission_id.into(),
            record.workspace_id.into(),
            record.evidence_id.into(),
            CoverageWindow::new(record.valid_from, record.valid_until)?,
            declaration,
            record.issued_by_user_id.into(),
            AgentConnectionId::from(record.issued_via_agent_connection_id),
            record.issued_at,
            record.expires_at,
            record.completed_at,
            record.document_id.map(DocumentId::from),
        )
        .map_err(|_| Error::InvariantViolation("persisted machine upload grant is inconsistent"))
    }
}

impl TryFrom<&AgentEvidenceUploadGrant> for GrantRecord {
    type Error = Error;

    fn try_from(grant: &AgentEvidenceUploadGrant) -> Result<Self, Self::Error> {
        Ok(Self {
            id: grant.id().into(),
            submission_id: grant.submission_id().into(),
            workspace_id: grant.workspace_id().into(),
            evidence_id: grant.evidence_id().into(),
            valid_from: grant.coverage().valid_from,
            valid_until: grant.coverage().valid_until,
            filename: grant.declaration().filename().to_owned(),
            content_type: grant.declaration().content_type().to_owned(),
            expected_content_length: i64::try_from(grant.declaration().expected_content_length())
                .map_err(|_| {
                Error::InvariantViolation("machine upload length exceeds Postgres BIGINT")
            })?,
            expected_sha256: grant
                .declaration()
                .expected_sha256()
                .map(|digest| digest.as_bytes().to_vec()),
            issued_by_user_id: grant.issued_by_user_id().into(),
            issued_via_agent_connection_id: grant.issued_via_agent_connection_id().into(),
            issued_at: grant.issued_at(),
            expires_at: grant.expires_at(),
            completed_at: grant.completed_at(),
            document_id: grant.document_id().map(Uuid::from),
        })
    }
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, TimeZone};

    use crate::{
        domain::{AgentEvidenceUploadGrantId, EvidenceId, EvidenceSubmissionId, Sha256Digest},
        repository::test_support::{self, TestWorkspace},
    };

    use super::*;

    /// Deterministic so that two constructions compare equal: the aggregate is
    /// `Eq` but not `Clone`, and the collision test needs an untouched copy of
    /// what it saved.
    fn grant(
        workspace: &TestWorkspace,
        evidence_id: EvidenceId,
        id: AgentEvidenceUploadGrantId,
        submission_id: EvidenceSubmissionId,
    ) -> AgentEvidenceUploadGrant {
        let issued_at = Utc.with_ymd_and_hms(2026, 7, 31, 12, 0, 0).unwrap();

        AgentEvidenceUploadGrant::issue(
            id,
            submission_id,
            workspace.workspace_id,
            evidence_id,
            CoverageWindow::new(issued_at, issued_at + Duration::days(1))
                .expect("coverage window is valid"),
            AgentEvidenceUploadDeclaration::new(
                "evidence.pdf".to_owned(),
                "application/pdf".to_owned(),
                3,
                Some(hex::encode(Sha256Digest::digest(b"abc").as_bytes())),
                100,
            )
            .into_result()
            .expect("declaration is valid"),
            workspace.user_id,
            workspace.agent_connection_id,
            issued_at,
            issued_at + Duration::minutes(5),
        )
        .expect("machine grant issues")
    }

    async fn save(
        postgres: &Postgres,
        workspace: &TestWorkspace,
        grant: AgentEvidenceUploadGrant,
    ) -> Result<(), Error> {
        postgres
            .in_unit_of_work(async move |unit_of_work| {
                let workspace_repositories = unit_of_work.for_workspace(workspace.workspace_id);
                let context = &workspace_repositories;
                let repository = context.agent_evidence_upload_grants();
                repository.save(&grant).await
            })
            .await
    }

    #[test]
    fn verification_and_transactional_reads_have_distinct_locking_sql() {
        assert!(!GET_SQL.contains("FOR UPDATE"));
        assert!(GET_FOR_UPDATE_SQL.contains("FOR UPDATE"));
    }

    #[tokio::test]
    async fn reads_are_scoped_to_the_owning_workspace_and_round_trip_the_full_snapshot() {
        let postgres = test_support::database().await;
        let owner = test_support::workspace(&postgres, "Owner").await;
        let other = test_support::workspace(&postgres, "Other").await;
        let evidence_id =
            test_support::evidence(&postgres, owner.workspace_id, "Access review").await;
        let grant_id = AgentEvidenceUploadGrantId::from(Uuid::new_v4());
        let grant = grant(
            &owner,
            evidence_id,
            grant_id,
            EvidenceSubmissionId::from(Uuid::new_v4()),
        );
        let owner_workspace_id = owner.workspace_id;

        let round_tripped = postgres
            .in_unit_of_work(async move |unit_of_work| {
                let workspace = unit_of_work.for_workspace(owner.workspace_id);
                let context = &workspace;
                let repository = context.agent_evidence_upload_grants();
                repository.save(&grant).await?;
                let reloaded = repository.get(grant_id, owner_workspace_id).await?;

                Ok(reloaded == Some(grant))
            })
            .await
            .expect("full-snapshot save completes");
        assert!(round_tripped);

        assert!(postgres
            .agent_evidence_upload_grants()
            .get(grant_id, other.workspace_id)
            .await
            .expect("tenant-scoped lookup succeeds")
            .is_none());
    }

    #[tokio::test]
    async fn save_does_not_overwrite_a_same_id_grant_in_another_workspace() {
        let postgres = test_support::database().await;
        let owner = test_support::workspace(&postgres, "Owner").await;
        let intruder = test_support::workspace(&postgres, "Intruder").await;
        let owner_evidence =
            test_support::evidence(&postgres, owner.workspace_id, "Access review").await;
        let intruder_evidence =
            test_support::evidence(&postgres, intruder.workspace_id, "Access review").await;
        let grant_id = AgentEvidenceUploadGrantId::from(Uuid::new_v4());
        let submission_id = EvidenceSubmissionId::from(Uuid::new_v4());

        save(
            &postgres,
            &owner,
            grant(&owner, owner_evidence, grant_id, submission_id),
        )
        .await
        .expect("owner grant saves");

        let collision = save(
            &postgres,
            &intruder,
            grant(
                &intruder,
                intruder_evidence,
                grant_id,
                EvidenceSubmissionId::from(Uuid::new_v4()),
            ),
        )
        .await;
        assert!(matches!(collision, Err(Error::InvariantViolation(_))));

        assert_eq!(
            postgres
                .agent_evidence_upload_grants()
                .get(grant_id, owner.workspace_id)
                .await
                .expect("owner grant loads"),
            Some(grant(&owner, owner_evidence, grant_id, submission_id))
        );
    }

    #[tokio::test]
    async fn save_requires_a_workspace_transaction() {
        let postgres = test_support::database().await;
        let owner = test_support::workspace(&postgres, "Owner").await;
        let evidence_id =
            test_support::evidence(&postgres, owner.workspace_id, "Access review").await;
        let grant = grant(
            &owner,
            evidence_id,
            AgentEvidenceUploadGrantId::from(Uuid::new_v4()),
            EvidenceSubmissionId::from(Uuid::new_v4()),
        );

        let result = postgres.agent_evidence_upload_grants().save(&grant).await;

        assert!(matches!(result, Err(Error::InvariantViolation(_))));
    }
}
