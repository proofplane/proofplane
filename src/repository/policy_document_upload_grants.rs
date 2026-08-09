use chrono::{DateTime, Utc};
use tokio_postgres::Row;
use uuid::Uuid;

use crate::{
    domain::{
        PolicyDocumentUploadGrant as DomainPolicyDocumentUploadGrant, PolicyDocumentUploadGrantId,
        WorkspaceId,
    },
    repository::{TransactionContext, WorkspaceTransactionContext},
};

use super::{
    snapshot::{save_workspace_snapshot, workspace_snapshot_record},
    Error,
};

enum SnapshotConnection<'a> {
    Transaction(&'a TransactionContext<'a>),
    Workspace(&'a WorkspaceTransactionContext<'a>),
}

pub struct PolicyDocumentUploadGrantRepository<'a> {
    connection: SnapshotConnection<'a>,
}

impl<'a> TransactionContext<'a> {
    pub fn policy_document_upload_grants(&'a self) -> PolicyDocumentUploadGrantRepository<'a> {
        PolicyDocumentUploadGrantRepository {
            connection: SnapshotConnection::Transaction(self),
        }
    }
}

impl<'a> WorkspaceTransactionContext<'a> {
    pub fn policy_document_upload_grants(&'a self) -> PolicyDocumentUploadGrantRepository<'a> {
        PolicyDocumentUploadGrantRepository {
            connection: SnapshotConnection::Workspace(self),
        }
    }
}

impl PolicyDocumentUploadGrantRepository<'_> {
    pub async fn get(
        &self,
        id: PolicyDocumentUploadGrantId,
        workspace_id: WorkspaceId,
    ) -> Result<Option<DomainPolicyDocumentUploadGrant>, Error> {
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
            .map(|row| PolicyGrantRecord::try_from(row).and_then(TryInto::try_into))
            .transpose()
    }

    pub async fn save(&self, grant: &DomainPolicyDocumentUploadGrant) -> Result<(), Error> {
        let transaction = match self.connection {
            SnapshotConnection::Transaction(context) => &context.transaction,
            SnapshotConnection::Workspace(context) => {
                if grant.workspace_id() != context.workspace_id {
                    return Err(Error::InvariantViolation(
                        "policy human upload grant workspace must match its transaction",
                    ));
                }
                &context.transaction
            }
        };
        let record = PolicyGrantRecord::from(grant);
        save_workspace_snapshot(transaction, record.as_workspace_snapshot()).await
    }
}

const GET_FOR_UPDATE_SQL: &str = r#"
SELECT id, workspace_id, policy_id, issued_by_user_id,
       issued_via_agent_connection_id, issued_at, expires_at, redeemed_at
FROM policy_document_upload_grants
WHERE id = $1 AND workspace_id = $2
FOR UPDATE
"#;

workspace_snapshot_record! {
    struct PolicyGrantRecord {
        id: Uuid,
        workspace_id: Uuid,
        policy_id: Uuid,
        issued_by_user_id: Uuid,
        issued_via_agent_connection_id: Uuid,
        issued_at: DateTime<Utc>,
        expires_at: DateTime<Utc>,
        redeemed_at: Option<DateTime<Utc>>,
    }
    table: policy_document_upload_grants,
    conflict: id,
    scope: workspace_id,
}

impl TryFrom<Row> for PolicyGrantRecord {
    type Error = Error;

    fn try_from(row: Row) -> Result<Self, Self::Error> {
        Ok(Self {
            id: row.try_get("id")?,
            workspace_id: row.try_get("workspace_id")?,
            policy_id: row.try_get("policy_id")?,
            issued_by_user_id: row.try_get("issued_by_user_id")?,
            issued_via_agent_connection_id: row.try_get("issued_via_agent_connection_id")?,
            issued_at: row.try_get("issued_at")?,
            expires_at: row.try_get("expires_at")?,
            redeemed_at: row.try_get("redeemed_at")?,
        })
    }
}

impl TryFrom<PolicyGrantRecord> for DomainPolicyDocumentUploadGrant {
    type Error = Error;

    fn try_from(record: PolicyGrantRecord) -> Result<Self, Self::Error> {
        DomainPolicyDocumentUploadGrant::rehydrate(
            record.id.into(),
            record.workspace_id.into(),
            record.policy_id.into(),
            record.issued_by_user_id.into(),
            record.issued_via_agent_connection_id.into(),
            record.issued_at,
            record.expires_at,
            record.redeemed_at,
        )
        .map_err(|_| Error::InvariantViolation("persisted policy human upload grant is invalid"))
    }
}

impl From<&DomainPolicyDocumentUploadGrant> for PolicyGrantRecord {
    fn from(grant: &DomainPolicyDocumentUploadGrant) -> Self {
        Self {
            id: grant.id().into(),
            workspace_id: grant.workspace_id().into(),
            policy_id: grant.policy_id().into(),
            issued_by_user_id: grant.issued_by_user_id().into(),
            issued_via_agent_connection_id: grant.issued_via_agent_connection_id().into(),
            issued_at: grant.issued_at(),
            expires_at: grant.expires_at(),
            redeemed_at: grant.redeemed_at(),
        }
    }
}
