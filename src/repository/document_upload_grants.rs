use chrono::{DateTime, Utc};
use tokio_postgres::Row;
use uuid::Uuid;

use crate::{
    domain::{
        AgentConnectionId, CoverageWindow, DocumentUploadGrantId,
        EvidenceDocumentUploadGrant as DomainEvidenceDocumentUploadGrant, EvidenceId, UserId,
        WorkspaceId,
    },
    repository::{TransactionContext, WorkspaceTransactionContext},
};

use super::{
    snapshot::{save_workspace_snapshot, workspace_snapshot_record},
    Error, Postgres,
};

enum SnapshotConnection<'a> {
    Transaction(&'a TransactionContext<'a>),
    Workspace(&'a WorkspaceTransactionContext<'a>),
}

pub struct EvidenceDocumentUploadGrantRepository<'a> {
    connection: SnapshotConnection<'a>,
}

impl<'a> TransactionContext<'a> {
    pub fn evidence_document_upload_grants(&'a self) -> EvidenceDocumentUploadGrantRepository<'a> {
        EvidenceDocumentUploadGrantRepository {
            connection: SnapshotConnection::Transaction(self),
        }
    }
}

impl<'a> WorkspaceTransactionContext<'a> {
    pub fn evidence_document_upload_grants(&'a self) -> EvidenceDocumentUploadGrantRepository<'a> {
        EvidenceDocumentUploadGrantRepository {
            connection: SnapshotConnection::Workspace(self),
        }
    }
}

impl EvidenceDocumentUploadGrantRepository<'_> {
    pub async fn get(
        &self,
        id: DocumentUploadGrantId,
        workspace_id: WorkspaceId,
    ) -> Result<Option<DomainEvidenceDocumentUploadGrant>, Error> {
        let parameters: [&(dyn tokio_postgres::types::ToSql + Sync); 2] =
            [&Uuid::from(id), &Uuid::from(workspace_id)];
        let rows = match self.connection {
            SnapshotConnection::Transaction(context) => {
                context
                    .transaction
                    .query(GET_FOR_UPDATE_SQL, &parameters)
                    .await?
            }
            SnapshotConnection::Workspace(context) => {
                context
                    .transaction
                    .query(GET_FOR_UPDATE_SQL, &parameters)
                    .await?
            }
        };
        rows.into_iter()
            .next()
            .map(|row| EvidenceGrantRecord::try_from(row).and_then(TryInto::try_into))
            .transpose()
    }

    pub async fn save(&self, grant: &DomainEvidenceDocumentUploadGrant) -> Result<(), Error> {
        let transaction = match self.connection {
            SnapshotConnection::Transaction(context) => &context.transaction,
            SnapshotConnection::Workspace(context) => {
                if grant.workspace_id() != context.workspace_id {
                    return Err(Error::InvariantViolation(
                        "evidence human upload grant workspace must match its transaction",
                    ));
                }
                &context.transaction
            }
        };
        let record = EvidenceGrantRecord::from(grant);
        save_workspace_snapshot(transaction, record.as_workspace_snapshot()).await
    }
}

const GET_FOR_UPDATE_SQL: &str = r#"
SELECT id, workspace_id, evidence_id, valid_from, valid_until,
       issued_by_user_id, issued_via_agent_connection_id, issued_at,
       expires_at, redeemed_at
FROM document_upload_grants
WHERE id = $1 AND workspace_id = $2
FOR UPDATE
"#;

workspace_snapshot_record! {
    struct EvidenceGrantRecord {
        id: Uuid,
        workspace_id: Uuid,
        evidence_id: Uuid,
        valid_from: DateTime<Utc>,
        valid_until: DateTime<Utc>,
        issued_by_user_id: Uuid,
        issued_via_agent_connection_id: Uuid,
        issued_at: DateTime<Utc>,
        expires_at: DateTime<Utc>,
        redeemed_at: Option<DateTime<Utc>>,
    }
    table: document_upload_grants,
    conflict: id,
    scope: workspace_id,
}

impl TryFrom<Row> for EvidenceGrantRecord {
    type Error = Error;

    fn try_from(row: Row) -> Result<Self, Self::Error> {
        Ok(Self {
            id: row.try_get("id")?,
            workspace_id: row.try_get("workspace_id")?,
            evidence_id: row.try_get("evidence_id")?,
            valid_from: row.try_get("valid_from")?,
            valid_until: row.try_get("valid_until")?,
            issued_by_user_id: row.try_get("issued_by_user_id")?,
            issued_via_agent_connection_id: row.try_get("issued_via_agent_connection_id")?,
            issued_at: row.try_get("issued_at")?,
            expires_at: row.try_get("expires_at")?,
            redeemed_at: row.try_get("redeemed_at")?,
        })
    }
}

impl TryFrom<EvidenceGrantRecord> for DomainEvidenceDocumentUploadGrant {
    type Error = Error;

