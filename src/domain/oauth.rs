use chrono::{DateTime, Utc};

use super::{ids::uuid_id, AgentConnectionId, UserId, WorkspacePermission};

uuid_id!(OAuthAuthorizationRequestId);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OAuthAuthorizationRequest {
    pub id: OAuthAuthorizationRequestId,
    pub client_id: String,
    pub client_name: String,
    pub redirect_uri: String,
    pub code_challenge: String,
    pub state: String,
    pub resource: String,
    pub scopes: Vec<WorkspacePermission>,
    pub auth0_subject: Option<String>,
    pub user_id: Option<UserId>,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewOAuthAuthorizationRequest {
    pub id: OAuthAuthorizationRequestId,
    pub client_id: String,
    pub client_name: String,
    pub redirect_uri: String,
    pub code_challenge: String,
    pub state: String,
    pub resource: String,
    pub scopes: Vec<WorkspacePermission>,
    pub csrf_token: String,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OAuthAuthorizationCode {
    pub request_id: OAuthAuthorizationRequestId,
    pub agent_connection_id: AgentConnectionId,
    pub workspace_id: super::WorkspaceId,
    pub client_id: String,
    pub redirect_uri: String,
    pub code_challenge: String,
    pub resource: String,
    pub scopes: Vec<WorkspacePermission>,
    pub auth0_subject: String,
    pub user_id: UserId,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewOAuthAuthorizationCode {
    pub code: String,
    pub request_id: OAuthAuthorizationRequestId,
    pub agent_connection_id: AgentConnectionId,
    pub workspace_id: super::WorkspaceId,
    pub client_id: String,
    pub redirect_uri: String,
    pub code_challenge: String,
    pub resource: String,
    pub scopes: Vec<WorkspacePermission>,
    pub expires_at: DateTime<Utc>,
}
