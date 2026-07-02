use axum::{
    body::Body,
    extract::{Path, State},
    http::{
        header::{
            CACHE_CONTROL, CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_TYPE, COOKIE, SET_COOKIE,
        },
        HeaderMap, HeaderName, HeaderValue, StatusCode,
    },
    response::{IntoResponse, Response},
    routing::{get, post},
    Extension, Json, Router,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    domain::{
        AuditorPortalAttachment, AuditorPortalControl, AuditorPortalEvidenceRequest,
        AuditorPortalReadModel, AuditorPortalSubmission, EvidenceRequest, EvidenceSubmission,
        FrameworkRequirement,
    },
    observability::audit::{AuditActor, AuditClientType, AuditEvent, AuditObject, AuditOutcome},
    routes::{error::ApiError, request_context::RequestId},
    services::{
        attachment_downloads::{AttachmentDownloadService, DownloadError},
        auditor_access_grants::{AuditorAccessGrantError, AuditorAccessGrantService},
        auditor_access_sessions::{AuditorAccessSessionError, AuditorAccessSessionService},
        auditor_portal::AuditorPortalReadModelService,
    },
};
use chrono::{DateTime, Utc};

const AUDITOR_SESSION_COOKIE: &str = "proofplane_auditor_session";
const REFERRER_POLICY: HeaderName = HeaderName::from_static("referrer-policy");

