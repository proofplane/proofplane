pub use crate::read_models::{AgentConnectionAuthority, ReusableAgentConnection};
use crate::{
    domain::{AgentConnectionId, UserId, WorkspacePermission},
    persistence::{Error, Postgres},
};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct FindReusableAgentConnection {
    pub auth0_subject: String,
    pub auth0_client_id: String,
    pub resource: String,
    pub permissions: Vec<WorkspacePermission>,
}
#[derive(Debug, Clone, Copy)]
pub struct ResolveAgentConnectionAuthority {
    pub connection_id: AgentConnectionId,
}
#[derive(Debug, Clone, Copy)]
pub struct ListUserAgentConnections {
    pub user_id: UserId,
}
#[derive(Clone)]
pub struct FindReusableAgentConnectionHandler {
    repository: Arc<Postgres>,
}
#[derive(Clone)]
pub struct ResolveAgentConnectionAuthorityHandler {
    repository: Arc<Postgres>,
}
#[derive(Clone)]
pub struct ListUserAgentConnectionsHandler {
    repository: Arc<Postgres>,
}
impl FindReusableAgentConnectionHandler {
    pub fn new(repository: Arc<Postgres>) -> Self {
        Self { repository }
    }
    pub async fn handle(
        &self,
        query: FindReusableAgentConnection,
    ) -> Result<Option<ReusableAgentConnection>, Error> {
        self.repository
            .reads()
            .await?
            .agent_connections()
            .find_reusable(
                &query.auth0_subject,
                &query.auth0_client_id,
                &query.resource,
                query.permissions,
            )
            .await
    }
}
impl ResolveAgentConnectionAuthorityHandler {
    pub fn new(repository: Arc<Postgres>) -> Self {
        Self { repository }
    }
    pub async fn handle(
        &self,
        query: ResolveAgentConnectionAuthority,
    ) -> Result<Option<AgentConnectionAuthority>, Error> {
        self.repository
            .reads()
            .await?
            .agent_connections()
            .resolve_authority(query.connection_id)
            .await
    }
}
impl ListUserAgentConnectionsHandler {
    pub fn new(repository: Arc<Postgres>) -> Self {
        Self { repository }
    }
    pub async fn handle(
        &self,
        query: ListUserAgentConnections,
    ) -> Result<Vec<crate::read_models::UserAgentConnectionSummary>, Error> {
        self.repository
            .reads()
            .await?
            .agent_connections()
            .list_for_user(query.user_id)
            .await
    }
}
