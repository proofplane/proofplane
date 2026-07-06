use axum::{
    body::Body,
    extract::{rejection::QueryRejection, Form, Path, Query, State},
    http::{
        header::{
            CACHE_CONTROL, CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_TYPE, COOKIE, LOCATION,
            SET_COOKIE,
        },
        HeaderMap, HeaderName, HeaderValue, StatusCode,
    },
    response::{Html, IntoResponse, Response},
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

#[derive(Debug, Deserialize)]
struct InviteQuery {
    token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct BrowserInviteForm {
    token: String,
}

#[derive(Debug, Deserialize)]
struct BrowserVerifyForm {
    token: String,
    code: String,
}

#[derive(Debug, Serialize)]
struct StatusResponse {
    status: &'static str,
}

pub fn router(state: AuditorAccessState) -> Router {
    Router::new()
        .route("/auditor-access/portal", get(portal_page))
        .route("/auditor-access/portal/data", get(portal_data))
        .route(
            "/auditor-access/portal/{*download_path}",
            get(download_attachment),
        )
        .route("/auditor-access/{workspace_id}", get(open_invite))
        .route(
            "/auditor-access/{workspace_id}/otp/request",
            post(request_otp),
        )
        .route(
            "/auditor-access/{workspace_id}/otp/verify",
            post(verify_otp),
        )
        .route(
            "/auditor-access/{workspace_id}/otp/request/browser",
            post(request_otp_browser),
        )
        .route(
            "/auditor-access/{workspace_id}/otp/verify/browser",
            post(verify_otp_browser),
        )
        .route("/auditor-access/logout", post(logout))
        .with_state(state)
}

async fn open_invite(
    State(state): State<AuditorAccessState>,
    Path(workspace_id): Path<Uuid>,
    query: Result<Query<InviteQuery>, QueryRejection>,
) -> Result<Response, ApiError> {
    let Query(query) = match query {
        Ok(query) => query,
        Err(_) => return Ok(unavailable_response()),
    };
    let Some(token) = query.token.filter(|token| !token.trim().is_empty()) else {
        return Ok(unavailable_response());
    };
    let grant = match state.grants.load_for_use(workspace_id.into(), &token).await {
        Ok(grant) => grant,
        Err(AuditorAccessGrantError::Unavailable | AuditorAccessGrantError::Denied) => {
            return Ok(unavailable_response());
        }
        Err(error) => return Err(grant_error(error)),
    };

    Ok(Html(render_invite_page(
        workspace_id,
        &token,
        &grant.auditor_email,
        None,
    ))
    .into_response())
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

async fn request_otp_browser(
    State(state): State<AuditorAccessState>,
    Extension(request_id): Extension<RequestId>,
    Path(workspace_id): Path<Uuid>,
    Form(payload): Form<BrowserInviteForm>,
) -> Result<Response, ApiError> {
    let token = payload.token.trim();
    let grant = match state.grants.load_for_use(workspace_id.into(), token).await {
        Ok(grant) => grant,
        Err(AuditorAccessGrantError::Unavailable | AuditorAccessGrantError::Denied) => {
            return Ok(unavailable_response());
        }
        Err(error) => return Err(grant_error(error)),
    };

    match state.sessions.request_otp(&grant).await {
        Ok(()) => {
            audit(
                "auditor_access_otp.requested",
                "request_auditor_access_otp",
                request_id.0,
                workspace_id,
                Uuid::from(grant.id),
                &grant.auditor_email,
            );
            Ok(Html(render_verify_page(
                workspace_id,
                token,
                &grant.auditor_email,
                Some("Code sent. Check the intended auditor inbox."),
            ))
            .into_response())
        }
        Err(AuditorAccessSessionError::RateLimited) => Ok((
            StatusCode::CONFLICT,
            Html(render_verify_page(
                workspace_id,
                token,
                &grant.auditor_email,
                Some("Too many code requests. Use the latest code or wait before trying again."),
            )),
        )
            .into_response()),
        Err(error) => Err(session_error(error)),
    }
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

async fn verify_otp_browser(
    State(state): State<AuditorAccessState>,
    Extension(request_id): Extension<RequestId>,
    Path(workspace_id): Path<Uuid>,
    Form(payload): Form<BrowserVerifyForm>,
) -> Result<Response, ApiError> {
    let token = payload.token.trim();
    let grant = match state.grants.load_for_use(workspace_id.into(), token).await {
        Ok(grant) => grant,
        Err(AuditorAccessGrantError::Unavailable | AuditorAccessGrantError::Denied) => {
            return Ok(unavailable_response());
        }
        Err(error) => return Err(grant_error(error)),
    };
    let created = match state.sessions.verify_otp(&grant, payload.code.trim()).await {
        Ok(created) => created,
        Err(AuditorAccessSessionError::Unavailable) => {
            return Ok((
                StatusCode::NOT_FOUND,
                Html(render_verify_page(
                    workspace_id,
                    token,
                    &grant.auditor_email,
                    Some("That code could not be verified. Request a new code if it expired."),
                )),
            )
                .into_response());
        }
        Err(error) => return Err(session_error(error)),
    };

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

    let mut response = StatusCode::SEE_OTHER.into_response();
    response
        .headers_mut()
        .insert(LOCATION, HeaderValue::from_static("/auditor-access/portal"));
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

async fn portal_page(
    State(state): State<AuditorAccessState>,
    Extension(request_id): Extension<RequestId>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let Some(raw_session) = auditor_session_cookie(&headers) else {
        return Ok(unavailable_response());
    };
    let session = match state.sessions.load_session(raw_session).await {
        Ok(session) => session,
        Err(AuditorAccessSessionError::Unavailable) => return Ok(unavailable_response()),
        Err(error) => return Err(session_error(error)),
    };
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

    Ok(Html(render_portal_page(&model)).into_response())
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

fn render_invite_page(
    workspace_id: Uuid,
    token: &str,
    auditor_email: &str,
    message: Option<&str>,
) -> String {
    render_shell(
        "Auditor access",
        &format!(
            r#"<main class="narrow">
<p class="eyebrow">Auditor verification</p>
<h1>Verify access for {}</h1>
<p class="lede">Proofplane will send a single-use code to this email before opening the read-only evidence portal.</p>
{}
<form class="panel form-panel" method="post" action="/auditor-access/{}/otp/request/browser">
<input type="hidden" name="token" value="{}">
<button type="submit">Send verification code</button>
</form>
</main>"#,
            escape_html(auditor_email),
            notice(message),
            workspace_id,
            escape_html(token),
        ),
    )
}

fn render_verify_page(
    workspace_id: Uuid,
    token: &str,
    auditor_email: &str,
    message: Option<&str>,
) -> String {
    render_shell(
        "Enter auditor code",
        &format!(
            r#"<main class="narrow">
<p class="eyebrow">Code required</p>
<h1>Enter the code sent to {}</h1>
<p class="lede">Codes expire after 10 minutes. A successful check creates a seven-day browser session for this portal only.</p>
{}
<form class="panel form-panel" method="post" action="/auditor-access/{}/otp/verify/browser">
<input type="hidden" name="token" value="{}">
<label for="code">Verification code</label>
<input id="code" name="code" inputmode="numeric" autocomplete="one-time-code" pattern="[0-9]{{6}}" maxlength="6" required>
<button type="submit">Open portal</button>
</form>
</main>"#,
            escape_html(auditor_email),
            notice(message),
            workspace_id,
            escape_html(token),
        ),
    )
}

fn render_portal_page(model: &AuditorPortalReadModel) -> String {
    let control_count = model.controls.len();
    let controls = if model.controls.is_empty() {
        r#"<p class="empty">No mapped controls are available for this auditor portal.</p>"#
            .to_owned()
    } else {
        model
            .controls
            .iter()
            .map(render_control)
            .collect::<Vec<_>>()
            .join("")
    };

    render_shell(
        "Auditor portal",
        &format!(
            r#"<main class="portal">
<header class="portal-header">
<div>
<p class="eyebrow">Auditor portal</p>
<h1>{}</h1>
<p class="lede">Read-only review of mapped controls, framework requirements, evidence requests, submissions, and attachments.</p>
</div>
<dl class="session-meta">
<div><dt>Auditor</dt><dd>{}</dd></div>
<div><dt>Workspace</dt><dd>{}</dd></div>
<div><dt>Controls</dt><dd>{}</dd></div>
</dl>
</header>
<div class="section-heading">
<p class="eyebrow">Review scope</p>
<h2>Controls and evidence</h2>
</div>
<section class="control-list" aria-label="Workspace controls">
{}
</section>
</main>"#,
            escape_html(&model.workspace_name),
            escape_html(&model.auditor_email),
            escape_html(&model.workspace_name),
            control_count,
            controls,
        ),
    )
}

fn render_control(control: &AuditorPortalControl) -> String {
    let requirements = if control.framework_requirements.is_empty() {
        r#"<p class="empty compact">No framework requirements are mapped to this control.</p>"#
            .to_owned()
    } else {
        format!(
            r#"<div class="requirement-list">{}</div>"#,
            control
                .framework_requirements
                .iter()
                .map(render_framework_requirement)
                .collect::<Vec<_>>()
                .join("")
        )
    };
    let requests = if control.evidence_requests.is_empty() {
        r#"<p class="empty">No evidence requests are mapped to this control.</p>"#.to_owned()
    } else {
        control
            .evidence_requests
            .iter()
            .map(render_evidence_request)
            .collect::<Vec<_>>()
            .join("")
    };

    format!(
        r#"<article class="control">
<header class="control-heading">
<div>
<p class="object-label">Control</p>
<h2><span class="control-code">{}</span>{}</h2>
<p>{}</p>
</div>
</header>
<section class="framework-panel" aria-label="Framework coverage for {}">
<p class="object-label">Framework coverage</p>
{}
</section>
<section class="evidence-panel" aria-label="Evidence mapped to {}">
<p class="object-label">Evidence requests</p>
<div class="request-list">{}</div>
</section>
</article>"#,
        escape_html(&control.code),
        escape_html(&control.title),
        escape_html(&control.description),
        escape_html(&control.title),
        requirements,
        escape_html(&control.title),
        requests,
    )
}

fn render_framework_requirement(requirement: &FrameworkRequirement) -> String {
    format!(
        r#"<article class="requirement">
<dl class="details">
<div><dt>Framework</dt><dd>{}</dd></div>
<div><dt>Framework requirement</dt><dd><span class="requirement-code">{}</span>{}</dd></div>
</dl>
<p>{}</p>
</article>"#,
        escape_html(&requirement.framework_name),
        escape_html(&requirement.code),
        escape_html(&requirement.title),
        escape_html(&requirement.description),
    )
}

fn render_evidence_request(request: &AuditorPortalEvidenceRequest) -> String {
    let submissions = if request.submissions.is_empty() {
        r#"<p class="empty">No submissions have been received for this request.</p>"#.to_owned()
    } else {
        request
            .submissions
            .iter()
            .map(render_submission)
            .collect::<Vec<_>>()
            .join("")
    };

    format!(
        r#"<section class="request" aria-labelledby="request-{}">
<div class="request-heading">
<div>
<p class="object-label">Evidence request</p>
<h3 id="request-{}">{}</h3>
<p>{}</p>
</div>
<span class="status-chip">Status: {}</span>
</div>
<dl class="details">
<div><dt>Due date</dt><dd>{}</dd></div>
<div><dt>Cadence</dt><dd>{}</dd></div>
<div><dt>Mapped to control</dt><dd>{}</dd></div>
</dl>
<dl class="mapping"><dt>Control mapping rationale</dt><dd>{}</dd></dl>
<p class="object-label submissions-label">Evidence submissions</p>
<div class="submission-list">{}</div>
</section>"#,
        Uuid::from(request.request.id),
        Uuid::from(request.request.id),
        escape_html(&request.request.title),
        escape_html(&request.request.description),
        escape_html(request.request.status.as_str()),
        format_date(request.request.due_at),
        escape_html(request.request.cadence.as_str()),
        format_date(request.mapping_created_at),
        escape_html(&request.mapping_rationale),
        submissions,
    )
}

fn render_submission(submission: &AuditorPortalSubmission) -> String {
    let attachments = if submission.attachments.is_empty() {
        r#"<p class="empty compact">No attachments are available for this submission.</p>"#
            .to_owned()
    } else {
        format!(
            r#"<table><caption>Evidence attachments</caption><thead><tr><th>Attachment</th><th>Size</th><th>Status</th><th>Action</th></tr></thead><tbody>{}</tbody></table>"#,
            submission
                .attachments
                .iter()
                .map(render_attachment)
                .collect::<Vec<_>>()
                .join("")
        )
    };
    let summary = submission
        .submission
        .summary
        .as_deref()
        .map(|summary| format!(r#"<p>{}</p>"#, escape_html(summary)))
        .unwrap_or_default();
    let description = submission
        .submission
        .description
        .as_deref()
        .map(|description| format!(r#"<p class="muted">{}</p>"#, escape_html(description)))
        .unwrap_or_default();

    format!(
        r#"<article class="submission">
<header>
<p class="object-label">Evidence submission</p>
<h4>Received {}</h4>
</header>
<dl class="details">
<div><dt>Coverage period</dt><dd>{} to {}</dd></div>
<div><dt>Source system</dt><dd>{}</dd></div>
<div><dt>Collection method</dt><dd>{}</dd></div>
</dl>
{}
{}
{}
</article>"#,
        format_datetime(submission.submission.received_at),
        format_date(submission.submission.coverage_start_at),
        format_date(submission.submission.coverage_end_at),
        escape_html(&submission.submission.source_system),
        escape_html(&submission.submission.collection_method),
        summary,
        description,
        attachments,
    )
}

fn render_attachment(attachment: &AuditorPortalAttachment) -> String {
    let action = if attachment.download_eligible {
        format!(
            r#"<a class="button" href="/auditor-access/portal/evidence-submissions/{}/attachments/{}/download">Download evidence</a>"#,
            Uuid::from(attachment.evidence_submission_id),
            Uuid::from(attachment.id),
        )
    } else {
        "Unavailable".to_owned()
    };

    format!(
        r#"<tr><td data-label="Attachment">{}</td><td data-label="Size">{}</td><td data-label="Status">{}</td><td data-label="Action">{}</td></tr>"#,
        escape_html(&attachment.filename),
        format_bytes(attachment.content_length),
        escape_html(attachment.upload_status.as_str()),
        action,
    )
}

fn render_shell(title: &str, body: &str) -> String {
    format!(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{}</title>
<style>
:root {{
  color-scheme: dark;
  --canvas: oklch(17% 0.012 170);
  --surface: oklch(24% 0.014 170);
  --surface-raised: oklch(30% 0.018 170);
  --line: oklch(39% 0.018 170);
  --ink: oklch(94% 0.01 150);
  --muted: oklch(76% 0.015 155);
  --accent: oklch(78% 0.09 174);
  --signal: oklch(78% 0.08 48);
}}
* {{ box-sizing: border-box; }}
body {{
  margin: 0;
  min-height: 100vh;
  background: var(--canvas);
  color: var(--ink);
  font-family: Inter, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
}}
main {{ width: min(1120px, calc(100% - 32px)); margin: 0 auto; padding: 48px 0; }}
.narrow {{ width: min(640px, calc(100% - 32px)); padding-top: 72px; }}
h1, h2, h3, h4, p {{ margin-top: 0; letter-spacing: 0; }}
h1 {{ margin-bottom: 12px; font-size: 2rem; line-height: 1.1; }}
h2 {{ margin-bottom: 8px; font-size: 1.25rem; line-height: 1.2; }}
h3 {{ margin-bottom: 6px; font-size: 1.05rem; line-height: 1.25; }}
h4 {{ margin-bottom: 6px; font-size: 0.95rem; line-height: 1.25; }}
p {{ color: var(--muted); line-height: 1.55; }}
.lede {{ max-width: 68ch; }}
.eyebrow, label, dt, th, .object-label, caption {{ color: var(--muted); font-size: 0.8125rem; font-weight: 620; line-height: 1.2; }}
.eyebrow {{ margin-bottom: 8px; color: var(--accent); }}
.object-label {{ margin-bottom: 8px; }}
.panel, .control, .request, .submission, .requirement {{
  border: 1px solid var(--line);
  background: var(--surface);
  border-radius: 8px;
}}
.form-panel {{ display: grid; gap: 14px; margin-top: 24px; padding: 24px; }}
.notice {{
  border: 1px solid color-mix(in oklch, var(--accent) 45%, var(--line));
  background: oklch(27% 0.03 170);
  border-radius: 8px;
  padding: 14px 16px;
  margin-top: 22px;
}}
.notice p {{ margin: 0; }}
input {{
  width: 100%;
  border: 1px solid var(--line);
  background: var(--surface-raised);
  color: var(--ink);
  border-radius: 6px;
  padding: 10px 12px;
  font: inherit;
}}
button, .button {{
  display: inline-block;
  justify-self: start;
  border: 0;
  border-radius: 6px;
  background: var(--accent);
  color: var(--canvas);
  padding: 10px 16px;
  font-size: 0.8125rem;
  font-weight: 700;
  text-decoration: none;
  cursor: pointer;
}}
button:hover, .button:hover {{ background: oklch(72% 0.09 174); }}
button:focus-visible, .button:focus-visible, input:focus-visible {{ outline: 2px solid var(--signal); outline-offset: 2px; }}
.portal-header {{ display: flex; justify-content: space-between; gap: 24px; align-items: end; margin-bottom: 28px; }}
.section-heading {{ margin-bottom: 14px; }}
.session-meta, .details {{ display: flex; flex-wrap: wrap; gap: 14px; margin: 0; }}
.session-meta div, .details div {{ min-width: 120px; }}
dd {{ margin: 4px 0 0; }}
.session-meta dd, .details dd {{ color: var(--ink); }}
.control-list {{ display: grid; gap: 20px; }}
.control {{ padding: 22px; }}
.control-heading {{ margin-bottom: 18px; }}
.control-code, .requirement-code {{ display: inline-block; margin-right: 8px; color: var(--accent); font-weight: 700; }}
.framework-panel, .evidence-panel {{ border-top: 1px solid var(--line); padding-top: 18px; margin-top: 18px; }}
.requirement-list {{ display: grid; gap: 10px; }}
.requirement {{ padding: 14px; background: var(--surface-raised); }}
.requirement p {{ margin: 10px 0 0; }}
.chips {{ display: flex; flex-wrap: wrap; gap: 8px; padding: 0; margin: 14px 0 0; list-style: none; }}
.chips li, .status-chip {{
  border-radius: 4px;
  background: oklch(27% 0.035 170);
  color: var(--accent);
  padding: 5px 8px;
  font-size: 0.8125rem;
  font-weight: 620;
}}
.request-list {{ display: grid; gap: 14px; margin-top: 18px; }}
.request {{ padding: 18px; background: var(--surface-raised); }}
.request-heading {{ display: flex; justify-content: space-between; gap: 16px; align-items: start; }}
.mapping {{ margin: 14px 0 0; }}
.mapping dd {{ color: var(--ink); line-height: 1.5; }}
.submissions-label {{ margin-top: 18px; }}
.submission-list {{ display: grid; gap: 12px; margin-top: 16px; }}
.submission {{ padding: 16px; background: var(--surface); }}
.muted, .empty {{ color: var(--muted); }}
.compact {{ margin-bottom: 0; }}
table {{ width: 100%; border-collapse: collapse; margin-top: 12px; }}
caption {{ caption-side: top; margin-bottom: 8px; text-align: left; color: var(--accent); }}
th, td {{ padding: 11px 10px; text-align: left; vertical-align: middle; border-bottom: 1px solid var(--line); }}
th {{ background: var(--surface-raised); }}
td {{ font-size: 0.95rem; }}
@media (max-width: 720px) {{
  main {{ width: min(100% - 24px, 1120px); padding: 32px 0; }}
  .portal-header, .request-heading {{ display: block; }}
  .session-meta {{ margin-top: 16px; }}
  .control-heading {{ display: block; }}
  table, thead, tbody, tr, th, td {{ display: block; }}
  thead {{ display: none; }}
  td {{ border-bottom: 0; padding: 8px 0; }}
  td::before {{ content: attr(data-label); display: block; color: var(--muted); font-size: 0.8125rem; font-weight: 620; margin-bottom: 3px; }}
  tr {{ border-top: 1px solid var(--line); padding: 10px 0; }}
}}
</style>
</head>
<body>
{}
</body>
</html>"#,
        escape_html(title),
        body,
    )
}

fn unavailable_page() -> String {
    render_shell(
        "Auditor access unavailable",
        r#"<main class="narrow">
<p class="eyebrow">Access unavailable</p>
<h1>This auditor portal is not available</h1>
<p class="lede">The link or session may be expired or revoked. Ask the Proofplane workspace owner for a new auditor access link.</p>
</main>"#,
    )
}

fn unavailable_response() -> Response {
    (StatusCode::NOT_FOUND, Html(unavailable_page())).into_response()
}

fn notice(message: Option<&str>) -> String {
    message
        .map(|message| {
            format!(
                r#"<section class="notice" role="status"><p>{}</p></section>"#,
                escape_html(message)
            )
        })
        .unwrap_or_default()
}

fn format_bytes(bytes: i64) -> String {
    const KB: i64 = 1024;
    const MB: i64 = 1024 * KB;
    if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{bytes} B")
    }
}

fn format_date(value: DateTime<Utc>) -> String {
    value.format("%Y-%m-%d").to_string()
}

fn format_datetime(value: DateTime<Utc>) -> String {
    value.format("%Y-%m-%d %H:%M UTC").to_string()
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
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
    workspace_name: String,
    auditor_email: String,
    controls: Vec<AuditorPortalControlResponse>,
}

impl From<AuditorPortalReadModel> for AuditorPortalReadModelResponse {
    fn from(model: AuditorPortalReadModel) -> Self {
        Self {
            workspace_name: model.workspace_name,
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
    framework_code: String,
    framework_name: String,
    code: String,
    title: String,
    description: String,
}

impl From<FrameworkRequirement> for FrameworkRequirementResponse {
    fn from(requirement: FrameworkRequirement) -> Self {
        Self {
            id: Uuid::from(requirement.id),
            framework_id: Uuid::from(requirement.framework_id),
            framework_code: requirement.framework_code,
            framework_name: requirement.framework_name,
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
