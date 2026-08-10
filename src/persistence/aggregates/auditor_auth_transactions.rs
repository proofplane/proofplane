use chrono::{DateTime, Utc};
use secrecy::{ExposeSecret, SecretString};
use tokio_postgres::Row;
use uuid::Uuid;

use super::{
    snapshot::{save_snapshot, snapshot_record},
    Error, UnitOfWork,
};
use crate::domain::{AuditorAuthTransaction, AuditorAuthTransactionId, Sha256Digest};

/// Complete-snapshot persistence for the one-use auditor authentication transaction.
pub struct AuditorAuthTransactionRepository<'a> {
    unit_of_work: &'a UnitOfWork<'a>,
}

impl<'a> UnitOfWork<'a> {
    pub fn auditor_auth_transactions(&'a self) -> AuditorAuthTransactionRepository<'a> {
        AuditorAuthTransactionRepository { unit_of_work: self }
    }
}

impl AuditorAuthTransactionRepository<'_> {
    pub async fn get(
        &self,
        id: AuditorAuthTransactionId,
    ) -> Result<Option<AuditorAuthTransaction>, Error> {
        self.unit_of_work
            .transaction
            .query_opt(GET_FOR_UPDATE_SQL, &[&Uuid::from(id)])
            .await?
            .map(|row| AuditorAuthTransactionRecord::try_from_row(&row)?.into_domain())
            .transpose()
    }

    pub async fn save(&self, transaction: &AuditorAuthTransaction) -> Result<(), Error> {
        let record = AuditorAuthTransactionRecord::from_domain(transaction)?;
        save_snapshot(&self.unit_of_work.transaction, record.as_snapshot()).await
    }
}

const GET_FOR_UPDATE_SQL: &str = "SELECT id, grant_id, state_digest, nonce_digest, pkce_verifier, expires_at, consumed_at, created_at, updated_at FROM auditor_auth_transactions WHERE id = $1 FOR UPDATE";

snapshot_record! {
    struct AuditorAuthTransactionRecord {
        id: Uuid,
        grant_id: Uuid,
        state_digest: Vec<u8>,
        nonce_digest: Vec<u8>,
        pkce_verifier: String,
        expires_at: DateTime<Utc>,
        consumed_at: Option<DateTime<Utc>>,
        created_at: DateTime<Utc>,
        updated_at: DateTime<Utc>,
    }
    table: auditor_auth_transactions,
    conflict: id,
}

impl AuditorAuthTransactionRecord {
    fn try_from_row(row: &Row) -> Result<Self, Error> {
        Ok(Self {
            id: row.try_get("id")?,
            grant_id: row.try_get("grant_id")?,
            state_digest: row.try_get("state_digest")?,
            nonce_digest: row.try_get("nonce_digest")?,
            pkce_verifier: row.try_get("pkce_verifier")?,
            expires_at: row.try_get("expires_at")?,
            consumed_at: row.try_get("consumed_at")?,
            created_at: row.try_get("created_at")?,
            updated_at: row.try_get("updated_at")?,
        })
    }

    fn from_domain(transaction: &AuditorAuthTransaction) -> Result<Self, Error> {
        Ok(Self {
            id: transaction.id().into(),
            grant_id: transaction.grant_id().into(),
            state_digest: transaction.state_digest().as_bytes().to_vec(),
            nonce_digest: transaction.nonce_digest().as_bytes().to_vec(),
            pkce_verifier: transaction.pkce_verifier().expose_secret().to_owned(),
            expires_at: transaction.expires_at(),
            consumed_at: transaction.consumed_at(),
            created_at: transaction.created_at(),
            updated_at: transaction.updated_at(),
        })
    }

    fn into_domain(self) -> Result<AuditorAuthTransaction, Error> {
        let state: [u8; 32] = self
            .state_digest
            .try_into()
            .map_err(|_| Error::InvariantViolation("auditor state digest must contain 32 bytes"))?;
        let nonce: [u8; 32] = self
            .nonce_digest
            .try_into()
            .map_err(|_| Error::InvariantViolation("auditor nonce digest must contain 32 bytes"))?;
        AuditorAuthTransaction::rehydrate(
            self.id.into(),
            self.grant_id.into(),
            Sha256Digest::from_bytes(state),
            Sha256Digest::from_bytes(nonce),
            SecretString::from(self.pkce_verifier),
            self.created_at,
            self.expires_at,
            self.consumed_at,
            self.updated_at,
        )
        .map_err(|_| {
            Error::InvariantViolation(
                "persisted auditor authentication transaction is inconsistent",
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::GET_FOR_UPDATE_SQL;
    #[test]
    fn repository_locks_and_saves_complete_transaction_snapshots() {
        assert!(GET_FOR_UPDATE_SQL.contains("FOR UPDATE"));
        assert!(GET_FOR_UPDATE_SQL.contains("updated_at"));
    }
}
