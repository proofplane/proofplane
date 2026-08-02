use chrono::{DateTime, Utc};
use tokio_postgres::Row;
use uuid::Uuid;

use crate::{
    domain::{
        AgentConnectionId, PolicyDocumentUploadGrant as DomainPolicyDocumentUploadGrant,
        PolicyDocumentUploadGrantId, PolicyId, UserId, WorkspaceId,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NewPolicyDocumentUploadGrant {
    pub id: PolicyDocumentUploadGrantId,
    pub policy_id: PolicyId,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PolicyDocumentUploadGrant {
    pub id: PolicyDocumentUploadGrantId,
    pub workspace_id: WorkspaceId,
    pub policy_id: PolicyId,
    pub issued_by_user_id: UserId,
    pub issued_via_agent_connection_id: AgentConnectionId,
    pub issued_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub redeemed_at: Option<DateTime<Utc>>,
}

impl WorkspaceTransactionContext<'_> {
    pub async fn create_policy_document_upload_grant(
        &self,
        grant: NewPolicyDocumentUploadGrant,
    ) -> Result<Option<PolicyDocumentUploadGrant>, Error> {
        let agent_connection_id = self.credential.agent_connection_uuid();
        let rows = self
            .transaction
            .query(
                r#"
WITH scoped_policy AS (
    SELECT id
    FROM policies
    WHERE id = $2
      AND workspace_id = $3
      AND archived_at IS NULL
),
inserted AS (
    INSERT INTO policy_document_upload_grants (
        id,
        workspace_id,
        policy_id,
        issued_by_user_id,
        issued_via_agent_connection_id,
        expires_at
    )
    SELECT $1, $3, scoped_policy.id, $4, $5, $6
    FROM scoped_policy
    RETURNING
        id,
        workspace_id,
        policy_id,
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
                    &Uuid::from(grant.policy_id),
                    &Uuid::from(self.workspace_id),
                    &Uuid::from(self.user_id),
                    &agent_connection_id,
                    &grant.expires_at,
                ],
            )
            .await?;

        rows.into_iter()
            .next()
            .map(|row| policy_document_upload_grant_from_row(&row))
            .transpose()
    }
}

impl Postgres {
    pub async fn redeem_policy_document_upload_grant(
        &self,
        grant_id: PolicyDocumentUploadGrantId,
        workspace_id: WorkspaceId,
        policy_id: PolicyId,
    ) -> Result<Option<PolicyDocumentUploadGrant>, Error> {
        let client = self.get().await?;
        let rows = client
            .query(
                r#"
UPDATE policy_document_upload_grants AS g
SET redeemed_at = now()
FROM policies AS policy
WHERE g.id = $1
  AND g.workspace_id = $2
  AND g.policy_id = $3
  AND g.redeemed_at IS NULL
  AND g.expires_at > now()
  AND policy.id = g.policy_id
  AND policy.workspace_id = g.workspace_id
  AND policy.archived_at IS NULL
RETURNING
    g.id,
    g.workspace_id,
    g.policy_id,
    g.issued_by_user_id,
    g.issued_via_agent_connection_id,
    g.issued_at,
    g.expires_at,
    g.redeemed_at
"#,
                &[
                    &Uuid::from(grant_id),
                    &Uuid::from(workspace_id),
                    &Uuid::from(policy_id),
                ],
            )
            .await?;

        rows.into_iter()
            .next()
            .map(|row| policy_document_upload_grant_from_row(&row))
            .transpose()
    }
}

fn policy_document_upload_grant_from_row(row: &Row) -> Result<PolicyDocumentUploadGrant, Error> {
    Ok(PolicyDocumentUploadGrant {
        id: PolicyDocumentUploadGrantId::from(row.try_get::<_, Uuid>("id")?),
        workspace_id: WorkspaceId::from(row.try_get::<_, Uuid>("workspace_id")?),
        policy_id: PolicyId::from(row.try_get::<_, Uuid>("policy_id")?),
        issued_by_user_id: UserId::from(row.try_get::<_, Uuid>("issued_by_user_id")?),
        issued_via_agent_connection_id: AgentConnectionId::from(
            row.try_get::<_, Uuid>("issued_via_agent_connection_id")?,
        ),
        issued_at: row.try_get("issued_at")?,
        expires_at: row.try_get("expires_at")?,
        redeemed_at: row.try_get("redeemed_at")?,
    })
}
