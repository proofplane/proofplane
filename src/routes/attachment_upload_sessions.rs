use axum::{
    extract::{
        multipart::{Field, MultipartError},
        rejection::QueryRejection,
        DefaultBodyLimit, Multipart, Path, Query, State,
    },
    http::{
        header::{COOKIE, LOCATION, SET_COOKIE},
        HeaderMap, HeaderValue, StatusCode,
    },
    response::{Html, IntoResponse, Response},
    routing::{get, post},
    Extension, Router,
};
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use bytes::Bytes;
use chrono::{DateTime, Utc};
use futures_core::Stream;
use futures_util::stream;
use serde::Deserialize;
use std::sync::{
    atomic::{AtomicU32, Ordering},
    Arc,
};
use uuid::Uuid;

use crate::{
    domain::{
        validate_attachment_filename, AttachmentUploadStatus, EvidenceAttachment,
        EvidenceAttachmentId, EvidenceSubmissionDetail, EvidenceSubmissionId,
    },
    observability::audit::{AuditActor, AuditClientType, AuditEvent, AuditObject, AuditOutcome},
    repository::ArchiveAttachmentResult,
    routes::{
        error::{domain_errors, ApiError},
        request_context::RequestId,
    },
    services::{
        agent_connections::AgentConnectionContext,
        attachment_downloads::DownloadGrantIssuer,
        attachment_downloads::{AttachmentDownloadService, DownloadError},
        attachment_upload_grants::{AttachmentUploadGrantService, UploadGrantError},
        evidence_submissions::{EvidenceSubmissionService, UploadEvidenceAttachmentPayload},
        upload_sessions::{
            UploadSessionError, UploadSessionIssuer, UploadSessionTokenService,
            VerifiedUploadSession,
        },
    },
};

const UPLOAD_SESSION_COOKIE: &str = "proofplane_attachment_upload_session";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AttachmentUploadDigest {
    ComputeOnly,
}

#[derive(Clone)]
pub struct AttachmentUploadSessionState {
    pub grants: AttachmentUploadGrantService,
    pub downloads: AttachmentDownloadService,
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
        .route(
            "/evidence-attachment-uploads/files/{attachment_id}/download",
            get(download_file),
        )
        .route(
            "/evidence-attachment-uploads/files/{attachment_id}/archive",
            post(archive_file),
        )
        .with_state(state)
}

#[derive(Debug, Deserialize)]
struct UploadSessionQuery {
    token: Option<String>,
}

