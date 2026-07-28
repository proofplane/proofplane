use chrono::{DateTime, Utc};
use secrecy::{ExposeSecret, SecretString};
use tokio_postgres::Row;
use uuid::Uuid;

use crate::domain::{AuditorAccessGrantId, AuditorAuthTransactionId, Sha256Digest};

use super::{Error, Postgres};

pub struct NewAuditorAuthTransaction {
    pub id: AuditorAuthTransactionId,
    pub grant_id: AuditorAccessGrantId,
    pub state_digest: Sha256Digest,
    pub nonce_digest: Sha256Digest,
    pub pkce_verifier: SecretString,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug)]
pub struct ClaimedAuditorAuthTransaction {
    pub id: AuditorAuthTransactionId,
    pub grant_id: AuditorAccessGrantId,
    pub nonce_digest: Sha256Digest,
    pub pkce_verifier: SecretString,
    pub expires_at: DateTime<Utc>,
    pub consumed_at: DateTime<Utc>,
}

impl Postgres {
    pub async fn create_auditor_auth_transaction(
        &self,
        transaction: NewAuditorAuthTransaction,
    ) -> Result<bool, Error> {
        let client = self.get().await?;
        let state_digest: &[u8] = transaction.state_digest.as_bytes();
        let nonce_digest: &[u8] = transaction.nonce_digest.as_bytes();
        let inserted = client
            .execute(
                r#"
WITH cleaned AS (
    DELETE FROM auditor_auth_transactions
    WHERE grant_id = $2
      AND (expires_at <= now() OR consumed_at IS NOT NULL)
)
INSERT INTO auditor_auth_transactions (
    id, grant_id, state_digest, nonce_digest, pkce_verifier, expires_at
)
SELECT $1, $2, $3, $4, $5, $6
FROM auditor_access_grants
WHERE id = $2
  AND revoked_at IS NULL
  AND expires_at > now()
"#,
                &[
                    &Uuid::from(transaction.id),
                    &Uuid::from(transaction.grant_id),
                    &state_digest,
                    &nonce_digest,
                    &transaction.pkce_verifier.expose_secret(),
                    &transaction.expires_at,
                ],
            )
            .await?;

        Ok(inserted > 0)
    }

    pub async fn claim_auditor_auth_transaction(
        &self,
        state_digest: Sha256Digest,
    ) -> Result<Option<ClaimedAuditorAuthTransaction>, Error> {
        let client = self.get().await?;
        let state_digest: &[u8] = state_digest.as_bytes();
        client
            .query_opt(
                r#"
UPDATE auditor_auth_transactions
SET consumed_at = now(), updated_at = now()
WHERE state_digest = $1
  AND expires_at > now()
  AND consumed_at IS NULL
RETURNING id, grant_id, nonce_digest, pkce_verifier, expires_at, consumed_at
"#,
                &[&state_digest],
            )
            .await?
            .map(|row| claimed_transaction_from_row(&row))
            .transpose()
    }
}

fn claimed_transaction_from_row(row: &Row) -> Result<ClaimedAuditorAuthTransaction, Error> {
    let nonce_digest = row.try_get::<_, Vec<u8>>("nonce_digest")?;
    let nonce_digest = nonce_digest
        .try_into()
        .map_err(|_| Error::InvariantViolation("auditor nonce digest must contain 32 bytes"))?;

    Ok(ClaimedAuditorAuthTransaction {
        id: AuditorAuthTransactionId::from(row.try_get::<_, Uuid>("id")?),
        grant_id: AuditorAccessGrantId::from(row.try_get::<_, Uuid>("grant_id")?),
        nonce_digest: Sha256Digest::from_bytes(nonce_digest),
        pkce_verifier: SecretString::from(row.try_get::<_, String>("pkce_verifier")?),
        expires_at: row.try_get("expires_at")?,
        consumed_at: row.try_get("consumed_at")?,
    })
}
