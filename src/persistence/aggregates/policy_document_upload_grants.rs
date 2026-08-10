use chrono::{DateTime, Utc};
use tokio_postgres::Row;
use uuid::Uuid;

use crate::{
    domain::{
        PolicyDocumentUploadGrant as DomainPolicyDocumentUploadGrant, PolicyDocumentUploadGrantId,
    },
    persistence::WorkspaceUnitOfWork,
};

use super::{
    snapshot::{save_snapshot, snapshot_record},
    Error,
};

pub struct PolicyDocumentUploadGrantRepository<'a> {
    workspace: &'a WorkspaceUnitOfWork<'a>,
}

impl<'a> WorkspaceUnitOfWork<'a> {
    pub fn policy_document_upload_grants(&'a self) -> PolicyDocumentUploadGrantRepository<'a> {
        PolicyDocumentUploadGrantRepository { workspace: self }
    }
}

impl PolicyDocumentUploadGrantRepository<'_> {
    pub async fn get(
        &self,
        id: PolicyDocumentUploadGrantId,
    ) -> Result<Option<DomainPolicyDocumentUploadGrant>, Error> {
        let parameters: [&(dyn tokio_postgres::types::ToSql + Sync); 2] =
            [&Uuid::from(id), &Uuid::from(self.workspace.workspace_id)];
        let rows = self
            .workspace
            .transaction
            .query(GET_FOR_UPDATE_SQL, &parameters)
            .await?;
        rows.into_iter()
            .next()
            .map(|row| PolicyDocumentUploadGrantRecord::try_from_row(&row)?.into_domain())
            .transpose()
    }

    pub async fn save(&self, grant: &DomainPolicyDocumentUploadGrant) -> Result<(), Error> {
        let record = PolicyDocumentUploadGrantRecord::from_domain(grant)?;
        save_snapshot(self.workspace.transaction, record.as_snapshot()).await
    }
}

const GET_FOR_UPDATE_SQL: &str = r#"
SELECT id, workspace_id, policy_id, issued_by_user_id,
       issued_via_agent_connection_id, issued_at, expires_at, redeemed_at
FROM policy_document_upload_grants
WHERE id = $1 AND workspace_id = $2
FOR UPDATE
"#;

snapshot_record! {
    struct PolicyDocumentUploadGrantRecord {
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
}

impl PolicyDocumentUploadGrantRecord {
    fn try_from_row(row: &Row) -> Result<Self, Error> {
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
    fn from_domain(grant: &DomainPolicyDocumentUploadGrant) -> Result<Self, Error> {
        Ok(Self {
            id: grant.id().into(),
            workspace_id: grant.workspace_id().into(),
            policy_id: grant.policy_id().into(),
            issued_by_user_id: grant.issued_by_user_id().into(),
            issued_via_agent_connection_id: grant.issued_via_agent_connection_id().into(),
            issued_at: grant.issued_at(),
            expires_at: grant.expires_at(),
            redeemed_at: grant.redeemed_at(),
        })
    }

    fn into_domain(self) -> Result<DomainPolicyDocumentUploadGrant, Error> {
        DomainPolicyDocumentUploadGrant::rehydrate(
            self.id.into(),
            self.workspace_id.into(),
            self.policy_id.into(),
            self.issued_by_user_id.into(),
            self.issued_via_agent_connection_id.into(),
            self.issued_at,
            self.expires_at,
            self.redeemed_at,
        )
        .map_err(|_| Error::InvariantViolation("persisted policy human upload grant is invalid"))
    }
}
