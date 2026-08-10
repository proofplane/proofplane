use uuid::Uuid;

use crate::{
    domain::{OAuthAuthorizationRequestId, Sha256Digest},
    persistence::Error,
};

use super::{ReadExecutor, TransactionalReadExecutor};

pub(crate) struct OAuthAuthorizationFlowReads<'a, E> {
    executor: &'a E,
}

impl<'a, E> OAuthAuthorizationFlowReads<'a, E> {
    pub(crate) fn new(executor: &'a E) -> Self {
        Self { executor }
    }
}

impl OAuthAuthorizationFlowReads<'_, TransactionalReadExecutor<'_>> {
    pub async fn resolve_id_by_csrf_digest(
        &self,
        digest: Sha256Digest,
    ) -> Result<Option<OAuthAuthorizationRequestId>, Error> {
        let digest: &[u8] = digest.as_bytes();
        self.executor
            .query_opt(
                "SELECT id FROM oauth_authorization_requests WHERE csrf_token_digest = $1",
                &[&digest],
            )
            .await?
            .map(|row| {
                row.try_get::<_, Uuid>("id")
                    .map(OAuthAuthorizationRequestId::from)
            })
            .transpose()
            .map_err(Into::into)
    }

    pub async fn resolve_id_by_code_digest(
        &self,
        digest: Sha256Digest,
    ) -> Result<Option<OAuthAuthorizationRequestId>, Error> {
        let digest: &[u8] = digest.as_bytes();
        self.executor
            .query_opt(
                "SELECT request_id FROM oauth_authorization_codes WHERE code_digest = $1",
                &[&digest],
            )
            .await?
            .map(|row| {
                row.try_get::<_, Uuid>("request_id")
                    .map(OAuthAuthorizationRequestId::from)
            })
            .transpose()
            .map_err(Into::into)
    }
}
