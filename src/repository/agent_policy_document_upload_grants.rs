use chrono::{DateTime, Utc};
use tokio_postgres::Row;
use uuid::Uuid;

use crate::domain::{
    AgentConnectionId, AgentPolicyDocumentUploadDeclaration, AgentPolicyDocumentUploadGrant,
    AgentPolicyDocumentUploadGrantId, DocumentId, Sha256Digest, WorkspaceId,
};

use super::{
    snapshot::{save_workspace_snapshot, workspace_snapshot_record},
    Error, Postgres, WorkspaceTransactionContext,
};

enum RepositoryConnection<'a> {
    Postgres(&'a Postgres),
    Transaction(&'a WorkspaceTransactionContext<'a>),
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

impl<'a> WorkspaceTransactionContext<'a> {
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
            RepositoryConnection::Transaction(context) => {
                context
                    .transaction
                    .query(GET_FOR_UPDATE_SQL, &parameters)
                    .await?
            }
        };
        rows.into_iter()
            .next()
            .map(|row| {
                GrantRecord::try_from(row).and_then(AgentPolicyDocumentUploadGrant::try_from)
            })
            .transpose()
    }

    /// Persists the aggregate's complete current snapshot.
    pub async fn save(&self, grant: &AgentPolicyDocumentUploadGrant) -> Result<(), Error> {
        let RepositoryConnection::Transaction(context) = self.connection else {
            return Err(Error::InvariantViolation(
                "policy machine upload grants must be saved in a workspace transaction",
            ));
        };
        if grant.workspace_id() != context.workspace_id {
            return Err(Error::InvariantViolation(
                "policy machine upload grant workspace must match its transaction",
            ));
        }
        let record = GrantRecord::try_from(grant)?;
        save_workspace_snapshot(&context.transaction, record.as_workspace_snapshot()).await
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

workspace_snapshot_record! {
    struct GrantRecord {
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
    scope: workspace_id,
}

#[cfg(test)]
mod tests {
    use super::{GET_FOR_UPDATE_SQL, GET_SQL};

    #[test]
    fn verification_and_transactional_reads_have_distinct_locking_sql() {
        assert!(!GET_SQL.contains("FOR UPDATE"));
        assert!(GET_FOR_UPDATE_SQL.contains("FOR UPDATE"));
    }
}

impl TryFrom<Row> for GrantRecord {
    type Error = Error;

    fn try_from(row: Row) -> Result<Self, Self::Error> {
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
}

impl TryFrom<GrantRecord> for AgentPolicyDocumentUploadGrant {
    type Error = Error;

    fn try_from(record: GrantRecord) -> Result<Self, Self::Error> {
        let expected_content_length =
            u64::try_from(record.expected_content_length).map_err(|_| {
                Error::InvariantViolation("persisted policy machine upload length is negative")
            })?;
        let expected_sha256 = record
            .expected_sha256
            .map(|bytes| {
                bytes.try_into().map(Sha256Digest::from_bytes).map_err(|_| {
                    Error::InvariantViolation("persisted policy machine upload SHA-256 is invalid")
                })
            })
            .transpose()?;
        let declaration = AgentPolicyDocumentUploadDeclaration::rehydrate(
            record.filename,
            record.content_type,
            expected_content_length,
            expected_sha256,
        )
        .map_err(|_| {
            Error::InvariantViolation("persisted policy machine upload declaration is invalid")
        })?;
        AgentPolicyDocumentUploadGrant::rehydrate(
            record.id.into(),
            record.workspace_id.into(),
            record.policy_id.into(),
            declaration,
            record.issued_by_user_id.into(),
            AgentConnectionId::from(record.issued_via_agent_connection_id),
            record.issued_at,
            record.expires_at,
            record.completed_at,
            record.document_id.map(DocumentId::from),
        )
        .map_err(|_| {
            Error::InvariantViolation("persisted policy machine upload grant is inconsistent")
        })
    }
}

impl TryFrom<&AgentPolicyDocumentUploadGrant> for GrantRecord {
    type Error = Error;

    fn try_from(grant: &AgentPolicyDocumentUploadGrant) -> Result<Self, Self::Error> {
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