    fn try_from(record: EvidenceGrantRecord) -> Result<Self, Self::Error> {
        DomainEvidenceDocumentUploadGrant::rehydrate(
            record.id.into(),
            record.workspace_id.into(),
            record.evidence_id.into(),
            CoverageWindow::new(record.valid_from, record.valid_until)?,
            record.issued_by_user_id.into(),
            record.issued_via_agent_connection_id.into(),
            record.issued_at,
            record.expires_at,
            record.redeemed_at,
        )
        .map_err(|_| Error::InvariantViolation("persisted evidence human upload grant is invalid"))
    }
}

impl From<&DomainEvidenceDocumentUploadGrant> for EvidenceGrantRecord {
    fn from(grant: &DomainEvidenceDocumentUploadGrant) -> Self {
        Self {
            id: grant.id().into(),
            workspace_id: grant.workspace_id().into(),
            evidence_id: grant.evidence_id().into(),
            valid_from: grant.coverage().valid_from,
            valid_until: grant.coverage().valid_until,
            issued_by_user_id: grant.issued_by_user_id().into(),
            issued_via_agent_connection_id: grant.issued_via_agent_connection_id().into(),
            issued_at: grant.issued_at(),
            expires_at: grant.expires_at(),
            redeemed_at: grant.redeemed_at(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NewDocumentUploadGrant {
    pub id: DocumentUploadGrantId,
    pub evidence_id: EvidenceId,
    pub coverage: CoverageWindow,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DocumentUploadGrant {
    pub id: DocumentUploadGrantId,
    pub workspace_id: WorkspaceId,
    pub evidence_id: EvidenceId,
    pub valid_from: DateTime<Utc>,
    pub valid_until: DateTime<Utc>,
    pub issued_by_user_id: UserId,
    pub issued_via_agent_connection_id: Option<AgentConnectionId>,
    pub issued_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub redeemed_at: Option<DateTime<Utc>>,
}

impl WorkspaceTransactionContext<'_> {
    pub async fn create_document_upload_grant(
        &self,
        grant: NewDocumentUploadGrant,
    ) -> Result<Option<DocumentUploadGrant>, Error> {
        let agent_connection_id = self.credential.agent_connection_uuid();
        let rows = self
            .transaction
            .query(
                r#"
WITH scoped_evidence AS (
    SELECT e.id
    FROM evidence e
    WHERE e.id = $2
      AND e.workspace_id = $3
),
inserted AS (
    INSERT INTO document_upload_grants (
        id,
        workspace_id,
        evidence_id,
        valid_from,
        valid_until,
        issued_by_user_id,
        issued_via_agent_connection_id,
        expires_at
    )
    SELECT $1, $3, scoped_evidence.id, $4, $5, $6, $7, $8
    FROM scoped_evidence
    RETURNING
        id,
        workspace_id,
        evidence_id,
        valid_from,
        valid_until,
        issued_by_user_id,
        issued_via_agent_connection_id,
        issued_at,
        expires_at,
        redeemed_at
)
SELECT *
FROM inserted
"#,
                &[
                    &Uuid::from(grant.id),
                    &Uuid::from(grant.evidence_id),
                    &Uuid::from(self.workspace_id),
                    &grant.coverage.valid_from,
                    &grant.coverage.valid_until,
                    &Uuid::from(self.user_id),
                    &agent_connection_id,
                    &grant.expires_at,
                ],
            )
            .await?;

        rows.into_iter()
            .next()
            .map(|row| document_upload_grant_from_row(&row))
            .transpose()
    }
}

impl Postgres {
    pub async fn redeem_document_upload_grant(
        &self,
        grant_id: DocumentUploadGrantId,
        workspace_id: WorkspaceId,
    ) -> Result<Option<DocumentUploadGrant>, Error> {
        let client = self.get().await?;
        let rows = client
            .query(
                r#"
UPDATE document_upload_grants
SET redeemed_at = now()
WHERE id = $1
  AND workspace_id = $2
  AND redeemed_at IS NULL
  AND expires_at > now()
RETURNING
    id,
    workspace_id,
    evidence_id,
    valid_from,
    valid_until,
    issued_by_user_id,
    issued_via_agent_connection_id,
    issued_at,
    expires_at,
    redeemed_at
"#,
                &[&Uuid::from(grant_id), &Uuid::from(workspace_id)],
            )
            .await?;

        rows.into_iter()
            .next()
            .map(|row| document_upload_grant_from_row(&row))
            .transpose()
    }
}

fn document_upload_grant_from_row(row: &Row) -> Result<DocumentUploadGrant, Error> {
    Ok(DocumentUploadGrant {
        id: DocumentUploadGrantId::from(row.try_get::<_, Uuid>("id")?),
        workspace_id: WorkspaceId::from(row.try_get::<_, Uuid>("workspace_id")?),
        evidence_id: EvidenceId::from(row.try_get::<_, Uuid>("evidence_id")?),
        valid_from: row.try_get("valid_from")?,
        valid_until: row.try_get("valid_until")?,
        issued_by_user_id: UserId::from(row.try_get::<_, Uuid>("issued_by_user_id")?),
        issued_via_agent_connection_id: row
            .try_get::<_, Option<Uuid>>("issued_via_agent_connection_id")?
            .map(AgentConnectionId::from),
        issued_at: row.try_get("issued_at")?,
        expires_at: row.try_get("expires_at")?,
        redeemed_at: row.try_get("redeemed_at")?,
    })
}
