use axum::{
    extract::{Path, State},
    http::{
        header::{COOKIE, SET_COOKIE},
        HeaderMap, HeaderValue, StatusCode,
    },
    response::{IntoResponse, Response},
    routing::post,
    Extension, Json, Router,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    observability::audit::{AuditActor, AuditClientType, AuditEvent, AuditObject, AuditOutcome},
    routes::{error::ApiError, request_context::RequestId},
    services::{
        auditor_access_grants::{AuditorAccessGrantError, AuditorAccessGrantService},
        auditor_access_sessions::{AuditorAccessSessionError, AuditorAccessSessionService},
    },
};

const AUDITOR_SESSION_COOKIE: &str = "proofplane_auditor_session";

#[derive(Clone)]
pub struct AuditorAccessState {
    pub grants: AuditorAccessGrantService,
    pub sessions: AuditorAccessSessionService,
    pub secure_cookie: bool,
}

#[derive(Debug, Deserialize)]
struct InvitePayload {
    token: String,
}

#[derive(Debug, Deserialize)]
struct VerifyPayload {
    token: String,
    code: String,
}

#[derive(Debug, Serialize)]
struct StatusResponse {
    status: &'static str,
}

pub fn router(state: AuditorAccessState) -> Router {
    Router::new()
        .route(
            "/auditor-access/{workspace_id}/otp/request",
            post(request_otp),
        )
        .route(
            "/auditor-access/{workspace_id}/otp/verify",
            post(verify_otp),
        )
        .route("/auditor-access/logout", post(logout))
        .with_state(state)
}

async fn request_otp(
    State(state): State<AuditorAccessState>,
    Extension(request_id): Extension<RequestId>,
    Path(workspace_id): Path<Uuid>,
    Json(payload): Json<InvitePayload>,
) -> Result<Json<StatusResponse>, ApiError> {
    let grant = state
        .grants
        .load_for_use(workspace_id.into(), &payload.token)
        .await
        .map_err(grant_error)?;
    state
        .sessions
        .request_otp(&grant)
        .await
        .map_err(session_error)?;
    audit(
        "auditor_access_otp.requested",
        "request_auditor_access_otp",
        request_id.0,
        workspace_id,
        Uuid::from(grant.id),
        &grant.auditor_email,
    );

    Ok(Json(StatusResponse { status: "sent" }))
}

async fn verify_otp(
    State(state): State<AuditorAccessState>,
    Extension(request_id): Extension<RequestId>,
    Path(workspace_id): Path<Uuid>,
    Json(payload): Json<VerifyPayload>,
) -> Result<Response, ApiError> {
    let grant = state
        .grants
        .load_for_use(workspace_id.into(), &payload.token)
        .await
        .map_err(grant_error)?;
    let created = state
        .sessions
        .verify_otp(&grant, payload.code.trim())
        .await
        .map_err(session_error)?;
    audit(
        "auditor_access_otp.verified",
        "verify_auditor_access_otp",
        request_id.0,
        workspace_id,
        Uuid::from(grant.id),
        &grant.auditor_email,
    );
    audit(
        "auditor_access_session.created",
        "create_auditor_access_session",
        request_id.0,
        workspace_id,
        Uuid::from(created.session.id),
        &grant.auditor_email,
    );

    let mut response =
        (StatusCode::OK, Json(StatusResponse { status: "verified" })).into_response();
    response.headers_mut().insert(
        SET_COOKIE,
        HeaderValue::from_str(&session_cookie(&created.raw_session, state.secure_cookie))
            .map_err(|_| ApiError::Internal)?,
    );
    Ok(response)
}

async fn logout(
    State(state): State<AuditorAccessState>,
    Extension(request_id): Extension<RequestId>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    if let Some(raw_session) = auditor_session_cookie(&headers) {
        if let Some(session) = state
            .sessions
            .revoke_session(raw_session)
            .await
            .map_err(session_error)?
        {
            audit(
                "auditor_access_session.revoked",
                "logout_auditor_access_session",
                request_id.0,
                Uuid::from(session.workspace_id),
                Uuid::from(session.id),
                &session.auditor_email,
            );
        }
    }

    let mut response = (
        StatusCode::OK,
        Json(StatusResponse {
            status: "logged_out",
        }),
    )
        .into_response();
    response.headers_mut().insert(
        SET_COOKIE,
        HeaderValue::from_static(
            "proofplane_auditor_session=; HttpOnly; SameSite=Lax; Path=/auditor-access; Max-Age=0",
        ),
    );
    Ok(response)
}

fn session_cookie(token: &str, secure: bool) -> String {
    let mut cookie = format!(
        "{AUDITOR_SESSION_COOKIE}={token}; HttpOnly; SameSite=Lax; Path=/auditor-access; Max-Age=604800"
    );
    if secure {
        cookie.push_str("; Secure");
    }
    cookie
}

pub fn auditor_session_cookie(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .map(str::trim)
        .find_map(|cookie| cookie.strip_prefix(&format!("{AUDITOR_SESSION_COOKIE}=")))
        .filter(|value| !value.is_empty())
}

fn audit(
    event_name: &'static str,
    operation: &'static str,
    request_id: Uuid,
    workspace_id: Uuid,
    object_id: Uuid,
    auditor_email: &str,
) {
    AuditEvent::new(
        event_name,
        AuditOutcome::Success,
        AuditActor::System {
            name: "auditor_browser",
        },
        AuditClientType::Rest,
        operation,
    )
    .workspace_id(workspace_id)
    .request_id(request_id)
    .metadata("auditor_email", auditor_email)
    .object(AuditObject::new("auditor_access", object_id))
    .emit();
}

fn grant_error(error: AuditorAccessGrantError) -> ApiError {
    match error {
        AuditorAccessGrantError::Unavailable => ApiError::NotFound,
        AuditorAccessGrantError::Denied => ApiError::NotFound,
        AuditorAccessGrantError::Invalid(message) => ApiError::BadRequest(vec![message.to_owned()]),
        AuditorAccessGrantError::Secret(_) | AuditorAccessGrantError::Repository(_) => {
            ApiError::Internal
        }
    }
}

fn session_error(error: AuditorAccessSessionError) -> ApiError {
    match error {
        AuditorAccessSessionError::Unavailable => ApiError::NotFound,
        AuditorAccessSessionError::RateLimited => ApiError::Conflict {
            code: "auditor_otp_rate_limited",
            message: "too many OTP requests".to_owned(),
        },
        AuditorAccessSessionError::Mail(_) => ApiError::Internal,
        AuditorAccessSessionError::Random => ApiError::Internal,
        AuditorAccessSessionError::Repository(error) => {
            tracing::error!(%error, "auditor access session repository failure");
            ApiError::Internal
        }
    }
}
