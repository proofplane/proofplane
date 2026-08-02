use chrono::{DateTime, Utc};
use secrecy::{ExposeSecret, SecretString};

use super::{ids::uuid_id, AuditorAccessGrantId, Sha256Digest};

uuid_id!(AuditorAuthTransactionId);

#[derive(Debug)]
pub struct AuditorAuthTransaction {
    id: AuditorAuthTransactionId,
    grant_id: AuditorAccessGrantId,
    state_digest: Sha256Digest,
    nonce_digest: Sha256Digest,
    pkce_verifier: SecretString,
    expires_at: DateTime<Utc>,
    consumed_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum AuditorAuthTransactionLifecycleError {
    #[error("auditor authentication transaction creation is invalid")]
    InvalidCreation,
    #[error("persisted auditor authentication transaction is inconsistent")]
    InvalidRehydration,
    #[error("auditor authentication transaction is unavailable")]
    Unavailable,
}

impl AuditorAuthTransaction {
    #[allow(clippy::too_many_arguments)]
    pub fn start(
        id: AuditorAuthTransactionId,
        grant_id: AuditorAccessGrantId,
        state_digest: Sha256Digest,
        nonce_digest: Sha256Digest,
        pkce_verifier: SecretString,
        created_at: DateTime<Utc>,
        expires_at: DateTime<Utc>,
    ) -> Result<Self, AuditorAuthTransactionLifecycleError> {
        if expires_at <= created_at || !(43..=128).contains(&pkce_verifier.expose_secret().len()) {
            return Err(AuditorAuthTransactionLifecycleError::InvalidCreation);
        }
        Ok(Self {
            id,
            grant_id,
            state_digest,
            nonce_digest,
            pkce_verifier,
            expires_at,
            consumed_at: None,
            created_at,
        })
    }

    #[allow(clippy::too_many_arguments, dead_code)]
    pub(crate) fn rehydrate(
        id: AuditorAuthTransactionId,
        grant_id: AuditorAccessGrantId,
        state_digest: Sha256Digest,
        nonce_digest: Sha256Digest,
        pkce_verifier: SecretString,
        created_at: DateTime<Utc>,
        expires_at: DateTime<Utc>,
        consumed_at: Option<DateTime<Utc>>,
    ) -> Result<Self, AuditorAuthTransactionLifecycleError> {
        let mut transaction = Self::start(
            id,
            grant_id,
            state_digest,
            nonce_digest,
            pkce_verifier,
            created_at,
            expires_at,
        )
        .map_err(|_| AuditorAuthTransactionLifecycleError::InvalidRehydration)?;
        if consumed_at.is_some_and(|value| value < created_at || value >= expires_at) {
            return Err(AuditorAuthTransactionLifecycleError::InvalidRehydration);
        }
        transaction.consumed_at = consumed_at;
        Ok(transaction)
    }

    pub fn claim(
        &mut self,
        consumed_at: DateTime<Utc>,
    ) -> Result<(), AuditorAuthTransactionLifecycleError> {
        if self.consumed_at.is_some()
            || consumed_at < self.created_at
            || consumed_at >= self.expires_at
        {
            return Err(AuditorAuthTransactionLifecycleError::Unavailable);
        }
        self.consumed_at = Some(consumed_at);
        Ok(())
    }

    pub fn id(&self) -> AuditorAuthTransactionId {
        self.id
    }

    pub fn grant_id(&self) -> AuditorAccessGrantId {
        self.grant_id
    }

    pub fn state_digest(&self) -> Sha256Digest {
        self.state_digest
    }

    pub fn nonce_digest(&self) -> Sha256Digest {
        self.nonce_digest
    }

    pub fn pkce_verifier(&self) -> &SecretString {
        &self.pkce_verifier
    }

    pub fn expires_at(&self) -> DateTime<Utc> {
        self.expires_at
    }

    pub fn consumed_at(&self) -> Option<DateTime<Utc>> {
        self.consumed_at
    }

    pub fn created_at(&self) -> DateTime<Utc> {
        self.created_at
    }
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, TimeZone, Utc};
    use secrecy::SecretString;
    use uuid::Uuid;

    use super::{AuditorAuthTransaction, AuditorAuthTransactionLifecycleError};
    use crate::domain::Sha256Digest;

    #[test]
    fn authentication_transaction_claims_once_before_expiry() {
        let created_at = Utc.with_ymd_and_hms(2026, 8, 2, 12, 0, 0).unwrap();
        let expires_at = created_at + Duration::minutes(10);
        let mut transaction = AuditorAuthTransaction::start(
            Uuid::new_v4().into(),
            Uuid::new_v4().into(),
            Sha256Digest::digest(b"state"),
            Sha256Digest::digest(b"nonce"),
            SecretString::from("a".repeat(43)),
            created_at,
            expires_at,
        )
        .unwrap();

        assert_eq!(transaction.claim(created_at), Ok(()));
        assert_eq!(transaction.consumed_at(), Some(created_at));
        assert_eq!(
            transaction.claim(created_at + Duration::seconds(1)),
            Err(AuditorAuthTransactionLifecycleError::Unavailable)
        );

        let mut expired = AuditorAuthTransaction::start(
            Uuid::new_v4().into(),
            Uuid::new_v4().into(),
            Sha256Digest::digest(b"other-state"),
            Sha256Digest::digest(b"other-nonce"),
            SecretString::from("b".repeat(43)),
            created_at,
            expires_at,
        )
        .unwrap();
        assert_eq!(
            expired.claim(expires_at),
            Err(AuditorAuthTransactionLifecycleError::Unavailable)
        );
    }
}
