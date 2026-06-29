use axum::{
    extract::{rejection::QueryRejection, DefaultBodyLimit, Multipart, Query, State},
    http::{
        header::{COOKIE, LOCATION, SET_COOKIE},
        HeaderMap, HeaderValue, StatusCode,
    },
    response::{Html, IntoResponse, Response},
    routing::{get, post},
    Extension, Router,
};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use uuid::Uuid;

use crate::{
    authentication::ApiTokenContext,
    domain::{
        AttachmentUploadStatus, EvidenceAttachment, EvidenceSubmissionDetail, EvidenceSubmissionId,
    },
    observability::audit::{AuditActor, AuditClientType, AuditEvent, AuditObject, AuditOutcome},
    routes::{
        error::ApiError,
        evidence_submissions::{attachment_upload_from_multipart, AttachmentUploadDigest},
        request_context::RequestId,
    },
    services::{
        attachment_upload_grants::{AttachmentUploadGrantService, UploadGrantError},
        evidence_submissions::EvidenceSubmissionService,
        upload_sessions::{UploadSessionError, UploadSessionTokenService, VerifiedUploadSession},
    },
};

const UPLOAD_SESSION_COOKIE: &str = "proofplane_attachment_upload_session";

#[derive(Clone)]
pub struct AttachmentUploadSessionState {
    pub grants: AttachmentUploadGrantService,
    pub sessions: UploadSessionTokenService,
    pub submissions: EvidenceSubmissionService,
    pub secure_cookie: bool,
    pub max_attachment_bytes: usize,
}

pub fn router(state: AttachmentUploadSessionState) -> Router {
    Router::new()
        .route("/evidence-attachment-uploads", get(open_upload_session))
        .route(
            "/evidence-attachment-uploads/files",
            post(upload_file).layer(DefaultBodyLimit::max(state.max_attachment_bytes)),
        )
        .with_state(state)
}

#[derive(Debug, Deserialize)]
struct UploadSessionQuery {
    token: Option<String>,
    uploaded: Option<String>,
}

#[derive(Debug)]
struct UploadSessionAttachmentResponse {
    filename: String,
    content_length: i64,
    upload_status: String,
}

async fn open_upload_session(
    State(state): State<AttachmentUploadSessionState>,
    headers: HeaderMap,
    query: Result<Query<UploadSessionQuery>, QueryRejection>,
) -> Result<Response, ApiError> {
    let Query(query) = match query {
        Ok(query) => query,
        Err(_) => return Ok(unavailable_response()),
    };
    if let Some(token) = query.token {
        return redeem_grant(state, token).await;
    }

    let session = match verify_session(&state, &headers) {
        Ok(session) => session,
        Err(UploadSessionError::Unavailable) => return Ok(unavailable_response()),
        Err(UploadSessionError::Internal) => return Err(ApiError::Internal),
    };
    if query.uploaded.is_some() {
        return Ok(Html(success_page()).into_response());
    }
    let body = render_upload_page(
        &inventory(
            &state.submissions,
            session.submission_id,
            session.api_token_context(),
        )
        .await?,
        None,
    );
    Ok(Html(body).into_response())
}

