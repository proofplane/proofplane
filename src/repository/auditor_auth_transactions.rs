use chrono::{DateTime, Utc};
use secrecy::{ExposeSecret, SecretString};
use tokio_postgres::Row;
use uuid::Uuid;

use super::{Error, TransactionContext};
use crate::domain::{AuditorAuthTransaction, Sha256Digest};

/// Complete-snapshot persistence for the one-use auditor authentication transaction.
pub struct AuditorAuthTransactionRepository<'a> {
    context: &'a TransactionContext<'a>,
}

impl<'a> TransactionContext<'a> {
    pub fn auditor_auth_transactions(&'a self) -> AuditorAuthTransactionRepository<'a> {
        AuditorAuthTransactionRepository { context: self }
    }
}

impl AuditorAuthTransactionRepository<'_> {
    pub async fn get(
        &self,
        state_digest: Sha256Digest,
    ) -> Result<Option<AuditorAuthTransaction>, Error> {
        let digest: &[u8] = state_digest.as_bytes();
        self.context
            .transaction
            .query_opt(GET_FOR_UPDATE_SQL, &[&digest])
            .await?
            .map(transaction_from_row)
            .transpose()
    }

    pub async fn save(&self, transaction: &AuditorAuthTransaction) -> Result<(), Error> {
        let state_digest = transaction.state_digest();
        let nonce_digest = transaction.nonce_digest();
        let state_digest: &[u8] = state_digest.as_bytes();
        let nonce_digest: &[u8] = nonce_digest.as_bytes();
        let affected = self
            .context
            .transaction
            .execute(
                SAVE_SQL,
                &[
                    &Uuid::from(transaction.id()),
                    &Uuid::from(transaction.grant_id()),
                    &state_digest,
                    &nonce_digest,
                    &transaction.pkce_verifier().expose_secret(),
                    &transaction.expires_at(),
                    &transaction.consumed_at(),
                    &transaction.created_at(),
                ],
            )
            .await?;
        if affected != 1 {
            return Err(Error::InvariantViolation(
                "auditor authentication transaction snapshot save affected an unexpected row count",
            ));
        }
        Ok(())
    }
}

const GET_FOR_UPDATE_SQL: &str = "SELECT id, grant_id, state_digest, nonce_digest, pkce_verifier, expires_at, consumed_at, created_at FROM auditor_auth_transactions WHERE state_digest = $1 FOR UPDATE";
const SAVE_SQL: &str = r#"
INSERT INTO auditor_auth_transactions (id, grant_id, state_digest, nonce_digest, pkce_verifier, expires_at, consumed_at, created_at)
VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
ON CONFLICT (id) DO UPDATE SET grant_id = EXCLUDED.grant_id, state_digest = EXCLUDED.state_digest,
nonce_digest = EXCLUDED.nonce_digest, pkce_verifier = EXCLUDED.pkce_verifier, expires_at = EXCLUDED.expires_at,
consumed_at = EXCLUDED.consumed_at, created_at = EXCLUDED.created_at, updated_at = now()
"#;

fn transaction_from_row(row: Row) -> Result<AuditorAuthTransaction, Error> {
    let state: [u8; 32] = row
        .try_get::<_, Vec<u8>>("state_digest")?
        .try_into()
        .map_err(|_| Error::InvariantViolation("auditor state digest must contain 32 bytes"))?;
    let nonce: [u8; 32] = row
        .try_get::<_, Vec<u8>>("nonce_digest")?
        .try_into()
        .map_err(|_| Error::InvariantViolation("auditor nonce digest must contain 32 bytes"))?;
    AuditorAuthTransaction::rehydrate(
        row.try_get::<_, Uuid>("id")?.into(),
        row.try_get::<_, Uuid>("grant_id")?.into(),
        Sha256Digest::from_bytes(state),
        Sha256Digest::from_bytes(nonce),
        SecretString::from(row.try_get::<_, String>("pkce_verifier")?),
        row.try_get::<_, DateTime<Utc>>("created_at")?,
        row.try_get("expires_at")?,
        row.try_get("consumed_at")?,
    )
    .map_err(|_| {
        Error::InvariantViolation("persisted auditor authentication transaction is inconsistent")
    })
}

#[cfg(test)]
mod tests {
    use super::{GET_FOR_UPDATE_SQL, SAVE_SQL};
    #[test]
    fn repository_locks_and_saves_complete_transaction_snapshots() {
        assert!(GET_FOR_UPDATE_SQL.contains("FOR UPDATE"));
        for field in [
            "state_digest",
            "nonce_digest",
            "pkce_verifier",
            "consumed_at",
            "created_at",
        ] {
            assert!(SAVE_SQL.contains(field));
        }
    }
}
