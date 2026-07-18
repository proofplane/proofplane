use axum::{
    extract::{rejection::QueryRejection, Query, State},
    http::{
        header::{COOKIE, LOCATION, SET_COOKIE},
        HeaderMap, HeaderValue, StatusCode,
    },
    response::{Html, IntoResponse, Response},
    routing::get,
    Extension, Router,
};
use chrono::{DateTime, Utc};
use serde::Deserialize;

use crate::{
    domain::WorkspacePermissions,
    observability::audit::{AuditActor, AuditClientType, AuditEvent, AuditObject, AuditOutcome},
    routes::{error::ApiError, request_context::RequestId},
    services::{
        agent_connections::AgentConnectionContext,
        policies::PolicyService,
        policy_document_upload_grants::{PolicyDocumentUploadGrantService, PolicyUploadGrantError},
        policy_upload_sessions::{
            PolicyUploadSessionError, PolicyUploadSessionTokenService, VerifiedPolicyUploadSession,
        },
    },
};

const POLICY_UPLOAD_SESSION_COOKIE: &str = "proofplane_policy_document_upload_session";
const POLICY_UPLOAD_PATH: &str = "/policy-document-uploads";

#[derive(Clone)]
pub struct PolicyDocumentUploadSessionState {
    pub grants: PolicyDocumentUploadGrantService,
    pub sessions: PolicyUploadSessionTokenService,
    pub policies: PolicyService,
    pub secure_cookie: bool,
}

pub fn router(state: PolicyDocumentUploadSessionState) -> Router {
    Router::new()
        .route(POLICY_UPLOAD_PATH, get(open_policy_upload_session))
        .with_state(state)
}

#[derive(Debug, Deserialize)]
struct PolicyUploadSessionQuery {
    token: Option<String>,
}

async fn open_policy_upload_session(
    State(state): State<PolicyDocumentUploadSessionState>,
    Extension(request_id): Extension<RequestId>,
    headers: HeaderMap,
    query: Result<Query<PolicyUploadSessionQuery>, QueryRejection>,
) -> Result<Response, ApiError> {
    let Query(query) = match query {
        Ok(query) => query,
        Err(_) => return Ok(unavailable_response()),
    };
    if let Some(token) = query.token {
        return redeem_grant(state, request_id, token).await;
    }

    let session = match verify_session(&state, &headers) {
        Ok(session) => session,
        Err(PolicyUploadSessionError::Unavailable) => return Ok(unavailable_response()),
        Err(PolicyUploadSessionError::Internal) => return Err(ApiError::Internal),
    };
    let connection = AgentConnectionContext {
        user_id: session.issued_by_user_id,
        connection_id: session.issued_via_agent_connection_id,
        workspace_id: session.workspace_id,
        permissions: WorkspacePermissions::all(),
    };
    if state
        .policies
        .get(connection, session.policy_id)
        .await?
        .is_none()
    {
        return Ok(unavailable_response());
    }

    Ok(StatusCode::NO_CONTENT.into_response())
}

async fn redeem_grant(
    state: PolicyDocumentUploadSessionState,
    request_id: RequestId,
    token: String,
) -> Result<Response, ApiError> {
    if token.is_empty() {
        return Ok(unavailable_response());
    }
    let grant = match state.grants.redeem(&token).await {
        Ok(grant) => grant,
        Err(PolicyUploadGrantError::Unavailable) => return Ok(unavailable_response()),
        Err(error @ (PolicyUploadGrantError::Internal | PolicyUploadGrantError::Repository(_))) => {
            tracing::error!(%error, "policy document upload grant redemption failed");
            return Err(ApiError::Internal);
        }
    };
    let session = state
        .sessions
        .issue_until(
            grant.workspace_id,
            grant.policy_id,
            grant.issued_by_user_id,
            grant.issued_via_agent_connection_id,
            grant.expires_at,
        )
        .map_err(policy_upload_session_error)?;

    AuditEvent::new(
        "policy_document_upload_grant.redeemed",
        AuditOutcome::Success,
        AuditActor::AgentConnection {
            user_id: grant.issued_by_user_id.into(),
            agent_connection_id: grant.issued_via_agent_connection_id.into(),
        },
        AuditClientType::Rest,
        "redeem_policy_document_upload_grant",
    )
    .workspace_id(grant.workspace_id.into())
    .request_id(request_id.0)
    .metadata("policy_id", grant.policy_id)
    .object(AuditObject::new("policy", grant.policy_id.into()))
    .emit();

    let mut response = StatusCode::SEE_OTHER.into_response();
    response
        .headers_mut()
        .insert(LOCATION, HeaderValue::from_static(POLICY_UPLOAD_PATH));
    response.headers_mut().insert(
        SET_COOKIE,
        HeaderValue::from_str(&session_cookie(
            &session,
            state.secure_cookie,
            grant.expires_at,
        ))
        .map_err(|_| ApiError::Internal)?,
    );
    Ok(response)
}

fn verify_session(
    state: &PolicyDocumentUploadSessionState,
    headers: &HeaderMap,
) -> Result<VerifiedPolicyUploadSession, PolicyUploadSessionError> {
    let token =
        policy_upload_session_cookie(headers).ok_or(PolicyUploadSessionError::Unavailable)?;
    state.sessions.verify(token)
}

fn policy_upload_session_cookie(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .map(str::trim)
        .find_map(|cookie| cookie.strip_prefix(&format!("{POLICY_UPLOAD_SESSION_COOKIE}=")))
        .filter(|value| !value.is_empty())
}

fn session_cookie(token: &str, secure: bool, expires_at: DateTime<Utc>) -> String {
    let max_age = (expires_at - Utc::now()).num_seconds().max(1);
    let mut cookie = format!(
        "{POLICY_UPLOAD_SESSION_COOKIE}={token}; HttpOnly; SameSite=Lax; Path={POLICY_UPLOAD_PATH}; Max-Age={max_age}"
    );
    if secure {
        cookie.push_str("; Secure");
    }
    cookie
}

fn policy_upload_session_error(error: PolicyUploadSessionError) -> ApiError {
    match error {
        PolicyUploadSessionError::Unavailable => ApiError::NotFound,
        PolicyUploadSessionError::Internal => ApiError::Internal,
    }
}

fn unavailable_response() -> Response {
    (
        StatusCode::NOT_FOUND,
        Html("<!doctype html><title>Unavailable</title><h1>This policy document link is no longer available</h1>"),
    )
        .into_response()
}
