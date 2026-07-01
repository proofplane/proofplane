use crate::{
    authentication::ApiTokenContext,
    domain::{WorkspaceId, WorkspacePermission},
    routes::request_context::RequestId,
};
use serde_json::json;

#[cfg(test)]
use crate::domain::{ApiTokenId, UserId, WorkspacePermissions};

pub const SESSION_ID_HEADER: &str = "mcp-session-id";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpRequestContext {
    pub token: ApiTokenContext,
    pub request_id: RequestId,
    pub session_id: Option<String>,
}

impl McpRequestContext {
    pub fn audit_actor(&self) -> crate::observability::audit::AuditActor {
        match self.token.agent_connection_id {
            Some(agent_connection_id) => crate::observability::audit::AuditActor::AgentConnection {
                user_id: self.token.user_id.into(),
                agent_connection_id,
            },
            None => crate::observability::audit::AuditActor::ApiToken {
                user_id: self.token.user_id.into(),
                api_token_id: self.token.api_token_id.into(),
            },
        }
    }
    pub fn authorize_token_workspace(
        extensions: &http::Extensions,
        headers: &http::HeaderMap,
        required_permission: WorkspacePermission,
    ) -> Result<Self, rmcp::ErrorData> {
        let token = extensions
            .get::<ApiTokenContext>()
            .copied()
            .ok_or_else(internal_context_error)?;
        let request_id = extensions
            .get::<RequestId>()
            .copied()
            .ok_or_else(internal_context_error)?;

        if !token.permissions.has(required_permission) {
            return Err(not_found());
        }

        Ok(Self {
            token,
            request_id,
            session_id: headers
                .get(SESSION_ID_HEADER)
                .and_then(|value| value.to_str().ok())
                .map(str::to_owned),
        })
    }

    pub fn authorize(
        extensions: &http::Extensions,
        headers: &http::HeaderMap,
        workspace_id: WorkspaceId,
        required_permission: WorkspacePermission,
    ) -> Result<Self, rmcp::ErrorData> {
        let token = extensions
            .get::<ApiTokenContext>()
            .copied()
            .ok_or_else(internal_context_error)?;
        let request_id = extensions
            .get::<RequestId>()
            .copied()
            .ok_or_else(internal_context_error)?;

        if !token.allows(workspace_id, required_permission) {
            return Err(not_found());
        }

        Ok(Self {
            token,
            request_id,
            session_id: headers
                .get(SESSION_ID_HEADER)
                .and_then(|value| value.to_str().ok())
                .map(str::to_owned),
        })
    }
}

fn internal_context_error() -> rmcp::ErrorData {
    rmcp::ErrorData::internal_error("request context unavailable", None)
}

fn not_found() -> rmcp::ErrorData {
    rmcp::ErrorData::resource_not_found(
        "resource not found",
        Some(json!({
            "problem": {
                "code": "not_found",
                "message": "resource not found",
            }
        })),
    )
}

#[cfg(test)]
pub(crate) fn api_token_context(workspace_id: WorkspaceId) -> ApiTokenContext {
    ApiTokenContext {
        user_id: UserId::from(uuid::Uuid::new_v4()),
        api_token_id: ApiTokenId::from(uuid::Uuid::new_v4()),
        workspace_id,
        permissions: WorkspacePermissions::from_iter([WorkspacePermission::ReadControls]),
        agent_connection_id: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    #[test]
    fn authorization_conceals_workspace_and_permission_failures() {
        let workspace_id = WorkspaceId::from(uuid::Uuid::new_v4());
        let mut extensions = http::Extensions::new();
        extensions.insert(api_token_context(workspace_id));
        extensions.insert(RequestId(uuid::Uuid::new_v4()));
        let headers = http::HeaderMap::new();

        for result in [
            McpRequestContext::authorize(
                &extensions,
                &headers,
                WorkspaceId::from(uuid::Uuid::new_v4()),
                WorkspacePermission::ReadControls,
            ),
            McpRequestContext::authorize(
                &extensions,
                &headers,
                workspace_id,
                WorkspacePermission::WriteControls,
            ),
        ] {
            let error = result.expect_err("authorization is concealed");
            assert_eq!(error.code, rmcp::model::ErrorCode::RESOURCE_NOT_FOUND);
            assert_eq!(error.message, "resource not found");
        }
    }

    #[test]
    fn authorization_extracts_request_and_session_context() {
        let workspace_id = WorkspaceId::from(uuid::Uuid::new_v4());
        let token = api_token_context(workspace_id);
        let request_id = RequestId(uuid::Uuid::new_v4());
        let mut extensions = http::Extensions::new();
        extensions.insert(token);
        extensions.insert(request_id);
        let mut headers = http::HeaderMap::new();
        headers.insert(SESSION_ID_HEADER, HeaderValue::from_static("session-1"));

        let context = McpRequestContext::authorize(
            &extensions,
            &headers,
            workspace_id,
            WorkspacePermission::ReadControls,
        )
        .expect("authorization succeeds");

        assert_eq!(context.token, token);
        assert_eq!(context.request_id, request_id);
        assert_eq!(context.session_id.as_deref(), Some("session-1"));
    }

    #[test]
    fn token_workspace_authorization_checks_permission_without_argument_workspace() {
        let workspace_id = WorkspaceId::from(uuid::Uuid::new_v4());
        let token = api_token_context(workspace_id);
        let request_id = RequestId(uuid::Uuid::new_v4());
        let mut extensions = http::Extensions::new();
        extensions.insert(token);
        extensions.insert(request_id);
        let headers = http::HeaderMap::new();

        let context = McpRequestContext::authorize_token_workspace(
            &extensions,
            &headers,
            WorkspacePermission::ReadControls,
        )
        .expect("token workspace authorization succeeds");
        assert_eq!(context.token.workspace_id, workspace_id);

        let denied = McpRequestContext::authorize_token_workspace(
            &extensions,
            &headers,
            WorkspacePermission::WriteControls,
        )
        .expect_err("permission failure is concealed");
        assert_eq!(denied.code, rmcp::model::ErrorCode::RESOURCE_NOT_FOUND);
    }
}
