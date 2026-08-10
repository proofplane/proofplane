use chrono::{DateTime, Utc};
use tokio_postgres::Row;
use uuid::Uuid;

use crate::{
    domain::{
        CoverageWindow, DocumentUploadGrantId,
        EvidenceDocumentUploadGrant as DomainEvidenceDocumentUploadGrant, WorkspaceId,
    },
    persistence::{UnitOfWork, WorkspaceUnitOfWork},
};

use super::{
    snapshot::{save_snapshot, snapshot_record},
    Error,
};

enum SnapshotConnection<'a> {
    Transaction(&'a UnitOfWork<'a>),
    Workspace(&'a WorkspaceUnitOfWork<'a>),
}

pub struct EvidenceDocumentUploadGrantRepository<'a> {
    connection: SnapshotConnection<'a>,
}

impl<'a> UnitOfWork<'a> {
    pub fn evidence_document_upload_grants(&'a self) -> EvidenceDocumentUploadGrantRepository<'a> {
        EvidenceDocumentUploadGrantRepository {
            connection: SnapshotConnection::Transaction(self),
        }
    }
}

impl<'a> WorkspaceUnitOfWork<'a> {
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
            SnapshotConnection::Transaction(unit_of_work) => {
                unit_of_work
                    .transaction
                    .query(GET_FOR_UPDATE_SQL, &parameters)
                    .await?
            }
            SnapshotConnection::Workspace(workspace) => {
                workspace
                    .transaction
                    .query(GET_FOR_UPDATE_SQL, &parameters)
                    .await?
            }
        };
        rows.into_iter()
            .next()
            .map(|row| EvidenceDocumentUploadGrantRecord::try_from_row(&row)?.into_domain())
            .transpose()
    }

    pub async fn save(&self, grant: &DomainEvidenceDocumentUploadGrant) -> Result<(), Error> {
        let transaction = match self.connection {
            SnapshotConnection::Transaction(unit_of_work) => &unit_of_work.transaction,
            SnapshotConnection::Workspace(workspace) => workspace.transaction,
        };
        let record = EvidenceDocumentUploadGrantRecord::from_domain(grant)?;
        save_snapshot(transaction, record.as_snapshot()).await
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

snapshot_record! {
    struct EvidenceDocumentUploadGrantRecord {
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
}

impl EvidenceDocumentUploadGrantRecord {
    fn try_from_row(row: &Row) -> Result<Self, Error> {
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
    fn from_domain(grant: &DomainEvidenceDocumentUploadGrant) -> Result<Self, Error> {
        Ok(Self {
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
        })
    }

    fn into_domain(self) -> Result<DomainEvidenceDocumentUploadGrant, Error> {
        DomainEvidenceDocumentUploadGrant::rehydrate(
            self.id.into(),
            self.workspace_id.into(),
            self.evidence_id.into(),
            CoverageWindow::new(self.valid_from, self.valid_until)?,
            self.issued_by_user_id.into(),
            self.issued_via_agent_connection_id.into(),
            self.issued_at,
            self.expires_at,
            self.redeemed_at,
        )
        .map_err(|_| Error::InvariantViolation("persisted evidence human upload grant is invalid"))
    }
}
