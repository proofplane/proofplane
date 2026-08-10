use chrono::{DateTime, Utc};

use super::{
    ids::uuid_id, AgentConnectionId, Sha256Digest, UserId, WorkspaceId, WorkspacePermission,
};

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

/// The complete snapshot of one OAuth authorization flow. Secrets are kept as
/// digests; the clear-text CSRF value and authorization code never enter the
/// aggregate or its persistence record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OAuthAuthorizationFlow {
    id: OAuthAuthorizationRequestId,
    client_id: String,
    client_name: String,
    redirect_uri: String,
    code_challenge: String,
    state: String,
    resource: String,
    scopes: Vec<WorkspacePermission>,
    csrf_digest: Sha256Digest,
    auth0_subject: Option<String>,
    user_id: Option<UserId>,
    expires_at: DateTime<Utc>,
    created_at: DateTime<Utc>,
    consumed_at: Option<DateTime<Utc>>,
    authorization_code: Option<OAuthAuthorizationFlowCode>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OAuthAuthorizationFlowCode {
    code_digest: Sha256Digest,
    agent_connection_id: AgentConnectionId,
    workspace_id: WorkspaceId,
    expires_at: DateTime<Utc>,
    consumed_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum OAuthAuthorizationFlowError {
    #[error("OAuth authorization flow is unavailable")]
    Unavailable,
    #[error("OAuth authorization flow creation is invalid")]
    InvalidCreation,
    #[error("persisted OAuth authorization flow is inconsistent")]
    InvalidRehydration,
}

impl OAuthAuthorizationFlow {
    #[allow(clippy::too_many_arguments)]
    pub fn request(
        id: OAuthAuthorizationRequestId,
        client_id: String,
        client_name: String,
        redirect_uri: String,
        code_challenge: String,
        state: String,
        resource: String,
        scopes: Vec<WorkspacePermission>,
        csrf_digest: Sha256Digest,
        created_at: DateTime<Utc>,
        expires_at: DateTime<Utc>,
    ) -> Result<Self, OAuthAuthorizationFlowError> {
        if expires_at <= created_at
            || client_id.is_empty()
            || redirect_uri.is_empty()
            || code_challenge.is_empty()
            || resource.is_empty()
            || scopes.is_empty()
        {
            return Err(OAuthAuthorizationFlowError::InvalidCreation);
        }
        Ok(Self {
            id,
            client_id,
            client_name,
            redirect_uri,
            code_challenge,
            state,
            resource,
            scopes,
            csrf_digest,
            auth0_subject: None,
            user_id: None,
            expires_at,
            created_at,
            consumed_at: None,
            authorization_code: None,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn rehydrate(
        id: OAuthAuthorizationRequestId,
        client_id: String,
        client_name: String,
        redirect_uri: String,
        code_challenge: String,
        state: String,
        resource: String,
        scopes: Vec<WorkspacePermission>,
        csrf_digest: Sha256Digest,
        auth0_subject: Option<String>,
        user_id: Option<UserId>,
        expires_at: DateTime<Utc>,
        created_at: DateTime<Utc>,
        consumed_at: Option<DateTime<Utc>>,
        authorization_code: Option<OAuthAuthorizationFlowCode>,
    ) -> Result<Self, OAuthAuthorizationFlowError> {
        let mut flow = Self::request(
            id,
            client_id,
            client_name,
            redirect_uri,
            code_challenge,
            state,
            resource,
            scopes,
            csrf_digest,
            created_at,
            expires_at,
        )
        .map_err(|_| OAuthAuthorizationFlowError::InvalidRehydration)?;
        if auth0_subject.is_some() != user_id.is_some()
            || consumed_at.is_some_and(|at| at < created_at || at >= expires_at)
            || (authorization_code.is_some() && consumed_at.is_none())
        {
            return Err(OAuthAuthorizationFlowError::InvalidRehydration);
        }
        flow.auth0_subject = auth0_subject;
        flow.user_id = user_id;
        flow.consumed_at = consumed_at;
        flow.authorization_code = authorization_code;
        Ok(flow)
    }

    pub fn attach_subject(
        &mut self,
        auth0_subject: String,
        user_id: UserId,
        now: DateTime<Utc>,
    ) -> Result<(), OAuthAuthorizationFlowError> {
        if !self.available_at(now) || self.auth0_subject.is_some() || auth0_subject.is_empty() {
            return Err(OAuthAuthorizationFlowError::Unavailable);
        }
        self.auth0_subject = Some(auth0_subject);
        self.user_id = Some(user_id);
        Ok(())
    }

    pub fn cancel(&mut self, now: DateTime<Utc>) -> Result<(), OAuthAuthorizationFlowError> {
        if !self.available_at(now) || self.auth0_subject.is_none() || self.user_id.is_none() {
            return Err(OAuthAuthorizationFlowError::Unavailable);
        }
        self.consumed_at = Some(now);
        Ok(())
    }

    pub fn approve_and_issue_code(
        &mut self,
        code_digest: Sha256Digest,
        agent_connection_id: AgentConnectionId,
        workspace_id: WorkspaceId,
        now: DateTime<Utc>,
        code_expires_at: DateTime<Utc>,
    ) -> Result<(), OAuthAuthorizationFlowError> {
        if !self.available_at(now)
            || self.auth0_subject.is_none()
            || self.user_id.is_none()
            || code_expires_at <= now
        {
            return Err(OAuthAuthorizationFlowError::Unavailable);
        }
        self.consumed_at = Some(now);
        self.authorization_code = Some(OAuthAuthorizationFlowCode {
            code_digest,
            agent_connection_id,
            workspace_id,
            expires_at: code_expires_at,
            consumed_at: None,
            created_at: now,
        });
        Ok(())
    }

    pub fn consume_code(
        &mut self,
        client_id: &str,
        redirect_uri: &str,
        now: DateTime<Utc>,
    ) -> Result<(), OAuthAuthorizationFlowError> {
        let Some(code) = &mut self.authorization_code else {
            return Err(OAuthAuthorizationFlowError::Unavailable);
        };
        if self.client_id != client_id
            || self.redirect_uri != redirect_uri
            || code.consumed_at.is_some()
            || now < code.created_at
            || now >= code.expires_at
        {
            return Err(OAuthAuthorizationFlowError::Unavailable);
        }
        code.consumed_at = Some(now);
        Ok(())
    }

    fn available_at(&self, now: DateTime<Utc>) -> bool {
        self.consumed_at.is_none() && now >= self.created_at && now < self.expires_at
    }
    pub fn id(&self) -> OAuthAuthorizationRequestId {
        self.id
    }
    pub fn client_id(&self) -> &str {
        &self.client_id
    }
    pub fn client_name(&self) -> &str {
        &self.client_name
    }
    pub fn redirect_uri(&self) -> &str {
        &self.redirect_uri
    }
    pub fn code_challenge(&self) -> &str {
        &self.code_challenge
    }
    pub fn state(&self) -> &str {
        &self.state
    }
    pub fn resource(&self) -> &str {
        &self.resource
    }
    pub fn scopes(&self) -> &[WorkspacePermission] {
        &self.scopes
    }
    pub fn csrf_digest(&self) -> Sha256Digest {
        self.csrf_digest
    }
    pub fn auth0_subject(&self) -> Option<&str> {
        self.auth0_subject.as_deref()
    }
    pub fn user_id(&self) -> Option<UserId> {
        self.user_id
    }
    pub fn expires_at(&self) -> DateTime<Utc> {
        self.expires_at
    }
    pub fn created_at(&self) -> DateTime<Utc> {
        self.created_at
    }
    pub fn consumed_at(&self) -> Option<DateTime<Utc>> {
        self.consumed_at
    }
    pub fn authorization_code(&self) -> Option<&OAuthAuthorizationFlowCode> {
        self.authorization_code.as_ref()
    }
}

impl OAuthAuthorizationFlowCode {
    pub(crate) fn rehydrate(
        code_digest: Sha256Digest,
        agent_connection_id: AgentConnectionId,
        workspace_id: WorkspaceId,
        expires_at: DateTime<Utc>,
        created_at: DateTime<Utc>,
        consumed_at: Option<DateTime<Utc>>,
    ) -> Result<Self, OAuthAuthorizationFlowError> {
        if expires_at <= created_at
            || consumed_at.is_some_and(|at| at < created_at || at >= expires_at)
        {
            return Err(OAuthAuthorizationFlowError::InvalidRehydration);
        }
        Ok(Self {
            code_digest,
            agent_connection_id,
            workspace_id,
            expires_at,
            consumed_at,
            created_at,
        })
    }
    pub fn code_digest(&self) -> Sha256Digest {
        self.code_digest
    }
    pub fn agent_connection_id(&self) -> AgentConnectionId {
        self.agent_connection_id
    }
    pub fn workspace_id(&self) -> WorkspaceId {
        self.workspace_id
    }
    pub fn expires_at(&self) -> DateTime<Utc> {
        self.expires_at
    }
    pub fn created_at(&self) -> DateTime<Utc> {
        self.created_at
    }
    pub fn consumed_at(&self) -> Option<DateTime<Utc>> {
        self.consumed_at
    }
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, TimeZone, Utc};
    use uuid::Uuid;

    use super::{OAuthAuthorizationFlow, OAuthAuthorizationFlowError};
    use crate::domain::{Sha256Digest, WorkspacePermission};

    fn flow() -> OAuthAuthorizationFlow {
        let created_at = Utc.with_ymd_and_hms(2026, 8, 9, 12, 0, 0).unwrap();
        OAuthAuthorizationFlow::request(
            Uuid::new_v4().into(),
            "client".into(),
            "Client".into(),
            "http://127.0.0.1/callback".into(),
            "challenge".into(),
            "state".into(),
            "https://mcp.proofplane.test/mcp".into(),
            vec![WorkspacePermission::ReadControls],
            Sha256Digest::digest(b"csrf"),
            created_at,
            created_at + Duration::minutes(10),
        )
        .unwrap()
    }

    #[test]
    fn authorization_flow_issues_and_consumes_a_code_once() {
        let mut flow = flow();
        let now = flow.created_at();
        flow.attach_subject("auth0|subject".into(), Uuid::new_v4().into(), now)
            .unwrap();
        flow.approve_and_issue_code(
            Sha256Digest::digest(b"code"),
            Uuid::new_v4().into(),
            Uuid::new_v4().into(),
            now,
            now + Duration::minutes(5),
        )
        .unwrap();
        assert!(flow
            .consume_code("client", "http://127.0.0.1/callback", now)
            .is_ok());
        assert_eq!(
            flow.consume_code("client", "http://127.0.0.1/callback", now),
            Err(OAuthAuthorizationFlowError::Unavailable)
        );
    }

    #[test]
    fn authorization_flow_rejects_expiry_boundaries_and_mismatches() {
        let mut expired_flow = flow();
        let now = expired_flow.created_at();
        expired_flow
            .attach_subject("auth0|subject".into(), Uuid::new_v4().into(), now)
            .unwrap();
        assert_eq!(
            expired_flow.cancel(expired_flow.expires_at()),
            Err(OAuthAuthorizationFlowError::Unavailable)
        );
        let mut flow = flow();
        let now = flow.created_at();
        flow.attach_subject("auth0|subject".into(), Uuid::new_v4().into(), now)
            .unwrap();
        flow.approve_and_issue_code(
            Sha256Digest::digest(b"code"),
            Uuid::new_v4().into(),
            Uuid::new_v4().into(),
            now,
            now + Duration::minutes(5),
        )
        .unwrap();
        assert_eq!(
            flow.consume_code("other", "http://127.0.0.1/callback", now),
            Err(OAuthAuthorizationFlowError::Unavailable)
        );
        assert_eq!(
            flow.consume_code(
                "client",
                "http://127.0.0.1/callback",
                now + Duration::minutes(5)
            ),
            Err(OAuthAuthorizationFlowError::Unavailable)
        );
    }
}