#[derive(Debug)]
struct UploadSessionAttachmentResponse {
    id: Uuid,
    filename: String,
    content_length: i64,
    upload_status: String,
    downloadable: bool,
    archivable: bool,
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
    let body = render_upload_page(
        &inventory(
            &state.submissions,
            session.submission_id,
            session_context(&session),
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
    let connection = session_context(&session);
    let before = detail(&state.submissions, session.submission_id, connection).await?;

    let payload = match attachment_upload_from_multipart(
        &state.submissions,
        &connection,
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

    let attachment = state
        .submissions
        .create_attachment(&connection, request_id.0, session.submission_id, payload)
        .await?;
    emit_upload_audit(&session, request_id.0, &attachment);

    let mut response = StatusCode::SEE_OTHER.into_response();
    response.headers_mut().insert(
        LOCATION,
        HeaderValue::from_static("/evidence-attachment-uploads"),
    );
    Ok(response)
}

async fn download_file(
    State(state): State<AttachmentUploadSessionState>,
    headers: HeaderMap,
    Extension(request_id): Extension<RequestId>,
    Path(attachment_id): Path<Uuid>,
) -> Result<Response, ApiError> {
    let session = match verify_session(&state, &headers) {
        Ok(session) => session,
        Err(UploadSessionError::Unavailable) => return Ok(unavailable_response()),
        Err(UploadSessionError::Internal) => return Err(ApiError::Internal),
    };
    let grant = state
        .downloads
        .issue(
            session.workspace_id,
            session.issued_by_user_id,
            DownloadGrantIssuer::AgentConnection(session.issued_via.agent_connection_id()),
            session.submission_id,
            attachment_id.into(),
        )
        .await
        .map_err(upload_session_download_error)?;
    AuditEvent::new(
        "evidence_attachment_download_grant.issued",
        AuditOutcome::Success,
        session_audit_actor(&session),
        AuditClientType::Rest,
        "issue_attachment_download_grant_via_upload_session",
    )
    .workspace_id(session.workspace_id.into())
    .request_id(request_id.0)
    .metadata(
        "evidence_submission_id",
        Uuid::from(grant.audit.submission_id),
    )
    .metadata(
        "evidence_attachment_id",
        Uuid::from(grant.audit.attachment_id),
    )
    .object(AuditObject::new(
        "evidence_attachment",
        grant.audit.attachment_id.into(),
    ))
    .emit();

    let mut response = StatusCode::SEE_OTHER.into_response();
    response.headers_mut().insert(
        LOCATION,
        HeaderValue::from_str(grant.url.as_str()).map_err(|_| ApiError::Internal)?,
    );
    Ok(response)
}

async fn archive_file(
    State(state): State<AttachmentUploadSessionState>,
    headers: HeaderMap,
    Path(attachment_id): Path<Uuid>,
) -> Result<Response, ApiError> {
    let session = match verify_session(&state, &headers) {
        Ok(session) => session,
        Err(UploadSessionError::Unavailable) => return Ok(unavailable_response()),
        Err(UploadSessionError::Internal) => return Err(ApiError::Internal),
    };
    let connection = session_context(&session);
    match state
        .submissions
        .archive_attachment(
            &connection,
            session.submission_id,
            EvidenceAttachmentId::from(attachment_id),
        )
        .await?
    {
        ArchiveAttachmentResult::Archived => {
            let mut response = StatusCode::SEE_OTHER.into_response();
            response.headers_mut().insert(
                LOCATION,
                HeaderValue::from_static("/evidence-attachment-uploads"),
            );
            Ok(response)
        }
        ArchiveAttachmentResult::NotFound => Ok(unavailable_response()),
        ArchiveAttachmentResult::NotTerminal => {
            let attachments =
                inventory(&state.submissions, session.submission_id, connection).await?;
            Ok((
                StatusCode::CONFLICT,
                Html(render_upload_page(
                    &attachments,
                    Some("Archive failed: attachment is still processing"),
                )),
            )
                .into_response())
        }
    }
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
            UploadSessionIssuer::AgentConnection(grant.issued_via.agent_connection_id()),
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
    connection: AgentConnectionContext,
) -> Result<Vec<UploadSessionAttachmentResponse>, ApiError> {
    Ok(detail(submissions, submission_id, connection)
        .await?
        .attachments
        .into_iter()
        .map(Into::into)
        .collect())
}

async fn detail(
    submissions: &EvidenceSubmissionService,
    submission_id: EvidenceSubmissionId,
    connection: AgentConnectionContext,
) -> Result<EvidenceSubmissionDetail, ApiError> {
    submissions
        .get(connection, submission_id)
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
    session: &VerifiedUploadSession,
    request_id: Uuid,
    attachment: &EvidenceAttachment,
) {
    AuditEvent::new(
        "evidence_attachment.accepted",
        AuditOutcome::Success,
        session_audit_actor(session),
        AuditClientType::Rest,
        "upload_evidence_attachment_via_upload_session",
    )
    .workspace_id(session.workspace_id.into())
    .request_id(request_id)
    .metadata("evidence_submission_id", Uuid::from(session.submission_id))
    .metadata("evidence_attachment_id", Uuid::from(attachment.id))
    .metadata("lifecycle_status", attachment.upload_status.as_str())
    .object(AuditObject::new(
        "evidence_attachment",
        attachment.id.into(),
    ))
    .emit();
}

fn session_audit_actor(session: &VerifiedUploadSession) -> AuditActor {
    AuditActor::AgentConnection {
        user_id: session.issued_by_user_id.into(),
        agent_connection_id: session.issued_via.agent_connection_id().into(),
    }
}

fn session_context(session: &VerifiedUploadSession) -> AgentConnectionContext {
    AgentConnectionContext {
        user_id: session.issued_by_user_id,
        connection_id: session.issued_via.agent_connection_id(),
        workspace_id: session.workspace_id,
        permissions: crate::domain::WorkspacePermissions::all(),
    }
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
            r#"<table><thead><tr><th>Filename</th><th>Size</th><th>Status</th><th>Actions</th></tr></thead><tbody>{}</tbody></table>"#,
            attachments
                .iter()
                .map(|attachment| format!(
                    "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
                    escape_html(&attachment.filename),
                    format_bytes(attachment.content_length),
                    escape_html(&attachment.upload_status),
                    attachment_actions(attachment),
                ))
                .collect::<Vec<_>>()
                .join("")
        )
    };
    let upload = r#"<section class="panel" aria-labelledby="upload-file">
<h2 id="upload-file">Upload a file</h2>
<form method="post" action="/evidence-attachment-uploads/files" enctype="multipart/form-data">
<label for="file">File</label>
<input id="file" name="file" type="file" required>
<button type="submit">Upload</button>
</form>
</section>"#
        .to_owned();

    format!(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Evidence attachment management</title>
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
  --danger: oklch(66% 0.18 28);
  --danger-hover: oklch(60% 0.18 28);
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
th, td {{ padding: 12px 10px; text-align: left; vertical-align: middle; border-bottom: 1px solid var(--line); }}
th {{ color: var(--muted); font-size: 0.8125rem; font-weight: 620; }}
td {{ font-size: 0.95rem; }}
.empty {{ margin: 12px 0 0; }}
form {{ display: grid; gap: 14px; margin-top: 16px; max-width: 520px; }}
.actions {{ display: flex; flex-wrap: wrap; gap: 8px; align-items: center; justify-content: flex-start; }}
.actions form {{ display: inline; margin: 0; max-width: none; }}
.sr-only {{ position: absolute; width: 1px; height: 1px; padding: 0; margin: -1px; overflow: hidden; clip: rect(0, 0, 0, 0); white-space: nowrap; border: 0; }}
label {{ font-size: 0.8125rem; font-weight: 620; color: var(--muted); }}
input[type="file"] {{
  width: 100%;
  border: 1px solid var(--line);
  background: var(--surface-raised);
  border-radius: 6px;
  padding: 10px;
  color: var(--ink);
}}
input[type="file"]:focus-visible, button:focus-visible, .button:focus-visible {{
  outline: 2px solid var(--signal);
  outline-offset: 2px;
}}
button, .button {{
  display: inline-block;
  justify-self: start;
  border: 0;
  border-radius: 6px;
  background: var(--accent);
  color: var(--canvas);
  padding: 10px 16px;
  font-weight: 700;
  cursor: pointer;
  text-decoration: none;
}}
button:hover, .button:hover {{ background: oklch(72% 0.09 174); }}
.icon-button {{
  display: inline-grid;
  width: 40px;
  height: 40px;
  place-items: center;
  padding: 0;
}}
.icon-button svg {{ width: 18px; height: 18px; stroke: currentColor; }}
.danger-button {{ background: var(--danger); color: var(--ink); }}
.danger-button:hover {{ background: var(--danger-hover); }}
</style>
</head>
<body>
<main>
<h1>Evidence attachment management</h1>
<p>Add evidence files for the scoped submission. This link does not grant access to the rest of Proofplane.</p>
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

async fn attachment_upload_from_multipart(
    service: &EvidenceSubmissionService,
    connection: &AgentConnectionContext,
    evidence_submission_id: EvidenceSubmissionId,
    mut multipart: Multipart,
    digest: AttachmentUploadDigest,
) -> Result<UploadEvidenceAttachmentPayload, ApiError> {
    let field = multipart
        .next_field()
        .await
        .map_err(multipart_error)?
        .ok_or(ApiError::BadRequest(vec![
            "multipart upload requires at least one field".to_owned(),
        ]))?;

    let field_name = field.name().ok_or(ApiError::BadRequest(vec![
        "multipart upload field must be named".to_owned(),
    ]))?;

    if field_name != "file" {
        return Err(ApiError::BadRequest(vec![
            "multipart upload field for file must have correct name".to_owned(),
        ]));
    }

    let filename = field
        .file_name()
        .map(str::to_owned)
        .ok_or_else(|| ApiError::BadRequest(vec!["file filename is required".to_owned()]))?;
    let content_type = field
        .content_type()
        .map(str::to_owned)
        .unwrap_or_else(|| "application/octet-stream".to_owned());
    let filename = validate_attachment_filename(filename)
        .into_result()
        .map_err(domain_errors)?;

    let crc32c = Arc::new(AtomicU32::new(0));
    let chunks = file_chunks(field, Arc::clone(&crc32c));
    let mut uploaded_file = service
        .upload_attachment(
            connection,
            evidence_submission_id,
            filename,
            content_type,
            chunks,
        )
        .await?;

    let actual_crc32c = crc32c.load(Ordering::Relaxed);

    if digest == AttachmentUploadDigest::ComputeOnly
        && multipart
            .next_field()
            .await
            .map_err(multipart_error)?
            .is_some()
    {
        maybe_delete_uploaded_file(service, uploaded_file.object_key).await;
        return Err(ApiError::BadRequest(vec![
            "browser upload requires exactly one file field".to_owned(),
        ]));
    }

    uploaded_file.checksum_crc32c = BASE64_STANDARD.encode(actual_crc32c.to_be_bytes());
    Ok(uploaded_file)
}

fn multipart_error(error: MultipartError) -> ApiError {
    if error.status() == StatusCode::PAYLOAD_TOO_LARGE {
        return ApiError::PayloadTooLarge;
    }

    ApiError::BadRequest(vec![format!(
        "invalid multipart body: {}",
        error.body_text()
    )])
}

fn file_chunks(
    field: Field<'_>,
    crc32c: Arc<AtomicU32>,
) -> impl Stream<Item = Result<Bytes, crate::object_storage::StorageError>> + Send + '_ {
    stream::try_unfold(field, move |mut field| {
        let crc32c = Arc::clone(&crc32c);
        async move {
            match field.chunk().await.map_err(multipart_stream_error)? {
                Some(chunk) => {
                    let current = crc32c.load(Ordering::Relaxed);
                    crc32c.store(crc32c::crc32c_append(current, &chunk), Ordering::Relaxed);
                    Ok(Some((chunk, field)))
                }
                None => Ok(None),
            }
        }
    })
}

fn multipart_stream_error(error: MultipartError) -> crate::object_storage::StorageError {
    crate::object_storage::StorageError::StreamRead {
        payload_too_large: error.status() == StatusCode::PAYLOAD_TOO_LARGE,
        message: if error.status() == StatusCode::PAYLOAD_TOO_LARGE {
            "request payload is too large".to_owned()
        } else {
            format!("invalid multipart body: {}", error.body_text())
        },
    }
}

async fn maybe_delete_uploaded_file(service: &EvidenceSubmissionService, key: String) {
    let _ = service.delete_uploaded_attachment_object(&key).await;
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
            id: Uuid::from(attachment.id),
            filename: attachment.filename,
            content_length: attachment.content_length,
            upload_status: upload_status(attachment.upload_status).to_owned(),
            downloadable: attachment.upload_status == AttachmentUploadStatus::Uploaded,
            archivable: matches!(
                attachment.upload_status,
                AttachmentUploadStatus::Uploaded
                    | AttachmentUploadStatus::ContainsVirus
                    | AttachmentUploadStatus::FailedUpload
            ),
        }
    }
}