async fn upload_file(
    State(state): State<AttachmentUploadSessionState>,
    headers: HeaderMap,
    Extension(request_id): Extension<RequestId>,
    multipart: Multipart,
) -> Result<Response, ApiError> {
    let session = match verify_session(&state, &headers) {
        Ok(session) => session,
        Err(UploadSessionError::Unavailable) => return Ok(unavailable_response()),
        Err(UploadSessionError::Internal) => return Err(ApiError::Internal),
    };
    let token = session.api_token_context();
    let before = detail(&state.submissions, session.submission_id, token).await?;
    if !before.attachments.is_empty() {
        return Ok(upload_already_exists_response(&before));
    }

    let payload = match attachment_upload_from_multipart(
        &state.submissions,
        &token,
        session.submission_id,
        multipart,
        AttachmentUploadDigest::ComputeOnly,
    )
    .await
    {
        Ok(payload) => payload,
        Err(error @ (ApiError::BadRequest(_) | ApiError::PayloadTooLarge)) => {
            return Ok(upload_error_response(&before, error));
        }
        Err(error) => return Err(error),
    };

    let Some(attachment) = state
        .submissions
        .create_first_attachment(&token, request_id.0, session.submission_id, payload)
        .await?
    else {
        let latest = detail(&state.submissions, session.submission_id, token).await?;
        return Ok(upload_already_exists_response(&latest));
    };
    emit_upload_audit(&token, request_id.0, session.submission_id, &attachment);

    let mut response = StatusCode::SEE_OTHER.into_response();
    response.headers_mut().insert(
        LOCATION,
        HeaderValue::from_static("/evidence-attachment-uploads?uploaded=1"),
    );
    Ok(response)
}

