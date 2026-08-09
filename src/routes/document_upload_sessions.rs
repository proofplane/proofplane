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
use bytes::Bytes;
use chrono::{DateTime, Utc};
use futures_core::Stream;
use futures_util::stream;
use secrecy::SecretString;
use serde::Deserialize;
use uuid::Uuid;

use crate::{
    application::{
        commands::{
            issue_evidence_document_upload_grant::EvidenceDocumentUploadGrantHandlerError,
            redeem_evidence_document_upload_grant::{
                RedeemEvidenceDocumentUploadGrant, RedeemEvidenceDocumentUploadGrantHandler,
            },
        },
        queries::evidence_catalog::{
            EvidenceCatalogError, ListEvidenceControlMappings, ListEvidenceControlMappingsHandler,
        },
        queries::resolve_evidence_document_upload_grant_authority::{
            ResolveEvidenceDocumentUploadGrantAuthority,
            ResolveEvidenceDocumentUploadGrantAuthorityHandler,
        },
        ExecutionMetadata,
    },
    domain::{
        validate_document_filename, CoverageWindow, Document, DocumentId, DocumentUploadStatus,
        EvidenceId, EvidenceSubmissionDetail, EvidenceSubmissionId,
    },
    observability::audit::{AuditActor, AuditClientType, AuditEvent, AuditObject, AuditOutcome},
    repository::ArchiveDocumentResult,
    routes::{
        error::{domain_errors, ApiError},
        request_context::RequestId,
    },
    services::{
        agent_connections::AgentConnectionContext,
        document_downloads::DownloadGrantIssuer,
        document_downloads::{DocumentDownloadService, DownloadError},
        evidence_submissions::{
            EvidenceSubmissionService, StageEvidenceDocumentInput, StagedEvidenceDocument,
        },
        upload_sessions::{
            UploadSessionError, UploadSessionIssuer, UploadSessionTokenService,
            VerifiedUploadSession,
        },
    },
};

const UPLOAD_SESSION_COOKIE: &str = "proofplane_document_upload_session";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DocumentUploadDigest {
    ComputeOnly,
}

#[derive(Clone)]
pub struct DocumentUploadSessionState {
    pub resolve_grant: ResolveEvidenceDocumentUploadGrantAuthorityHandler,
    pub redeem_grant: RedeemEvidenceDocumentUploadGrantHandler,
    pub downloads: DocumentDownloadService,
    pub sessions: UploadSessionTokenService,
    pub submissions: EvidenceSubmissionService,
    pub list_control_mappings: ListEvidenceControlMappingsHandler,
    pub secure_cookie: bool,
    pub max_document_bytes: usize,
}

pub fn router(state: DocumentUploadSessionState) -> Router {
    Router::new()
        .route("/evidence-document-uploads", get(open_upload_session))
        .route(
            "/evidence-document-uploads/files",
            post(upload_file).layer(DefaultBodyLimit::max(state.max_document_bytes)),
        )
        .route(
            "/evidence-document-uploads/files/{submission_id}/{document_id}/download",
            get(download_file),
        )
        .route(
            "/evidence-document-uploads/files/{submission_id}/{document_id}/archive",
            post(archive_file),
        )
        .with_state(state)
}

#[derive(Debug, Deserialize)]
struct UploadSessionQuery {
    token: Option<String>,
}

#[derive(Debug)]
struct UploadSessionDocumentResponse {
    submission_id: Uuid,
    id: Uuid,
    filename: String,
    content_length: i64,
    upload_status: String,
    downloadable: bool,
    archivable: bool,
}

#[derive(Debug)]
struct UploadSessionControlResponse {
    code: String,
    title: String,
}

#[derive(Debug)]
struct UploadSessionPage {
    coverage: CoverageWindow,
    documents: Vec<UploadSessionDocumentResponse>,
    controls: Vec<UploadSessionControlResponse>,
}

async fn open_upload_session(
    State(state): State<DocumentUploadSessionState>,
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
            &state.list_control_mappings,
            session.evidence_id,
            session.coverage,
            session_context(&session),
        )
        .await?,
        None,
    );
    Ok(Html(body).into_response())
}

