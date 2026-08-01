use chrono::{DateTime, Utc};
use tokio_postgres::Row;
use uuid::Uuid;

use crate::domain::{
    AgentConnectionId, AgentEvidenceUploadDeclaration, AgentEvidenceUploadGrant,
    AgentEvidenceUploadGrantId, CoverageWindow, DocumentId, Sha256Digest, WorkspaceId,
};

use super::{
    snapshot::{save_workspace_snapshot, workspace_snapshot_record},
    Error, Postgres, WorkspaceTransactionContext,
};

enum RepositoryConnection<'a> {
    Postgres(&'a Postgres),
    Transaction(&'a WorkspaceTransactionContext<'a>),
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

impl<'a> WorkspaceTransactionContext<'a> {
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
                context.transaction.query(GET_SQL, &parameters).await?
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
                "machine upload grant workspace must match its transaction",
            ));
        }
        let record = GrantRecord::try_from(grant)?;
        save_workspace_snapshot(&context.transaction, record.as_workspace_snapshot()).await
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
FOR UPDATE
"#;

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
