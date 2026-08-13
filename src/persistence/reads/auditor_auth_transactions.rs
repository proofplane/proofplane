use uuid::Uuid;

use crate::{
    domain::{AuditorAuthTransactionId, Sha256Digest},
    persistence::Error,
};

use super::{param, ReadExecutor, TransactionalReadExecutor};

pub(crate) struct AuditorAuthTransactionReads<'a, E> {
    executor: &'a E,
}
impl<'a, E> AuditorAuthTransactionReads<'a, E> {
    pub(crate) fn new(executor: &'a E) -> Self {
        Self { executor }
    }
}

impl AuditorAuthTransactionReads<'_, TransactionalReadExecutor<'_>> {
    pub async fn resolve_id_by_state_digest(
        &self,
        digest: Sha256Digest,
    ) -> Result<Option<AuditorAuthTransactionId>, Error> {
        let digest: &[u8] = digest.as_bytes();
        self.executor
            .query_opt(
                "SELECT id FROM auditor_auth_transactions WHERE state_digest = $1",
                &[param(&digest)],
            )
            .await?
            .map(|row| {
                row.try_get::<_, Uuid>("id")
                    .map(AuditorAuthTransactionId::from)
            })
            .transpose()
            .map_err(Into::into)
    }
}