async fn upload_file(
    State(state): State<DocumentUploadSessionState>,
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
    let before = inventory(
        &state.submissions,
        &state.list_control_mappings,
        session.evidence_id,
        session.coverage,
        connection,
    )
    .await?;

    let submission_id = EvidenceSubmissionId::from(Uuid::new_v4());
    let payload = match document_upload_from_multipart(
        &state.submissions,
        &connection,
        submission_id,
        multipart,
        state.max_document_bytes,
        DocumentUploadDigest::ComputeOnly,
    )
    .await
    {
        Ok(payload) => payload,
        Err(error @ (ApiError::BadRequest(_) | ApiError::PayloadTooLarge)) => {
            return Ok(upload_error_response(&before, error));
        }
        Err(error) => return Err(error),
    };

    let document = match state
        .submissions
        .create_submission(
            &connection,
            request_id.0,
            submission_id,
            session.evidence_id,
            session.coverage,
            payload,
        )
        .await?
    {
        Some(document) => document,
        None => return Ok(unavailable_response()),
    };
    emit_upload_audit(&session, request_id.0, &document);

    let mut response = StatusCode::SEE_OTHER.into_response();
    response.headers_mut().insert(
        LOCATION,
        HeaderValue::from_static("/evidence-document-uploads"),
    );
    Ok(response)
}

