use uuid::Uuid;

use crate::{
    domain::{AuditorSessionId, Sha256Digest},
    persistence::Error,
};

use super::{param, ReadExecutor, TransactionalReadExecutor};

pub(crate) struct AuditorSessionReads<'a, E> {
    executor: &'a E,
}
impl<'a, E> AuditorSessionReads<'a, E> {
    pub(crate) fn new(executor: &'a E) -> Self {
        Self { executor }
    }
}

impl AuditorSessionReads<'_, TransactionalReadExecutor<'_>> {
    pub async fn resolve_id_by_digest(
        &self,
        digest: Sha256Digest,
    ) -> Result<Option<AuditorSessionId>, Error> {
        let digest: &[u8] = digest.as_bytes();
        self.executor
            .query_opt(
                "SELECT id FROM auditor_sessions WHERE session_digest = $1",
                &[param(&digest)],
            )
            .await?
            .map(|row| row.try_get::<_, Uuid>("id").map(AuditorSessionId::from))
            .transpose()
            .map_err(Into::into)
    }
}
