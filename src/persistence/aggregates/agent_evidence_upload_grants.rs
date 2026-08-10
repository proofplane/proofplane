use chrono::{DateTime, Utc};
use tokio_postgres::Row;
use uuid::Uuid;

use crate::domain::{
    AgentConnectionId, AgentEvidenceUploadDeclaration, AgentEvidenceUploadGrant,
    AgentEvidenceUploadGrantId, CoverageWindow, DocumentId, Sha256Digest, WorkspaceId,
};

use super::{
    snapshot::{save_snapshot, snapshot_record},
    Error, Postgres, WorkspaceUnitOfWork,
};

enum RepositoryConnection<'a> {
    Postgres(&'a Postgres),
    Transaction(&'a WorkspaceUnitOfWork<'a>),
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

impl<'a> WorkspaceUnitOfWork<'a> {
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
            RepositoryConnection::Transaction(workspace) => {
                workspace
                    .transaction
                    .query(GET_FOR_UPDATE_SQL, &parameters)
                    .await?
            }
        };
        rows.into_iter()
            .next()
            .map(|row| AgentEvidenceUploadGrantRecord::try_from_row(&row)?.into_domain())
            .transpose()
    }

    /// Persists the aggregate's complete current snapshot.
    pub async fn save(&self, grant: &AgentEvidenceUploadGrant) -> Result<(), Error> {
        let RepositoryConnection::Transaction(workspace) = self.connection else {
            return Err(Error::InvariantViolation(
                "machine upload grants must be saved in a workspace transaction",
            ));
        };
        let record = AgentEvidenceUploadGrantRecord::from_domain(grant)?;
        save_snapshot(workspace.transaction, record.as_snapshot()).await
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

snapshot_record! {
    struct AgentEvidenceUploadGrantRecord {
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
}

impl AgentEvidenceUploadGrantRecord {
    fn try_from_row(row: &Row) -> Result<Self, Error> {
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
    fn into_domain(self) -> Result<AgentEvidenceUploadGrant, Error> {
        let expected_content_length =
            u64::try_from(self.expected_content_length).map_err(|_| {
                Error::InvariantViolation("persisted machine upload length is negative")
            })?;
        let expected_sha256 = self
            .expected_sha256
            .map(|bytes| {
                bytes.try_into().map(Sha256Digest::from_bytes).map_err(|_| {
                    Error::InvariantViolation("persisted machine upload SHA-256 is invalid")
                })
            })
            .transpose()?;
        let declaration = AgentEvidenceUploadDeclaration::rehydrate(
            self.filename,
            self.content_type,
            expected_content_length,
            expected_sha256,
        )
        .map_err(|_| {
            Error::InvariantViolation("persisted machine upload declaration is invalid")
        })?;
        AgentEvidenceUploadGrant::rehydrate(
            self.id.into(),
            self.submission_id.into(),
            self.workspace_id.into(),
            self.evidence_id.into(),
            CoverageWindow::new(self.valid_from, self.valid_until)?,
            declaration,
            self.issued_by_user_id.into(),
            AgentConnectionId::from(self.issued_via_agent_connection_id),
            self.issued_at,
            self.expires_at,
            self.completed_at,
            self.document_id.map(DocumentId::from),
        )
        .map_err(|_| Error::InvariantViolation("persisted machine upload grant is inconsistent"))
    }
    fn from_domain(grant: &AgentEvidenceUploadGrant) -> Result<Self, Error> {
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
        persistence::test_support::{self, TestWorkspace},
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
                let workspace = unit_of_work.workspace(owner.workspace_id);
                let repository = workspace.agent_evidence_upload_grants();
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
