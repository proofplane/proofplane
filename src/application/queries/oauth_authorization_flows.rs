use std::sync::Arc;

use chrono::{DateTime, Utc};
use tokio_postgres::Row;
use uuid::Uuid;

use crate::{
    domain::{OAuthAuthorizationRequestId, UserId, WorkspaceId, WorkspacePermission},
    persistence::{Error, Postgres},
};

#[derive(Debug, Clone, Copy)]
pub struct ReadOAuthConsentContext {
    pub request_id: OAuthAuthorizationRequestId,
    pub now: DateTime<Utc>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OAuthConsentContext {
    pub request_id: OAuthAuthorizationRequestId,
    pub client_name: String,
    pub client_id: String,
    pub redirect_uri: String,
    pub state: String,
    pub resource: String,
    pub scopes: Vec<WorkspacePermission>,
    pub auth0_subject: String,
    pub user_id: UserId,
    pub workspace_id: WorkspaceId,
    pub expires_at: DateTime<Utc>,
}
#[derive(Clone)]
pub struct ReadOAuthConsentContextHandler {
    repository: Arc<Postgres>,
}
impl ReadOAuthConsentContextHandler {
    pub fn new(repository: Arc<Postgres>) -> Self {
        Self { repository }
    }
    pub async fn handle(
        &self,
        query: ReadOAuthConsentContext,
    ) -> Result<Option<OAuthConsentContext>, Error> {
        let mut client = self.repository.get().await?;
        let transaction = client.transaction().await?;
        let row = transaction
            .query_opt(
                CONSENT_CONTEXT_SQL,
                &[&Uuid::from(query.request_id), &query.now],
            )
            .await?;
        transaction.commit().await?;

        row.map(consent_context_from_row).transpose()
    }
}

/// Client-facing, token-compatible authorization-code data. This query does
/// not expose the code or CSRF secret and never opens a write transaction.
#[derive(Debug, Clone)]
pub struct ReadOAuthAuthorizationGrant {
    pub request_id: OAuthAuthorizationRequestId,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OAuthAuthorizationGrantView {
    pub client_id: String,
    pub redirect_uri: String,
    pub code_challenge: String,
    pub resource: String,
    pub scopes: Vec<WorkspacePermission>,
    pub auth0_subject: String,
    pub user_id: UserId,
    pub agent_connection_id: crate::domain::AgentConnectionId,
    pub workspace_id: WorkspaceId,
}
#[derive(Clone)]
pub struct ReadOAuthAuthorizationGrantHandler {
    repository: Arc<Postgres>,
}
impl ReadOAuthAuthorizationGrantHandler {
    pub fn new(repository: Arc<Postgres>) -> Self {
        Self { repository }
    }
    pub async fn handle(
        &self,
        query: ReadOAuthAuthorizationGrant,
    ) -> Result<Option<OAuthAuthorizationGrantView>, Error> {
        let mut client = self.repository.get().await?;
        let transaction = client.transaction().await?;
        let row = transaction
            .query_opt(GRANT_VIEW_SQL, &[&Uuid::from(query.request_id)])
            .await?;
        transaction.commit().await?;

        row.map(grant_view_from_row).transpose()
    }
}
const CONSENT_CONTEXT_SQL: &str = "SELECT r.id, r.client_name, r.client_id, r.redirect_uri, r.state, r.resource, r.scopes, r.auth0_subject, r.user_id, m.workspace_id, r.expires_at FROM oauth_authorization_requests r JOIN workspace_memberships m ON m.user_id = r.user_id WHERE r.id = $1 AND r.consumed_at IS NULL AND r.expires_at > $2 AND r.auth0_subject IS NOT NULL";
const GRANT_VIEW_SQL: &str = "SELECT r.client_id, r.redirect_uri, r.code_challenge, r.resource, r.scopes, r.auth0_subject, r.user_id, c.agent_connection_id, c.workspace_id FROM oauth_authorization_requests r JOIN oauth_authorization_codes c ON c.request_id = r.id WHERE r.id = $1";
fn consent_context_from_row(row: Row) -> Result<OAuthConsentContext, Error> {
    Ok(OAuthConsentContext {
        request_id: row.try_get::<_, Uuid>("id")?.into(),
        client_name: row.try_get("client_name")?,
        client_id: row.try_get("client_id")?,
        redirect_uri: row.try_get("redirect_uri")?,
        state: row.try_get("state")?,
        resource: row.try_get("resource")?,
        scopes: row
            .try_get::<_, Vec<String>>("scopes")?
            .into_iter()
            .map(|value| {
                value
                    .parse()
                    .map_err(|_| Error::InvariantViolation("unknown OAuth scope"))
            })
            .collect::<Result<_, _>>()?,
        auth0_subject: row.try_get("auth0_subject")?,
        user_id: row.try_get::<_, Uuid>("user_id")?.into(),
        workspace_id: row.try_get::<_, Uuid>("workspace_id")?.into(),
        expires_at: row.try_get("expires_at")?,
    })
}
fn grant_view_from_row(row: Row) -> Result<OAuthAuthorizationGrantView, Error> {
    Ok(OAuthAuthorizationGrantView {
        client_id: row.try_get("client_id")?,
        redirect_uri: row.try_get("redirect_uri")?,
        code_challenge: row.try_get("code_challenge")?,
        resource: row.try_get("resource")?,
        scopes: row
            .try_get::<_, Vec<String>>("scopes")?
            .into_iter()
            .map(|value| {
                value
                    .parse()
                    .map_err(|_| Error::InvariantViolation("unknown OAuth scope"))
            })
            .collect::<Result<_, _>>()?,
        auth0_subject: row.try_get("auth0_subject")?,
        user_id: row.try_get::<_, Uuid>("user_id")?.into(),
        agent_connection_id: row.try_get::<_, Uuid>("agent_connection_id")?.into(),
        workspace_id: row.try_get::<_, Uuid>("workspace_id")?.into(),
    })
}

#[cfg(test)]
mod tests {
    use super::{CONSENT_CONTEXT_SQL, GRANT_VIEW_SQL};
    #[test]
    fn oauth_queries_are_read_only_and_do_not_leak_secrets() {
        assert!(!CONSENT_CONTEXT_SQL.contains("UPDATE"));
        assert!(!GRANT_VIEW_SQL.contains("digest"));
        assert!(!GRANT_VIEW_SQL.contains("UPDATE"));
    }
}