async fn download_file(
    State(state): State<DocumentUploadSessionState>,
    headers: HeaderMap,
    Extension(request_id): Extension<RequestId>,
    Path((submission_id, document_id)): Path<(Uuid, Uuid)>,
) -> Result<Response, ApiError> {
    let session = match verify_session(&state, &headers) {
        Ok(session) => session,
        Err(UploadSessionError::Unavailable) => return Ok(unavailable_response()),
        Err(UploadSessionError::Internal) => return Err(ApiError::Internal),
    };
    if !session_owns_document(
        &state.submissions,
        &session,
        EvidenceSubmissionId::from(submission_id),
        DocumentId::from(document_id),
    )
    .await?
    {
        return Ok(unavailable_response());
    }
    let grant = state
        .downloads
        .issue(
            session.workspace_id,
            session.issued_by_user_id,
            DownloadGrantIssuer::AgentConnection(session.issued_via.agent_connection_id()),
            EvidenceSubmissionId::from(submission_id),
            DocumentId::from(document_id),
        )
        .await
        .map_err(upload_session_download_error)?;
    AuditEvent::new(
        "evidence_document_download_grant.issued",
        AuditOutcome::Success,
        session_audit_actor(&session),
        AuditClientType::Rest,
        "issue_document_download_grant_via_upload_session",
    )
    .workspace_id(session.workspace_id.into())
    .request_id(request_id.0)
    .metadata(
        "evidence_submission_id",
        Uuid::from(grant.audit.submission_id),
    )
    .metadata("evidence_document_id", Uuid::from(grant.audit.document_id))
    .object(AuditObject::new(
        "evidence_document",
        grant.audit.document_id.into(),
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
    State(state): State<DocumentUploadSessionState>,
    headers: HeaderMap,
    Path((submission_id, document_id)): Path<(Uuid, Uuid)>,
) -> Result<Response, ApiError> {
    let session = match verify_session(&state, &headers) {
        Ok(session) => session,
        Err(UploadSessionError::Unavailable) => return Ok(unavailable_response()),
        Err(UploadSessionError::Internal) => return Err(ApiError::Internal),
    };
    let connection = session_context(&session);
    if !session_owns_document(
        &state.submissions,
        &session,
        EvidenceSubmissionId::from(submission_id),
        DocumentId::from(document_id),
    )
    .await?
    {
        return Ok(unavailable_response());
    }
    match state
        .submissions
        .archive_document(
            &connection,
            EvidenceSubmissionId::from(submission_id),
            DocumentId::from(document_id),
        )
        .await?
    {
        ArchiveDocumentResult::Archived => {
            let mut response = StatusCode::SEE_OTHER.into_response();
            response.headers_mut().insert(
                LOCATION,
                HeaderValue::from_static("/evidence-document-uploads"),
            );
            Ok(response)
        }
        ArchiveDocumentResult::NotFound => Ok(unavailable_response()),
        ArchiveDocumentResult::NotTerminal => {
            let page = inventory(
                &state.submissions,
                &state.list_control_mappings,
                session.evidence_id,
                session.coverage,
                connection,
            )
            .await?;
            Ok((
                StatusCode::CONFLICT,
                Html(render_upload_page(
                    &page,
                    Some("Archive failed: this document is not ready to archive"),
                )),
            )
                .into_response())
        }
    }
}

async fn session_owns_document(
    submissions: &EvidenceSubmissionService,
    session: &VerifiedUploadSession,
    submission_id: EvidenceSubmissionId,
    document_id: DocumentId,
) -> Result<bool, ApiError> {
    let detail = submissions
        .get(session_context(session), submission_id)
        .await?;
    Ok(detail.is_some_and(|detail| {
        detail.submission.evidence_id == session.evidence_id
            && detail.submission.valid_from == session.coverage.valid_from
            && detail.submission.valid_until == session.coverage.valid_until
            && detail.document.id() == document_id
    }))
}

async fn redeem_grant(
    state: DocumentUploadSessionState,
    token: String,
) -> Result<Response, ApiError> {
    if token.is_empty() {
        return Ok(unavailable_response());
    }

    let authority = match state
        .resolve_grant
        .handle(
            ResolveEvidenceDocumentUploadGrantAuthority {
                credential: SecretString::from(token),
            },
            ExecutionMetadata::background(),
        )
        .await
    {
        Ok(authority) => authority,
        Err(_) => return Ok(unavailable_response()),
    };
    let grant = match state
        .redeem_grant
        .handle(
            RedeemEvidenceDocumentUploadGrant { authority },
            ExecutionMetadata::background(),
        )
        .await
    {
        Ok(grant) => grant,
        Err(EvidenceDocumentUploadGrantHandlerError::Unavailable) => {
            return Ok(unavailable_response());
        }
        Err(
            EvidenceDocumentUploadGrantHandlerError::Internal
            | EvidenceDocumentUploadGrantHandlerError::Repository(_),
        ) => {
            return Err(ApiError::Internal);
        }
    };
    let session = state
        .sessions
        .issue_until(
            grant.workspace_id,
            grant.evidence_id,
            grant.coverage,
            grant.issued_by_user_id,
            UploadSessionIssuer::AgentConnection(grant.issued_via.agent_connection_id()),
            grant.expires_at,
        )
        .map_err(upload_session_error)?;
    let mut response = StatusCode::SEE_OTHER.into_response();
    response.headers_mut().insert(
        LOCATION,
        HeaderValue::from_static("/evidence-document-uploads"),
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
    controls: &ListEvidenceControlMappingsHandler,
    evidence_id: EvidenceId,
    coverage: CoverageWindow,
    connection: AgentConnectionContext,
) -> Result<UploadSessionPage, ApiError> {
    let details = submissions
        .list_for_coverage(connection, evidence_id, coverage)
        .await?;
    let mappings = controls
        .handle(ListEvidenceControlMappings {
            connection,
            evidence_id,
        })
        .await?
        .ok_or_else(unavailable)?;

    Ok(UploadSessionPage {
        coverage,
        documents: details
            .into_iter()
            .map(upload_session_document_from_detail)
            .collect(),
        controls: mappings
            .into_iter()
            .map(|mapping| UploadSessionControlResponse {
                code: mapping.control.code,
                title: mapping.control.title,
            })
            .collect(),
    })
}

impl From<EvidenceCatalogError> for ApiError {
    fn from(error: EvidenceCatalogError) -> Self {
        match error {
            EvidenceCatalogError::Unavailable => Self::NotFound,
            EvidenceCatalogError::Repository(error) => error.into(),
        }
    }
}

fn upload_session_document_from_detail(
    detail: EvidenceSubmissionDetail,
) -> UploadSessionDocumentResponse {
    let submission_id = Uuid::from(detail.submission.id);
    let document = detail.document;
    UploadSessionDocumentResponse {
        submission_id,
        id: Uuid::from(document.id()),
        filename: document.filename,
        content_length: document.content_length,
        upload_status: upload_status(document.upload_status).to_owned(),
        downloadable: document.upload_status == DocumentUploadStatus::Uploaded,
        archivable: matches!(
            document.upload_status,
            DocumentUploadStatus::Uploaded
                | DocumentUploadStatus::ContainsVirus
                | DocumentUploadStatus::FailedUpload
        ),
    }
}

fn verify_session(
    state: &DocumentUploadSessionState,
    headers: &HeaderMap,
) -> Result<VerifiedUploadSession, UploadSessionError> {
    let token = upload_session_cookie(headers).ok_or(UploadSessionError::Unavailable)?;
    state.sessions.verify(token)
}

fn emit_upload_audit(session: &VerifiedUploadSession, request_id: Uuid, document: &Document) {
    AuditEvent::new(
        "evidence_document.accepted",
        AuditOutcome::Success,
        session_audit_actor(session),
        AuditClientType::Rest,
        "upload_evidence_document_via_upload_session",
    )
    .workspace_id(session.workspace_id.into())
    .request_id(request_id)
    .metadata("evidence_submission_id", document.owner().owner_uuid())
    .metadata("evidence_document_id", Uuid::from(document.id()))
    .metadata("lifecycle_status", document.upload_status.as_str())
    .object(AuditObject::new("evidence_document", document.id().into()))
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

fn render_upload_page(page: &UploadSessionPage, message: Option<&str>) -> String {
    let message = message
        .map(|message| {
            format!(
                r#"<section class="notice" role="alert"><strong>{}</strong><p>Review the message above, then try again.</p></section>"#,
                escape_html(message)
            )
        })
        .unwrap_or_default();
    let rows = if page.documents.is_empty() {
        r#"<div class="empty"><svg viewBox="0 0 24 24" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="1.7"><path d="M7 7.5V6a5 5 0 0 1 10 0v9a7 7 0 0 1-14 0V7a3 3 0 0 1 6 0v8a1 1 0 0 1-2 0V8.5"/></svg><div><strong>No evidence files yet</strong><p>Add the evidence file for this submission.</p></div></div>"#.to_owned()
    } else {
        format!(
            r#"<table><thead><tr><th>Filename</th><th>Size</th><th>Status</th><th>Actions</th></tr></thead><tbody>{}</tbody></table>"#,
            page.documents
                .iter()
                .map(|document| format!(
                    "<tr><td class=\"filename\" data-label=\"File\">{}</td><td data-label=\"Size\">{}</td><td data-label=\"Status\"><span class=\"status\">{}</span></td><td data-label=\"Actions\">{}</td></tr>",
                    escape_html(&document.filename),
                    format_bytes(document.content_length),
                    escape_html(&document.upload_status),
                    document_actions(document),
                ))
                .collect::<Vec<_>>()
                .join("")
        )
    };
    let controls = if page.controls.is_empty() {
        r#"<p class="control-empty">No controls are mapped to this evidence.</p>"#.to_owned()
    } else {
        format!(
            r#"<ul class="control-list">{}</ul>"#,
            page.controls
                .iter()
                .map(|control| format!(
                    r#"<li><strong>{}</strong><span>{}</span></li>"#,
                    escape_html(&control.code),
                    escape_html(&control.title),
                ))
                .collect::<Vec<_>>()
                .join("")
        )
    };
    let upload = r#"<aside class="upload-panel" aria-labelledby="upload-file">
<h2 id="upload-file">Add evidence</h2>
<p>Add one file at a time.</p>
<form class="upload-form" method="post" action="/evidence-document-uploads/files" enctype="multipart/form-data">
<label for="file">Choose file</label>
<input id="file" name="file" type="file" required>
<button type="submit">Upload evidence</button>
</form>
</aside>"#
        .to_owned();
    let coverage_window = format!(
        r#"<div class="coverage-window"><span class="coverage-label">Coverage window</span><p>Valid from <strong>{}</strong> until <strong>{}</strong></p></div>"#,
        format_date(page.coverage.valid_from),
        format_date(page.coverage.valid_until),
    );

    format!(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Evidence document management</title>
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
  background: transparent;
  color: var(--muted);
  transition: background-color 160ms ease-out, color 160ms ease-out;
}}
.icon-button svg {{ width: 18px; height: 18px; stroke: currentColor; }}
.icon-button:hover {{ background: var(--surface-raised); color: var(--ink); }}
.danger-button {{ color: var(--danger); }}
.danger-button:hover {{ background: oklch(25% 0.035 28); color: var(--danger); }}
.site-header {{ border-bottom: 1px solid var(--line); padding: 14px max(16px, calc((100vw - 1080px) / 2)); }}
.wordmark {{ color: var(--ink); font-size: 0.78rem; font-weight: 750; letter-spacing: 0.08em; }}
.wordmark span {{ color: var(--muted); font-weight: 550; }}
main {{ width: min(1080px, calc(100% - 32px)); padding: 52px 0 72px; }}
.page-header {{ display: grid; grid-template-columns: minmax(0, 1fr) auto; gap: 32px; align-items: end; }}
.eyebrow {{ margin: 0 0 10px; color: var(--accent); font-size: 0.78rem; font-weight: 700; }}
h1 {{ max-width: 18ch; font-size: clamp(1.8rem, 4vw, 2.5rem); line-height: 1.05; }}
.coverage-window {{ margin: 16px 0 0; display: inline-flex; flex-direction: column; gap: 3px; border: 1px solid var(--line); border-radius: 6px; padding: 10px 14px; }}
.coverage-label {{ color: var(--accent); font-size: 0.72rem; font-weight: 700; letter-spacing: 0.04em; text-transform: uppercase; }}
.coverage-window p {{ margin: 0; color: var(--ink); font-size: 0.9rem; }}
.coverage-window strong {{ color: var(--ink); font-weight: 700; }}
h2 {{ margin: 0; font-size: 1.1rem; }}
.control-context {{ width: min(360px, 38vw); margin: 0; padding: 14px 16px; border: 1px solid var(--line); border-radius: 6px; }}
.control-context > p:first-child {{ margin: 0 0 10px; color: var(--muted); font-size: 0.74rem; font-weight: 700; }}
.control-list {{ display: grid; gap: 9px; margin: 0; padding: 0; list-style: none; }}
.control-list li {{ display: grid; grid-template-columns: auto minmax(0, 1fr); gap: 9px; align-items: baseline; }}
.control-list strong {{ color: var(--accent); font-size: 0.8rem; }}
.control-list span {{ color: var(--ink); font-size: 0.88rem; line-height: 1.35; }}
.control-empty {{ margin: 0; font-size: 0.86rem; }}
.workspace {{ display: grid; grid-template-columns: minmax(0, 1.65fr) minmax(280px, 0.85fr); gap: 28px; margin-top: 36px; align-items: start; }}
.panel {{ margin: 0; padding: 18px 0 0; border-width: 1px 0 0; border-radius: 0; background: transparent; }}
.panel-heading {{ display: flex; justify-content: space-between; gap: 16px; align-items: baseline; margin-bottom: 14px; }}
.count {{ color: var(--muted); font-size: 0.78rem; font-weight: 650; }}
.upload-panel {{ border: 1px solid var(--line); background: var(--surface); border-radius: 8px; padding: 22px; }}
.upload-panel p {{ margin: 8px 0 20px; font-size: 0.9rem; }}
.upload-form {{ display: grid; gap: 14px; margin: 0; }}
.upload-form button {{ justify-self: start; }}
input[type="file"]::file-selector-button {{ margin-right: 10px; border: 0; border-radius: 4px; background: var(--ink); color: var(--canvas); padding: 7px 10px; font: inherit; font-weight: 700; cursor: pointer; }}
.filename {{ color: var(--ink); font-weight: 650; overflow-wrap: anywhere; }}
.status {{ display: inline-block; border-radius: 4px; background: var(--surface-raised); color: var(--muted); padding: 5px 8px; font-size: 0.76rem; font-weight: 650; }}
.empty {{ min-height: 210px; display: grid; place-content: center; justify-items: center; border: 1px dashed var(--line); border-radius: 8px; padding: 28px; text-align: center; }}
.empty svg {{ width: 28px; height: 28px; margin-bottom: 14px; color: var(--accent); }}
.empty strong {{ display: block; margin-bottom: 5px; }}
.empty p {{ margin: 0; font-size: 0.9rem; }}
@media (max-width: 760px) {{
  main {{ padding-top: 36px; }}
  .page-header, .workspace {{ grid-template-columns: 1fr; }}
  .control-context {{ width: 100%; }}
  .upload-panel {{ grid-row: 1; }}
  thead {{ position: absolute; width: 1px; height: 1px; overflow: hidden; clip: rect(0, 0, 0, 0); }}
  table, tbody, tr, td {{ display: block; }}
  tr {{ padding: 12px 0; border-bottom: 1px solid var(--line); }}
  td {{ display: grid; grid-template-columns: 108px minmax(0, 1fr); gap: 12px; padding: 6px 0; border: 0; }}
  td::before {{ content: attr(data-label); color: var(--muted); font-size: 0.76rem; font-weight: 650; }}
}}
@media (prefers-reduced-motion: reduce) {{ *, *::before, *::after {{ transition: none !important; }} }}
</style>
</head>
<body>
<header class="site-header"><div class="wordmark">PROOFPLANE <span>/ EVIDENCE INTAKE</span></div></header>
<main>
<header class="page-header"><div><p class="eyebrow">SCOPED EVIDENCE SUBMISSION</p><h1>Evidence documents</h1><p>Add the files that cover this period, then return to your MCP client to continue. Each file becomes one submission for the window below.</p>{coverage_window}</div><aside class="control-context" aria-label="Evidence target"><p>PROVIDING EVIDENCE FOR</p>{controls}</aside></header>
{message}
<div class="workspace">
<section class="panel" aria-labelledby="current-documents">
<div class="panel-heading"><h2 id="current-documents">Uploaded files</h2><span class="count">{count} total</span></div>
{rows}
</section>
{upload}
</div>
</main>
</body>
</html>"#,
        count = page.documents.len(),
    )
}

async fn document_upload_from_multipart(
    service: &EvidenceSubmissionService,
    connection: &AgentConnectionContext,
    evidence_submission_id: EvidenceSubmissionId,
    mut multipart: Multipart,
    max_document_bytes: usize,
    digest: DocumentUploadDigest,
) -> Result<StagedEvidenceDocument, ApiError> {
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
    let filename = validate_document_filename(filename)
        .into_result()
        .map_err(domain_errors)?;

    let chunks = file_chunks(field);
    let uploaded_file = service
        .stage_document(
            connection,
            StageEvidenceDocumentInput {
                evidence_submission_id,
                filename,
                content_type,
                max_bytes: max_document_bytes,
                chunks,
            },
        )
        .await?;

    if digest == DocumentUploadDigest::ComputeOnly
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
) -> impl Stream<Item = Result<Bytes, crate::object_storage::StorageError>> + Send + '_ {
    stream::try_unfold(field, |mut field| async move {
        match field.chunk().await.map_err(multipart_stream_error)? {
            Some(chunk) => Ok(Some((chunk, field))),
            None => Ok(None),
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
    let _ = service.delete_uploaded_document_object(&key).await;
}

fn upload_error_response(page: &UploadSessionPage, error: ApiError) -> Response {
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
    (
        status,
        Html(render_upload_page(
            page,
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
:root { color-scheme: dark; --canvas: oklch(17% 0.012 170); --line: oklch(39% 0.018 170); --ink: oklch(94% 0.01 150); --muted: oklch(76% 0.015 155); --accent: oklch(78% 0.09 174); }
* { box-sizing: border-box; }
body { margin: 0; min-height: 100vh; background: var(--canvas); color: var(--ink); font-family: Inter, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif; }
header { border-bottom: 1px solid var(--line); padding: 14px max(16px, calc((100vw - 1080px) / 2)); color: var(--ink); font-size: .78rem; font-weight: 750; letter-spacing: .08em; }
header span { color: var(--muted); font-weight: 550; }
main { width: min(690px, calc(100% - 32px)); min-height: calc(100vh - 49px); margin: 0 auto; display: grid; align-content: center; padding: 40px 0; }
.eyebrow { margin: 0 0 10px; color: var(--accent); font-size: .78rem; font-weight: 700; }
h1 { max-width: 20ch; margin: 0 0 12px; font-size: clamp(1.8rem, 4vw, 2.5rem); line-height: 1.05; }
p { max-width: 58ch; margin: 0; color: var(--muted); line-height: 1.55; }
</style>
</head>
<body><header>PROOFPLANE <span>/ EVIDENCE INTAKE</span></header><main><p class="eyebrow">LINK UNAVAILABLE</p><h1>This upload link is no longer available</h1><p>Return to your MCP client and request a new evidence upload link.</p></main></body>
</html>"#
        .to_owned()
}

fn format_date(value: DateTime<Utc>) -> String {
    value.format("%Y-%m-%d").to_string()
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
        "{UPLOAD_SESSION_COOKIE}={token}; HttpOnly; SameSite=Lax; Path=/evidence-document-uploads; Max-Age={}",
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

fn upload_status(status: DocumentUploadStatus) -> &'static str {
    match status {
        DocumentUploadStatus::PendingUpload => "Uploading",
        DocumentUploadStatus::Finalizing => "Scanning",
        DocumentUploadStatus::Uploaded => "Uploaded",
        DocumentUploadStatus::ContainsVirus => "Upload failed",
        DocumentUploadStatus::FailedUpload => "Upload failed",
    }
}

fn document_actions(document: &UploadSessionDocumentResponse) -> String {
    let mut actions = Vec::new();
    if document.downloadable {
        actions.push(format!(
            r#"<a class="button icon-button" href="/evidence-document-uploads/files/{}/{}/download" aria-label="Download document"><svg viewBox="0 0 24 24" aria-hidden="true" fill="none" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 3v12"/><path d="m7 10 5 5 5-5"/><path d="M5 21h14"/></svg><span class="sr-only">Download</span></a>"#,
            document.submission_id, document.id
        ));
    }
    if document.archivable {
        actions.push(format!(
            r#"<form method="post" action="/evidence-document-uploads/files/{}/{}/archive" onsubmit="return confirm('Archive this document?');"><button class="icon-button danger-button" type="submit" aria-label="Archive document"><svg viewBox="0 0 24 24" aria-hidden="true" fill="none" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M3 6h18"/><path d="M8 6V4h8v2"/><path d="M19 6l-1 14H6L5 6"/><path d="M10 11v5"/><path d="M14 11v5"/></svg><span class="sr-only">Archive</span></button></form>"#,
            document.submission_id, document.id
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
            code: "document_not_ready",
            message: "document is not ready for download".to_owned(),
        },
        DownloadError::MetadataMismatch | DownloadError::Internal => ApiError::Internal,
        DownloadError::Repository(repository_error) => {
            tracing::error!(error = %repository_error, "document download repository failure");
            ApiError::Internal
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        render_upload_page, upload_status, UploadSessionControlResponse,
        UploadSessionDocumentResponse, UploadSessionPage,
    };
    use crate::domain::CoverageWindow;
    use crate::domain::DocumentUploadStatus;

    #[test]
    fn upload_page_keeps_scope_actions_and_mobile_labels_visible() {
        let html = render_upload_page(
            &UploadSessionPage {
                coverage: CoverageWindow::new(
                    "2026-04-01T00:00:00+00:00".parse().unwrap(),
                    "2026-06-30T00:00:00+00:00".parse().unwrap(),
                )
                .unwrap(),
                documents: vec![UploadSessionDocumentResponse {
                    submission_id: uuid::Uuid::nil(),
                    id: uuid::Uuid::nil(),
                    filename: "access-review.pdf".to_owned(),
                    content_length: 2048,
                    upload_status: "uploaded".to_owned(),
                    downloadable: true,
                    archivable: true,
                }],
                controls: vec![UploadSessionControlResponse {
                    code: "CC6.1".to_owned(),
                    title: "Logical access controls".to_owned(),
                }],
            },
            None,
        );

        for expected in [
            "PROVIDING EVIDENCE FOR",
            "CC6.1",
            "Logical access controls",
            "Upload evidence",
            "Coverage window",
            "Valid from <strong>2026-04-01</strong> until <strong>2026-06-30</strong>",
            "data-label=\"Status\"",
            "aria-label=\"Download document\"",
            "Archive document",
        ] {
            assert!(html.contains(expected), "missing {expected}");
        }
        assert!(!html.contains("Limited access."));
    }

    #[test]
    fn upload_page_uses_user_facing_status_labels() {
        assert_eq!(
            upload_status(DocumentUploadStatus::PendingUpload),
            "Uploading"
        );
        assert_eq!(upload_status(DocumentUploadStatus::Finalizing), "Scanning");
        assert_eq!(upload_status(DocumentUploadStatus::Uploaded), "Uploaded");
        assert_eq!(
            upload_status(DocumentUploadStatus::ContainsVirus),
            "Upload failed"
        );
        assert_eq!(
            upload_status(DocumentUploadStatus::FailedUpload),
            "Upload failed"
        );
    }
}
