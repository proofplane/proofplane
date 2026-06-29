use axum::{
    extract::{rejection::QueryRejection, Query, State},
    http::{
        header::{COOKIE, SET_COOKIE},
        HeaderMap, HeaderValue,
    },
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    domain::{AttachmentUploadStatus, EvidenceAttachment, EvidenceSubmissionId},
    routes::error::ApiError,
    services::{
        attachment_upload_grants::{AttachmentUploadGrantService, UploadGrantError},
        evidence_submissions::EvidenceSubmissionService,
        upload_sessions::{UploadSessionError, UploadSessionTokenService, UPLOAD_SESSION_TTL},
    },
};

const UPLOAD_SESSION_COOKIE: &str = "proofplane_attachment_upload_session";

#[derive(Clone)]
pub struct AttachmentUploadSessionState {
    pub grants: AttachmentUploadGrantService,
    pub sessions: UploadSessionTokenService,
    pub submissions: EvidenceSubmissionService,
    pub secure_cookie: bool,
}

pub fn router(state: AttachmentUploadSessionState) -> Router {
    Router::new()
        .route("/evidence-attachment-uploads", get(open_upload_session))
        .with_state(state)
}

#[derive(Debug, Deserialize)]
struct UploadSessionQuery {
    token: Option<String>,
}

#[derive(Debug, Serialize)]
struct UploadSessionResponse {
    submission_id: Uuid,
    attachments: Vec<UploadSessionAttachmentResponse>,
}

#[derive(Debug, Serialize)]
struct UploadSessionAttachmentResponse {
    id: Uuid,
    filename: String,
    content_type: String,
    content_length: i64,
    upload_status: String,
}

async fn open_upload_session(
    State(state): State<AttachmentUploadSessionState>,
    headers: HeaderMap,
    query: Result<Query<UploadSessionQuery>, QueryRejection>,
) -> Result<Response, ApiError> {
    let Query(query) = query.map_err(|_| unavailable())?;
    if let Some(token) = query.token {
        return redeem_grant(state, token).await;
    }

    let token = upload_session_cookie(&headers).ok_or_else(unavailable)?;
    let session = state.sessions.verify(token).map_err(upload_session_error)?;
    let body = inventory(
        &state.submissions,
        session.submission_id,
        session.api_token_context(),
    )
    .await?;
    Ok(Json(body).into_response())
}

async fn redeem_grant(
    state: AttachmentUploadSessionState,
    token: String,
) -> Result<Response, ApiError> {
    if token.is_empty() {
        return Err(unavailable());
    }

    let grant = state
        .grants
        .redeem(&token)
        .await
        .map_err(upload_grant_error)?;
    let session = state
        .sessions
        .issue(
            grant.workspace_id,
            grant.submission_id,
            grant.issued_by_user_id,
            grant.issued_via_api_token_id,
        )
        .map_err(upload_session_error)?;
    let body = inventory(
        &state.submissions,
        grant.submission_id,
        crate::authentication::ApiTokenContext {
            workspace_id: grant.workspace_id,
            user_id: grant.issued_by_user_id,
            api_token_id: grant.issued_via_api_token_id,
            permissions: crate::domain::WorkspacePermissions::from_iter(
                crate::domain::WorkspacePermission::ALL,
            ),
        },
    )
    .await?;

    let mut response = Json(body).into_response();
    response.headers_mut().insert(
        SET_COOKIE,
        HeaderValue::from_str(&session_cookie(&session, state.secure_cookie))
            .map_err(|_| ApiError::Internal)?,
    );
    Ok(response)
}

async fn inventory(
    submissions: &EvidenceSubmissionService,
    submission_id: EvidenceSubmissionId,
    token: crate::authentication::ApiTokenContext,
) -> Result<UploadSessionResponse, ApiError> {
    let detail = submissions
        .get(token, submission_id)
        .await
        .map_err(ApiError::from)?
        .ok_or_else(unavailable)?;

    Ok(UploadSessionResponse {
        submission_id: Uuid::from(detail.submission.id),
        attachments: detail.attachments.into_iter().map(Into::into).collect(),
    })
}

fn upload_session_cookie(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .map(str::trim)
        .find_map(|cookie| cookie.strip_prefix(&format!("{UPLOAD_SESSION_COOKIE}=")))
        .filter(|value| !value.is_empty())
}

fn session_cookie(token: &str, secure: bool) -> String {
    let mut cookie = format!(
        "{UPLOAD_SESSION_COOKIE}={token}; HttpOnly; SameSite=Lax; Path=/evidence-attachment-uploads; Max-Age={}",
        UPLOAD_SESSION_TTL.as_secs()
    );
    if secure {
        cookie.push_str("; Secure");
    }
    cookie
}

fn upload_grant_error(error: UploadGrantError) -> ApiError {
    match error {
        UploadGrantError::Unavailable => unavailable(),
        UploadGrantError::Internal | UploadGrantError::Repository(_) => ApiError::Internal,
    }
}

fn upload_session_error(error: UploadSessionError) -> ApiError {
    match error {
        UploadSessionError::Unavailable => unavailable(),
        UploadSessionError::Internal => ApiError::Internal,
    }
}

fn unavailable() -> ApiError {
    ApiError::NotFound
}

impl From<EvidenceAttachment> for UploadSessionAttachmentResponse {
    fn from(attachment: EvidenceAttachment) -> Self {
        Self {
            id: Uuid::from(attachment.id),
            filename: attachment.filename,
            content_type: attachment.content_type,
            content_length: attachment.content_length,
            upload_status: upload_status(attachment.upload_status).to_owned(),
        }
    }
}

fn upload_status(status: AttachmentUploadStatus) -> &'static str {
    status.as_str()
}