fn upload_status(status: AttachmentUploadStatus) -> &'static str {
    status.as_str()
}

fn attachment_actions(attachment: &UploadSessionAttachmentResponse) -> String {
    let mut actions = Vec::new();
    if attachment.downloadable {
        actions.push(format!(
            r#"<a class="button" href="/evidence-attachment-uploads/files/{}/download">Download</a>"#,
            attachment.id
        ));
    }
    if attachment.archivable {
        actions.push(format!(
            r#"<form method="post" action="/evidence-attachment-uploads/files/{}/archive" onsubmit="return confirm('Archive this attachment?');"><button class="icon-button danger-button" type="submit" aria-label="Archive attachment"><svg viewBox="0 0 24 24" aria-hidden="true" fill="none" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M3 6h18"/><path d="M8 6V4h8v2"/><path d="M19 6l-1 14H6L5 6"/><path d="M10 11v5"/><path d="M14 11v5"/></svg><span class="sr-only">Archive</span></button></form>"#,
            attachment.id
        ));
    }

    if actions.is_empty() {
        String::new()
    } else {
        format!(r#"<div class="actions">{}</div>"#, actions.join(""))
    }
}

fn upload_session_download_error(error: DownloadError) -> ApiError {
    match error {
        DownloadError::NotFound => ApiError::NotFound,
        DownloadError::NotReady => ApiError::Conflict {
            code: "attachment_not_ready",
            message: "attachment is not ready for download".to_owned(),
        },
        DownloadError::MetadataMismatch | DownloadError::Internal => ApiError::Internal,
        DownloadError::Repository(repository_error) => {
            tracing::error!(error = %repository_error, "attachment download repository failure");
            ApiError::Internal
        }
    }
}