async fn redeem_grant(
    state: AttachmentUploadSessionState,
    token: String,
) -> Result<Response, ApiError> {
    if token.is_empty() {
        return Ok(unavailable_response());
    }

    let grant = match state.grants.redeem(&token).await {
        Ok(grant) => grant,
        Err(UploadGrantError::Unavailable) => return Ok(unavailable_response()),
        Err(UploadGrantError::Internal | UploadGrantError::Repository(_)) => {
            return Err(ApiError::Internal);
        }
    };
    let session = state
        .sessions
        .issue_until(
            grant.workspace_id,
            grant.submission_id,
            grant.issued_by_user_id,
            grant.issued_via_api_token_id,
            grant.expires_at,
        )
        .map_err(upload_session_error)?;
    let mut response = StatusCode::SEE_OTHER.into_response();
    response.headers_mut().insert(
        LOCATION,
        HeaderValue::from_static("/evidence-attachment-uploads"),
    );
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

async fn inventory(
    submissions: &EvidenceSubmissionService,
    submission_id: EvidenceSubmissionId,
    token: ApiTokenContext,
) -> Result<Vec<UploadSessionAttachmentResponse>, ApiError> {
    Ok(detail(submissions, submission_id, token)
        .await?
        .attachments
        .into_iter()
        .map(Into::into)
        .collect())
}

async fn detail(
    submissions: &EvidenceSubmissionService,
    submission_id: EvidenceSubmissionId,
    token: ApiTokenContext,
) -> Result<EvidenceSubmissionDetail, ApiError> {
    submissions
        .get(token, submission_id)
        .await
        .map_err(ApiError::from)?
        .ok_or_else(unavailable)
}

fn verify_session(
    state: &AttachmentUploadSessionState,
    headers: &HeaderMap,
) -> Result<VerifiedUploadSession, UploadSessionError> {
    let token = upload_session_cookie(headers).ok_or(UploadSessionError::Unavailable)?;
    state.sessions.verify(token)
}

fn emit_upload_audit(
    token: &ApiTokenContext,
    request_id: Uuid,
    submission_id: EvidenceSubmissionId,
    attachment: &EvidenceAttachment,
) {
    AuditEvent::new(
        "evidence_attachment.accepted",
        AuditOutcome::Success,
        AuditActor::ApiToken {
            user_id: token.user_id.into(),
            api_token_id: token.api_token_id.into(),
        },
        AuditClientType::Rest,
        "upload_evidence_attachment_via_upload_session",
    )
    .workspace_id(token.workspace_id.into())
    .request_id(request_id)
    .metadata("evidence_submission_id", Uuid::from(submission_id))
    .metadata("evidence_attachment_id", Uuid::from(attachment.id))
    .metadata("lifecycle_status", attachment.upload_status.as_str())
    .object(AuditObject::new(
        "evidence_attachment",
        attachment.id.into(),
    ))
    .emit();
}

fn render_upload_page(
    attachments: &[UploadSessionAttachmentResponse],
    message: Option<&str>,
) -> String {
    let message = message
        .map(|message| {
            format!(
                r#"<section class="notice"><strong>{}</strong><p>Ask the MCP client to check attachment processing status.</p></section>"#,
                escape_html(message)
            )
        })
        .unwrap_or_default();
    let rows = if attachments.is_empty() {
        r#"<p class="empty">No attachments have been added yet.</p>"#.to_owned()
    } else {
        format!(
            r#"<table><thead><tr><th>Filename</th><th>Size</th><th>Status</th></tr></thead><tbody>{}</tbody></table>"#,
            attachments
                .iter()
                .map(|attachment| format!(
                    "<tr><td>{}</td><td>{}</td><td>{}</td></tr>",
                    escape_html(&attachment.filename),
                    format_bytes(attachment.content_length),
                    escape_html(&attachment.upload_status),
                ))
                .collect::<Vec<_>>()
                .join("")
        )
    };
    let upload = if attachments.is_empty() {
        r#"<section class="panel" aria-labelledby="upload-file">
<h2 id="upload-file">Upload a file</h2>
<form method="post" action="/evidence-attachment-uploads/files" enctype="multipart/form-data">
<label for="file">File</label>
<input id="file" name="file" type="file" required>
<button type="submit">Upload</button>
</form>
</section>"#
            .to_owned()
    } else {
        String::new()
    };

    format!(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Evidence attachment upload</title>
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
main {{
  width: min(920px, calc(100% - 32px));
  margin: 0 auto;
  padding: 56px 0;
}}
h1 {{ margin: 0 0 10px; font-size: 1.75rem; line-height: 1.15; letter-spacing: 0; }}
p {{ color: var(--muted); line-height: 1.55; max-width: 68ch; }}
.panel {{
  border: 1px solid var(--line);
  background: var(--surface);
  border-radius: 8px;
  padding: 24px;
  margin-top: 24px;
}}
.notice {{
  border: 1px solid color-mix(in oklch, var(--accent) 50%, var(--line));
  background: oklch(27% 0.03 170);
  border-radius: 8px;
  padding: 16px;
  margin-top: 24px;
}}
.notice p {{ margin: 6px 0 0; }}
table {{ width: 100%; border-collapse: collapse; margin-top: 12px; }}
th, td {{ padding: 12px 10px; text-align: left; border-bottom: 1px solid var(--line); }}
th {{ color: var(--muted); font-size: 0.8125rem; font-weight: 620; }}
td {{ font-size: 0.95rem; }}
.empty {{ margin: 12px 0 0; }}
form {{ display: grid; gap: 14px; margin-top: 16px; max-width: 520px; }}
label {{ font-size: 0.8125rem; font-weight: 620; color: var(--muted); }}
input[type="file"] {{
  width: 100%;
  border: 1px solid var(--line);
  background: var(--surface-raised);
  border-radius: 6px;
  padding: 10px;
  color: var(--ink);
}}
input[type="file"]:focus-visible, button:focus-visible {{
  outline: 2px solid var(--signal);
  outline-offset: 2px;
}}
button {{
  justify-self: start;
  border: 0;
  border-radius: 6px;
  background: var(--accent);
  color: var(--canvas);
  padding: 10px 16px;
  font-weight: 700;
  cursor: pointer;
}}
button:hover {{ background: oklch(72% 0.09 174); }}
</style>
</head>
<body>
<main>
<h1>Evidence attachment upload</h1>
<p>Add one evidence file for the scoped submission. If you have to upload more than one file, put them in the same zip file and upload the zip. This link does not grant access to the rest of Proofplane.</p>
{message}
<section class="panel" aria-labelledby="current-attachments">
<h2 id="current-attachments">Current attachments</h2>
{rows}
</section>
{upload}
</main>
</body>
</html>"#
    )
}

fn success_page() -> String {
    r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Upload successful</title>
<style>
body { margin: 0; min-height: 100vh; display: grid; place-items: center; background: oklch(17% 0.012 170); color: oklch(94% 0.01 150); font-family: Inter, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif; }
main { width: min(560px, calc(100% - 32px)); border: 1px solid oklch(39% 0.018 170); background: oklch(24% 0.014 170); border-radius: 8px; padding: 24px; }
h1 { margin: 0 0 10px; font-size: 1.5rem; letter-spacing: 0; }
p { margin: 0; color: oklch(76% 0.015 155); line-height: 1.55; }
</style>
</head>
<body><main><h1>Upload successful</h1><p>You can safely close the page now.</p></main></body>
</html>"#
        .to_owned()
}

fn upload_error_response(detail: &EvidenceSubmissionDetail, error: ApiError) -> Response {
    let (status, message) = match error {
        ApiError::BadRequest(details) => details
            .into_iter()
            .next()
            .map(|message| (StatusCode::BAD_REQUEST, message))
            .unwrap_or_else(|| (StatusCode::BAD_REQUEST, "Upload failed".to_owned())),
        ApiError::PayloadTooLarge => (
            StatusCode::PAYLOAD_TOO_LARGE,
            "file is too large".to_owned(),
        ),
        _ => (StatusCode::BAD_REQUEST, "Upload failed".to_owned()),
    };
    let attachments = detail
        .attachments
        .clone()
        .into_iter()
        .map(Into::into)
        .collect::<Vec<_>>();

    (
        status,
        Html(render_upload_page(
            &attachments,
            Some(&format!("Upload failed: {message}")),
        )),
    )
        .into_response()
}

fn upload_already_exists_response(detail: &EvidenceSubmissionDetail) -> Response {
    let attachments = detail
        .attachments
        .clone()
        .into_iter()
        .map(Into::into)
        .collect::<Vec<_>>();

    (
        StatusCode::CONFLICT,
        Html(render_upload_page(
            &attachments,
            Some("A file has already been uploaded for this evidence submission."),
        )),
    )
        .into_response()
}

fn unavailable_page() -> String {
    r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Upload link unavailable</title>
<style>
body { margin: 0; min-height: 100vh; display: grid; place-items: center; background: oklch(17% 0.012 170); color: oklch(94% 0.01 150); font-family: Inter, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif; }
main { width: min(560px, calc(100% - 32px)); border: 1px solid oklch(39% 0.018 170); background: oklch(24% 0.014 170); border-radius: 8px; padding: 24px; }
h1 { margin: 0 0 10px; font-size: 1.5rem; letter-spacing: 0; }
p { margin: 0; color: oklch(76% 0.015 155); line-height: 1.55; }
</style>
</head>
<body><main><h1>This upload link is no longer available</h1><p>Ask the MCP client for a new upload link.</p></main></body>
</html>"#
        .to_owned()
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

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
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

fn session_cookie(token: &str, secure: bool, expires_at: DateTime<Utc>) -> String {
    let max_age = (expires_at - Utc::now()).num_seconds().max(1);
    let mut cookie = format!(
        "{UPLOAD_SESSION_COOKIE}={token}; HttpOnly; SameSite=Lax; Path=/evidence-attachment-uploads; Max-Age={}",
        max_age
    );
    if secure {
        cookie.push_str("; Secure");
    }
    cookie
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

fn unavailable_response() -> Response {
    (StatusCode::NOT_FOUND, Html(unavailable_page())).into_response()
}

impl From<EvidenceAttachment> for UploadSessionAttachmentResponse {
    fn from(attachment: EvidenceAttachment) -> Self {
        Self {
            filename: attachment.filename,
            content_length: attachment.content_length,
            upload_status: upload_status(attachment.upload_status).to_owned(),
        }
    }
}

fn upload_status(status: AttachmentUploadStatus) -> &'static str {
    status.as_str()
}
