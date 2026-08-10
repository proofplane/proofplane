use chrono::{DateTime, Utc};
use tokio_postgres::Row;
use uuid::Uuid;

use crate::domain::{
    AgentConnectionId, AgentPolicyDocumentUploadDeclaration, AgentPolicyDocumentUploadGrant,
    AgentPolicyDocumentUploadGrantId, DocumentId, Sha256Digest, WorkspaceId,
};

use super::{
    snapshot::{save_snapshot, snapshot_record},
    Error, Postgres, WorkspaceUnitOfWork,
};

enum RepositoryConnection<'a> {
    Postgres(&'a Postgres),
    Transaction(&'a WorkspaceUnitOfWork<'a>),
}

/// Persistence boundary for the policy machine-upload grant aggregate.
///
/// A transaction-backed instance keeps `FOR UPDATE` locks until its surrounding
/// workspace transaction commits. A Postgres-backed verification read uses an
/// autocommit statement, so its lock is released immediately.
pub struct AgentPolicyDocumentUploadGrantRepository<'a> {
    connection: RepositoryConnection<'a>,
}

impl Postgres {
    pub fn agent_policy_document_upload_grants(
        &self,
    ) -> AgentPolicyDocumentUploadGrantRepository<'_> {
        AgentPolicyDocumentUploadGrantRepository {
            connection: RepositoryConnection::Postgres(self),
        }
    }
}

impl<'a> WorkspaceUnitOfWork<'a> {
    pub fn agent_policy_document_upload_grants(
        &'a self,
    ) -> AgentPolicyDocumentUploadGrantRepository<'a> {
        AgentPolicyDocumentUploadGrantRepository {
            connection: RepositoryConnection::Transaction(self),
        }
    }
}

impl AgentPolicyDocumentUploadGrantRepository<'_> {
    pub async fn get(
        &self,
        upload_id: AgentPolicyDocumentUploadGrantId,
        workspace_id: WorkspaceId,
    ) -> Result<Option<AgentPolicyDocumentUploadGrant>, Error> {
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
            .map(|row| AgentPolicyDocumentUploadGrantRecord::try_from_row(&row)?.into_domain())
            .transpose()
    }

    /// Persists the aggregate's complete current snapshot.
    pub async fn save(&self, grant: &AgentPolicyDocumentUploadGrant) -> Result<(), Error> {
        let RepositoryConnection::Transaction(workspace) = self.connection else {
            return Err(Error::InvariantViolation(
                "policy machine upload grants must be saved in a workspace transaction",
            ));
        };
        let record = AgentPolicyDocumentUploadGrantRecord::from_domain(grant)?;
        save_snapshot(workspace.transaction, record.as_snapshot()).await
    }
}

const GET_SQL: &str = r#"
SELECT
    id, workspace_id, policy_id, filename, content_type,
    expected_content_length, expected_sha256, issued_by_user_id,
    issued_via_agent_connection_id, issued_at, expires_at, completed_at, document_id
FROM agent_policy_document_upload_grants
WHERE id = $1 AND workspace_id = $2
"#;

const GET_FOR_UPDATE_SQL: &str = concat!(
    r#"
SELECT
    id, workspace_id, policy_id, filename, content_type,
    expected_content_length, expected_sha256, issued_by_user_id,
    issued_via_agent_connection_id, issued_at, expires_at, completed_at, document_id
FROM agent_policy_document_upload_grants
WHERE id = $1 AND workspace_id = $2
"#,
    "FOR UPDATE"
);