#[derive(Clone)]
pub struct AuditorAccessState {
    pub grants: AuditorAccessGrantService,
    pub sessions: AuditorAccessSessionService,
    pub portal: AuditorPortalReadModelService,
    pub downloads: AttachmentDownloadService,
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
        .route("/auditor-access/portal/data", get(portal_data))
        .route(
            "/auditor-access/portal/{*download_path}",
            get(download_attachment),
        )
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

async fn portal_data(
    State(state): State<AuditorAccessState>,
    Extension(request_id): Extension<RequestId>,
    headers: HeaderMap,
) -> Result<Json<AuditorPortalReadModelResponse>, ApiError> {
    let raw_session = auditor_session_cookie(&headers).ok_or(ApiError::NotFound)?;
    let session = state
        .sessions
        .load_session(raw_session)
        .await
        .map_err(session_error)?;
    let model = state
        .portal
        .read_model(&session)
        .await
        .map_err(portal_error)?;

    audit_portal_read(
        request_id.0,
        Uuid::from(session.workspace_id),
        Uuid::from(session.id),
        &session.auditor_email,
    );

    Ok(Json(model.into()))
}

async fn download_attachment(
    State(state): State<AuditorAccessState>,
    Extension(request_id): Extension<RequestId>,
    Path(download_path): Path<String>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let (submission_id, attachment_id) = parse_download_path(&download_path)?;
    let raw_session = auditor_session_cookie(&headers).ok_or(ApiError::NotFound)?;
    let session = state
        .sessions
        .load_session(raw_session)
        .await
        .map_err(session_error)?;
    let downloaded = state
        .downloads
        .download_for_workspace(
            session.workspace_id,
            submission_id.into(),
            attachment_id.into(),
        )
        .await
        .map_err(download_error)?;

    AuditEvent::new(
        "auditor_attachment.downloaded",
        AuditOutcome::Success,
        AuditActor::System {
            name: "auditor_browser",
        },
        AuditClientType::Rest,
        "download_auditor_attachment",
    )
    .workspace_id(Uuid::from(downloaded.audit.workspace_id))
    .request_id(request_id.0)
    .metadata("auditor_email", &session.auditor_email)
    .metadata(
        "evidence_submission_id",
        Uuid::from(downloaded.audit.submission_id),
    )
    .metadata(
        "evidence_attachment_id",
        Uuid::from(downloaded.audit.attachment_id),
    )
    .object(AuditObject::new(
        "evidence_attachment",
        downloaded.audit.attachment_id.into(),
    ))
    .emit();

    let disposition =
        crate::routes::attachment_downloads::content_disposition(&downloaded.attachment.filename);
    let mut response = Body::from_stream(downloaded.object.chunks).into_response();
    *response.status_mut() = StatusCode::OK;
    let headers = response.headers_mut();
    headers.insert(
        CONTENT_TYPE,
        HeaderValue::from_str(&downloaded.attachment.content_type)
            .map_err(|_| ApiError::Internal)?,
    );
    headers.insert(
        CONTENT_LENGTH,
        HeaderValue::from_str(&downloaded.attachment.content_length.to_string())
            .map_err(|_| ApiError::Internal)?,
    );
    headers.insert(
        CONTENT_DISPOSITION,
        HeaderValue::from_str(&disposition).map_err(|_| ApiError::Internal)?,
    );
    headers.insert(CACHE_CONTROL, HeaderValue::from_static("private, no-store"));
    headers.insert(REFERRER_POLICY, HeaderValue::from_static("no-referrer"));

    Ok(response)
}

fn parse_download_path(path: &str) -> Result<(Uuid, Uuid), ApiError> {
    let segments = path.split('/').collect::<Vec<_>>();
    match segments.as_slice() {
        ["evidence-submissions", submission_id, "attachments", attachment_id, "download"] => {
            let submission_id = Uuid::parse_str(submission_id).map_err(|_| ApiError::NotFound)?;
            let attachment_id = Uuid::parse_str(attachment_id).map_err(|_| ApiError::NotFound)?;
            Ok((submission_id, attachment_id))
        }
        _ => Err(ApiError::NotFound),
    }
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

fn audit_portal_read(request_id: Uuid, workspace_id: Uuid, session_id: Uuid, auditor_email: &str) {
    AuditEvent::new(
        "auditor_portal.read",
        AuditOutcome::Success,
        AuditActor::System {
            name: "auditor_browser",
        },
        AuditClientType::Rest,
        "read_auditor_portal",
    )
    .workspace_id(workspace_id)
    .request_id(request_id)
    .metadata("auditor_email", auditor_email)
    .object(AuditObject::new("auditor_access_session", session_id))
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

fn portal_error(error: crate::services::Error) -> ApiError {
    tracing::error!(%error, "auditor portal read model failure");
    ApiError::Internal
}

fn download_error(error: DownloadError) -> ApiError {
    match error {
        DownloadError::NotFound | DownloadError::NotReady => ApiError::NotFound,
        DownloadError::MetadataMismatch | DownloadError::Internal => {
            tracing::error!(%error, "auditor attachment download failed");
            ApiError::Internal
        }
        DownloadError::Repository(repository_error) => {
            tracing::error!(error = %repository_error, "auditor attachment download repository failure");
            ApiError::Internal
        }
    }
}

#[derive(Debug, Serialize)]
struct AuditorPortalReadModelResponse {
    workspace_id: Uuid,
    auditor_email: String,
    controls: Vec<AuditorPortalControlResponse>,
}

impl From<AuditorPortalReadModel> for AuditorPortalReadModelResponse {
    fn from(model: AuditorPortalReadModel) -> Self {
        Self {
            workspace_id: Uuid::from(model.workspace_id),
            auditor_email: model.auditor_email,
            controls: model.controls.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Debug, Serialize)]
struct AuditorPortalControlResponse {
    id: Uuid,
    code: String,
    title: String,
    description: String,
    framework_requirements: Vec<FrameworkRequirementResponse>,
    evidence_requests: Vec<AuditorPortalEvidenceRequestResponse>,
}

impl From<AuditorPortalControl> for AuditorPortalControlResponse {
    fn from(control: AuditorPortalControl) -> Self {
        Self {
            id: Uuid::from(control.id),
            code: control.code,
            title: control.title,
            description: control.description,
            framework_requirements: control
                .framework_requirements
                .into_iter()
                .map(Into::into)
                .collect(),
            evidence_requests: control
                .evidence_requests
                .into_iter()
                .map(Into::into)
                .collect(),
        }
    }
}

#[derive(Debug, Serialize)]
struct FrameworkRequirementResponse {
    id: Uuid,
    framework_id: Uuid,
    code: String,
    title: String,
    description: String,
}

impl From<FrameworkRequirement> for FrameworkRequirementResponse {
    fn from(requirement: FrameworkRequirement) -> Self {
        Self {
            id: Uuid::from(requirement.id),
            framework_id: Uuid::from(requirement.framework_id),
            code: requirement.code,
            title: requirement.title,
            description: requirement.description,
        }
    }
}

#[derive(Debug, Serialize)]
struct AuditorPortalEvidenceRequestResponse {
    mapping_rationale: String,
    mapping_created_at: DateTime<Utc>,
    request: EvidenceRequestResponse,
    submissions: Vec<AuditorPortalSubmissionResponse>,
}

impl From<AuditorPortalEvidenceRequest> for AuditorPortalEvidenceRequestResponse {
    fn from(request: AuditorPortalEvidenceRequest) -> Self {
        Self {
            mapping_rationale: request.mapping_rationale,
            mapping_created_at: request.mapping_created_at,
            request: request.request.into(),
            submissions: request.submissions.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Debug, Serialize)]
struct EvidenceRequestResponse {
    id: Uuid,
    workspace_id: Uuid,
    title: String,
    description: String,
    collection_instructions: String,
    cadence: &'static str,
    due_at: DateTime<Utc>,
    schedule_anchor_at: DateTime<Utc>,
    freshness_window_days: Option<i32>,
    status: &'static str,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<EvidenceRequest> for EvidenceRequestResponse {
    fn from(request: EvidenceRequest) -> Self {
        Self {
            id: Uuid::from(request.id),
            workspace_id: Uuid::from(request.workspace_id),
            title: request.title,
            description: request.description,
            collection_instructions: request.collection_instructions,
            cadence: request.cadence.as_str(),
            due_at: request.due_at,
            schedule_anchor_at: request.schedule_anchor_at,
            freshness_window_days: request.freshness_window_days,
            status: request.status.as_str(),
            created_at: request.created_at,
            updated_at: request.updated_at,
        }
    }
}

#[derive(Debug, Serialize)]
struct AuditorPortalSubmissionResponse {
    submission: EvidenceSubmissionResponse,
    attachments: Vec<AuditorPortalAttachmentResponse>,
}

impl From<AuditorPortalSubmission> for AuditorPortalSubmissionResponse {
    fn from(submission: AuditorPortalSubmission) -> Self {
        Self {
            submission: submission.submission.into(),
            attachments: submission.attachments.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Debug, Serialize)]
struct EvidenceSubmissionResponse {
    id: Uuid,
    evidence_request_id: Uuid,
    submitted_by: EvidenceSubmitterResponse,
    received_at: DateTime<Utc>,
    coverage_start_at: DateTime<Utc>,
    coverage_end_at: DateTime<Utc>,
    source_system: String,
    collection_method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
}

impl From<EvidenceSubmission> for EvidenceSubmissionResponse {
    fn from(submission: EvidenceSubmission) -> Self {
        Self {
            id: Uuid::from(submission.id),
            evidence_request_id: Uuid::from(submission.evidence_request_id),
            submitted_by: EvidenceSubmitterResponse::from(submission.submitted_by),
            received_at: submission.received_at,
            coverage_start_at: submission.coverage_start_at,
            coverage_end_at: submission.coverage_end_at,
            source_system: submission.source_system,
            collection_method: submission.collection_method,
            summary: submission.summary,
            description: submission.description,
        }
    }
}

#[derive(Debug, Serialize)]
struct EvidenceSubmitterResponse {
    agent_connection_id: Option<Uuid>,
    user_id: Uuid,
}

impl From<crate::domain::EvidenceSubmitter> for EvidenceSubmitterResponse {
    fn from(submitter: crate::domain::EvidenceSubmitter) -> Self {
        Self {
            agent_connection_id: submitter.agent_connection_id().map(Uuid::from),
            user_id: Uuid::from(submitter.user_id()),
        }
    }
}

#[derive(Debug, Serialize)]
struct AuditorPortalAttachmentResponse {
    id: Uuid,
    evidence_submission_id: Uuid,
    filename: String,
    content_type: String,
    content_length: i64,
    checksum_sha256: String,
    checksum_crc32c: String,
    upload_status: &'static str,
    download_eligible: bool,
}

impl From<AuditorPortalAttachment> for AuditorPortalAttachmentResponse {
    fn from(attachment: AuditorPortalAttachment) -> Self {
        Self {
            id: Uuid::from(attachment.id),
            evidence_submission_id: Uuid::from(attachment.evidence_submission_id),
            filename: attachment.filename,
            content_type: attachment.content_type,
            content_length: attachment.content_length,
            checksum_sha256: attachment.checksum_sha256,
            checksum_crc32c: attachment.checksum_crc32c,
            upload_status: attachment.upload_status.as_str(),
            download_eligible: attachment.download_eligible,
        }
    }
}