snapshot_record! {
    struct AgentPolicyDocumentUploadGrantRecord {
        id: Uuid,
        workspace_id: Uuid,
        policy_id: Uuid,
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
    table: agent_policy_document_upload_grants,
    conflict: id,
}

impl AgentPolicyDocumentUploadGrantRecord {
    fn try_from_row(row: &Row) -> Result<Self, Error> {
        Ok(Self {
            id: row.try_get("id")?,
            workspace_id: row.try_get("workspace_id")?,
            policy_id: row.try_get("policy_id")?,
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
    fn into_domain(self) -> Result<AgentPolicyDocumentUploadGrant, Error> {
        let expected_content_length =
            u64::try_from(self.expected_content_length).map_err(|_| {
                Error::InvariantViolation("persisted policy machine upload length is negative")
            })?;
        let expected_sha256 = self
            .expected_sha256
            .map(|bytes| {
                bytes.try_into().map(Sha256Digest::from_bytes).map_err(|_| {
                    Error::InvariantViolation("persisted policy machine upload SHA-256 is invalid")
                })
            })
            .transpose()?;
        let declaration = AgentPolicyDocumentUploadDeclaration::rehydrate(
            self.filename,
            self.content_type,
            expected_content_length,
            expected_sha256,
        )
        .map_err(|_| {
            Error::InvariantViolation("persisted policy machine upload declaration is invalid")
        })?;
        AgentPolicyDocumentUploadGrant::rehydrate(
            self.id.into(),
            self.workspace_id.into(),
            self.policy_id.into(),
            declaration,
            self.issued_by_user_id.into(),
            AgentConnectionId::from(self.issued_via_agent_connection_id),
            self.issued_at,
            self.expires_at,
            self.completed_at,
            self.document_id.map(DocumentId::from),
        )
        .map_err(|_| {
            Error::InvariantViolation("persisted policy machine upload grant is inconsistent")
        })
    }
    fn from_domain(grant: &AgentPolicyDocumentUploadGrant) -> Result<Self, Error> {
        Ok(Self {
            id: grant.id().into(),
            workspace_id: grant.workspace_id().into(),
            policy_id: grant.policy_id().into(),
            filename: grant.declaration().filename().to_owned(),
            content_type: grant.declaration().content_type().to_owned(),
            expected_content_length: i64::try_from(grant.declaration().expected_content_length())
                .map_err(|_| {
                Error::InvariantViolation("policy machine upload length exceeds Postgres BIGINT")
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
        domain::PolicyId,
        persistence::test_support::{self, TestWorkspace},
    };

    use super::*;

    /// Deterministic so that two constructions compare equal: the aggregate is
    /// `Eq` but not `Clone`, and the collision test needs an untouched copy of
    /// what it saved.
    fn grant(
        workspace: &TestWorkspace,
        policy_id: PolicyId,
        id: AgentPolicyDocumentUploadGrantId,
    ) -> AgentPolicyDocumentUploadGrant {
        let issued_at = Utc.with_ymd_and_hms(2026, 7, 31, 12, 0, 0).unwrap();

        AgentPolicyDocumentUploadGrant::issue(
            id,
            workspace.workspace_id,
            policy_id,
            AgentPolicyDocumentUploadDeclaration::new(
                "policy.pdf".to_owned(),
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
        .expect("policy machine grant issues")
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
        let policy_id = test_support::policy(&postgres, owner.workspace_id, "Access control").await;
        let grant_id = AgentPolicyDocumentUploadGrantId::from(Uuid::new_v4());
        let grant = grant(&owner, policy_id, grant_id);
        let owner_workspace_id = owner.workspace_id;

        let round_tripped = postgres
            .in_unit_of_work(async move |unit_of_work| {
                let workspace = unit_of_work.workspace(owner.workspace_id);
                let repository = workspace.agent_policy_document_upload_grants();
                repository.save(&grant).await?;
                let reloaded = repository.get(grant_id, owner_workspace_id).await?;

                Ok(reloaded == Some(grant))
            })
            .await
            .expect("full-snapshot save completes");
        assert!(round_tripped);

        assert!(postgres
            .agent_policy_document_upload_grants()
            .get(grant_id, other.workspace_id)
            .await
            .expect("tenant-scoped lookup succeeds")
            .is_none());
    }
}
