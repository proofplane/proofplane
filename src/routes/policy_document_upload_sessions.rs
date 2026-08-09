use std::sync::{
    atomic::{AtomicU32, Ordering},
    Arc,
};

use axum::{
    body::Body,
    extract::{
        multipart::{Field, MultipartError},
        rejection::QueryRejection,
        DefaultBodyLimit, Multipart, Path, Query, State,
    },
    http::{
        header::{
            CACHE_CONTROL, CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_TYPE, COOKIE, LOCATION,
            SET_COOKIE,
        },
        HeaderMap, HeaderName, HeaderValue, StatusCode,
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
use secrecy::SecretString;
use serde::Deserialize;
use uuid::Uuid;

use crate::{
    application::{
        commands::{
            issue_policy_document_upload_grant::PolicyDocumentUploadGrantHandlerError,
            redeem_policy_document_upload_grant::{
                RedeemPolicyDocumentUploadGrant, RedeemPolicyDocumentUploadGrantHandler,
            },
        },
        queries::policy_catalog::{GetPolicy, GetPolicyHandler},
        queries::resolve_policy_document_upload_grant_authority::{
            ResolvePolicyDocumentUploadGrantAuthority,
            ResolvePolicyDocumentUploadGrantAuthorityHandler,
        },
        ExecutionMetadata,
    },
    authentication::AgentConnectionContext,
    domain::{
        validate_document_filename, DocumentId, DocumentUploadStatus, PolicyId,
        WorkspacePermissions,
    },
    observability::audit::{AuditActor, AuditClientType, AuditEvent, AuditObject, AuditOutcome},
    projections::PolicyDetail,
    repository::{ArchiveDocumentResult, CreatePolicyDocumentResult},
    routes::{
        document_downloads::content_disposition,
        error::{domain_errors, ApiError},
        request_context::RequestId,
    },
    services::{
        document_downloads::{DocumentDownloadService, DownloadError},
        policy_documents::{PolicyDocumentService, UploadPolicyDocumentPayload},
        policy_upload_sessions::{
            PolicyUploadSessionError, PolicyUploadSessionTokenService, VerifiedPolicyUploadSession,
        },
    },
};

const POLICY_UPLOAD_SESSION_COOKIE: &str = "proofplane_policy_document_upload_session";
const POLICY_UPLOAD_PATH: &str = "/policy-document-uploads";
const REFERRER_POLICY: HeaderName = HeaderName::from_static("referrer-policy");

#[derive(Clone)]
pub struct PolicyDocumentUploadSessionState {
    pub resolve_grant: ResolvePolicyDocumentUploadGrantAuthorityHandler,
    pub redeem_grant: RedeemPolicyDocumentUploadGrantHandler,
    pub downloads: DocumentDownloadService,
    pub sessions: PolicyUploadSessionTokenService,
    pub get_policy: GetPolicyHandler,
    pub documents: PolicyDocumentService,
    pub secure_cookie: bool,
    pub max_document_bytes: usize,
}

pub fn router(state: PolicyDocumentUploadSessionState) -> Router {
    Router::new()
        .route(POLICY_UPLOAD_PATH, get(open_policy_upload_session))
        .route(
            "/policy-document-uploads/files",
            post(upload_file).layer(DefaultBodyLimit::max(state.max_document_bytes)),
        )
        .route(
            "/policy-document-uploads/files/{document_id}/download",
            get(download_file),
        )
        .route(
            "/policy-document-uploads/files/{document_id}/archive",
            post(archive_file),
        )
        .with_state(state)
}

#[derive(Debug, Deserialize)]
struct PolicyUploadSessionQuery {
    token: Option<String>,
}

#[derive(Debug)]
struct PolicyUploadPage {
    policy_name: String,
    document: Option<PolicyUploadDocumentResponse>,
}

#[derive(Debug)]
struct PolicyUploadDocumentResponse {
    id: DocumentId,
    filename: String,
    content_length: i64,
    upload_status: DocumentUploadStatus,
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
    let Some(page) = inventory(&state, &session).await? else {
        return Ok(unavailable_response());
    };

    Ok(Html(render_upload_page(&page, None)).into_response())
}

async fn upload_file(
    State(state): State<PolicyDocumentUploadSessionState>,
    Extension(request_id): Extension<RequestId>,
    headers: HeaderMap,
    multipart: Multipart,
) -> Result<Response, ApiError> {
    let session = match verify_session(&state, &headers) {
        Ok(session) => session,
        Err(PolicyUploadSessionError::Unavailable) => return Ok(unavailable_response()),
        Err(PolicyUploadSessionError::Internal) => return Err(ApiError::Internal),
    };
    let Some(before) = inventory(&state, &session).await? else {
        return Ok(unavailable_response());
    };
    if before.document.is_some() {
        return Ok(conflict_response(
            &before,
            "Upload failed: this policy already has a current document",
        ));
    }

    let connection = session_context(&session);
    let payload = match policy_document_upload_from_multipart(
        &state.documents,
        &connection,
        session.policy_id,
        multipart,
    )
    .await
    {
        Ok(payload) => payload,
        Err(error @ (ApiError::BadRequest(_) | ApiError::PayloadTooLarge)) => {
            return Ok(upload_error_response(&before, error));
        }
        Err(error) => return Err(error),
    };

    match state
        .documents
        .create(&connection, request_id.0, session.policy_id, payload)
        .await?
    {
        CreatePolicyDocumentResult::Created(_) => Ok(redirect_to_management()),
        CreatePolicyDocumentResult::PolicyNotFound => Ok(unavailable_response()),
        CreatePolicyDocumentResult::DocumentExists => {
            let Some(page) = inventory(&state, &session).await? else {
                return Ok(unavailable_response());
            };
            Ok(conflict_response(
                &page,
                "Upload failed: this policy already has a current document",
            ))
        }
    }
}

async fn archive_file(
    State(state): State<PolicyDocumentUploadSessionState>,
    Extension(request_id): Extension<RequestId>,
    headers: HeaderMap,
    Path(document_id): Path<Uuid>,
) -> Result<Response, ApiError> {
    let session = match verify_session(&state, &headers) {
        Ok(session) => session,
        Err(PolicyUploadSessionError::Unavailable) => return Ok(unavailable_response()),
        Err(PolicyUploadSessionError::Internal) => return Err(ApiError::Internal),
    };
    if inventory(&state, &session).await?.is_none() {
        return Ok(unavailable_response());
    }
    let connection = session_context(&session);
    match state
        .documents
        .archive(
            &connection,
            request_id.0,
            session.policy_id,
            document_id.into(),
        )
        .await?
    {
        ArchiveDocumentResult::Archived => Ok(redirect_to_management()),
        ArchiveDocumentResult::NotFound => Ok(unavailable_response()),
        ArchiveDocumentResult::NotTerminal => {
            let Some(page) = inventory(&state, &session).await? else {
                return Ok(unavailable_response());
            };
            Ok(conflict_response(
                &page,
                "Archive failed: this document is not ready to archive",
            ))
        }
    }
}

async fn download_file(
    State(state): State<PolicyDocumentUploadSessionState>,
    Extension(request_id): Extension<RequestId>,
    headers: HeaderMap,
    Path(document_id): Path<Uuid>,
) -> Result<Response, ApiError> {
    let session = match verify_session(&state, &headers) {
        Ok(session) => session,
        Err(PolicyUploadSessionError::Unavailable) => return Ok(unavailable_response()),
        Err(PolicyUploadSessionError::Internal) => return Err(ApiError::Internal),
    };
    let Some(page) = inventory(&state, &session).await? else {
        return Ok(unavailable_response());
    };
    let document_id = DocumentId::from(document_id);
    if !matches!(
        page.document.as_ref(),
        Some(document)
            if document.id == document_id
                && document.upload_status == DocumentUploadStatus::Uploaded
    ) {
        return Ok(unavailable_response());
    }

    let downloaded = match state
        .downloads
        .download_policy_for_workspace(session.workspace_id, session.policy_id, document_id)
        .await
    {
        Ok(downloaded) => downloaded,
        Err(DownloadError::NotFound | DownloadError::NotReady) => {
            return Ok(unavailable_response());
        }
        Err(error @ (DownloadError::MetadataMismatch | DownloadError::Internal)) => {
            tracing::error!(%error, "policy document download failed");
            return Err(ApiError::Internal);
        }
        Err(DownloadError::Repository(error)) => {
            tracing::error!(%error, "policy document download repository failure");
            return Err(ApiError::Internal);
        }
    };

    AuditEvent::new(
        "policy_document.downloaded",
        AuditOutcome::Success,
        session_audit_actor(&session),
        AuditClientType::Rest,
        "download_policy_document_via_upload_session",
    )
    .workspace_id(session.workspace_id.into())
    .request_id(request_id.0)
    .metadata("policy_id", Uuid::from(session.policy_id))
    .metadata("policy_document_id", Uuid::from(document_id))
    .object(AuditObject::new("policy_document", document_id.into()))
    .emit();

    let disposition = content_disposition(&downloaded.document.filename);
    let mut response = Body::from_stream(downloaded.object.chunks).into_response();
    *response.status_mut() = StatusCode::OK;
    let response_headers = response.headers_mut();
    response_headers.insert(
        CONTENT_TYPE,
        HeaderValue::from_str(&downloaded.document.content_type).map_err(|_| ApiError::Internal)?,
    );
    response_headers.insert(
        CONTENT_LENGTH,
        HeaderValue::from_str(&downloaded.document.content_length.to_string())
            .map_err(|_| ApiError::Internal)?,
    );
    response_headers.insert(
        CONTENT_DISPOSITION,
        HeaderValue::from_str(&disposition).map_err(|_| ApiError::Internal)?,
    );
    response_headers.insert(CACHE_CONTROL, HeaderValue::from_static("private, no-store"));
    response_headers.insert(REFERRER_POLICY, HeaderValue::from_static("no-referrer"));
    Ok(response)
}

async fn redeem_grant(
    state: PolicyDocumentUploadSessionState,
    request_id: RequestId,
    token: String,
) -> Result<Response, ApiError> {
    if token.is_empty() {
        return Ok(unavailable_response());
    }
    let authority = match state
        .resolve_grant
        .handle(
            ResolvePolicyDocumentUploadGrantAuthority {
                credential: SecretString::from(token),
            },
            ExecutionMetadata::for_request(request_id.0),
        )
        .await
    {
        Ok(authority) => authority,
        Err(_) => return Ok(unavailable_response()),
    };
    let grant = match state
        .redeem_grant
        .handle(
            RedeemPolicyDocumentUploadGrant { authority },
            ExecutionMetadata::for_request(request_id.0),
        )
        .await
    {
        Ok(grant) => grant,
        Err(PolicyDocumentUploadGrantHandlerError::Unavailable) => {
            return Ok(unavailable_response());
        }
        Err(
            error @ (PolicyDocumentUploadGrantHandlerError::Internal
            | PolicyDocumentUploadGrantHandlerError::Repository(_)),
        ) => {
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

    let mut response = redirect_to_management();
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
    state: &PolicyDocumentUploadSessionState,
    session: &VerifiedPolicyUploadSession,
) -> Result<Option<PolicyUploadPage>, ApiError> {
    let detail = state
        .get_policy
        .handle(GetPolicy {
            connection: session_context(session),
            policy_id: session.policy_id,
        })
        .await
        .map_err(|error| {
            tracing::error!(%error, "policy upload inventory query failed");
            ApiError::Internal
        })?;
    Ok(detail.map(Into::into))
}

fn verify_session(
    state: &PolicyDocumentUploadSessionState,
    headers: &HeaderMap,
) -> Result<VerifiedPolicyUploadSession, PolicyUploadSessionError> {
    let token =
        policy_upload_session_cookie(headers).ok_or(PolicyUploadSessionError::Unavailable)?;
    state.sessions.verify(token)
}

fn session_context(session: &VerifiedPolicyUploadSession) -> AgentConnectionContext {
    AgentConnectionContext {
        user_id: session.issued_by_user_id,
        connection_id: session.issued_via_agent_connection_id,
        workspace_id: session.workspace_id,
        permissions: WorkspacePermissions::all(),
    }
}

fn session_audit_actor(session: &VerifiedPolicyUploadSession) -> AuditActor {
    AuditActor::AgentConnection {
        user_id: session.issued_by_user_id.into(),
        agent_connection_id: session.issued_via_agent_connection_id.into(),
    }
}

async fn policy_document_upload_from_multipart(
    service: &PolicyDocumentService,
    connection: &AgentConnectionContext,
    policy_id: PolicyId,
    mut multipart: Multipart,
) -> Result<UploadPolicyDocumentPayload, ApiError> {
    let field = multipart
        .next_field()
        .await
        .map_err(multipart_error)?
        .ok_or(ApiError::BadRequest(vec![
            "multipart upload requires at least one field".to_owned(),
        ]))?;
    if field.name() != Some("file") {
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

    let crc32c = Arc::new(AtomicU32::new(0));
    let chunks = file_chunks(field, Arc::clone(&crc32c));
    let mut uploaded = service
        .upload(connection, policy_id, filename, content_type, chunks)
        .await
        .map_err(ApiError::from)?;
    if multipart
        .next_field()
        .await
        .map_err(multipart_error)?
        .is_some()
    {
        let _ = service.delete_staged_object(&uploaded.object_key).await;
        return Err(ApiError::BadRequest(vec![
            "browser upload requires exactly one file field".to_owned(),
        ]));
    }

    uploaded.checksum_crc32c = BASE64_STANDARD.encode(crc32c.load(Ordering::Relaxed).to_be_bytes());
    Ok(uploaded)
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

fn multipart_error(error: MultipartError) -> ApiError {
    if error.status() == StatusCode::PAYLOAD_TOO_LARGE {
        ApiError::PayloadTooLarge
    } else {
        ApiError::BadRequest(vec![format!(
            "invalid multipart body: {}",
            error.body_text()
        )])
    }
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

fn render_upload_page(page: &PolicyUploadPage, message: Option<&str>) -> String {
    let message = message
        .map(|message| {
            format!(
                r#"<section class="notice" role="alert"><strong>{}</strong><p>Review the message above, then try again.</p></section>"#,
                escape_html(message)
            )
        })
        .unwrap_or_default();
    let document = page
        .document
        .as_ref()
        .map(document_row)
        .unwrap_or_else(|| {
            r#"<div class="empty"><svg viewBox="0 0 24 24" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="1.7"><path d="M7 7.5V6a5 5 0 0 1 10 0v9a7 7 0 0 1-14 0V7a3 3 0 0 1 6 0v8a1 1 0 0 1-2 0V8.5"/></svg><div><strong>No policy document yet</strong><p>Add the document for this policy.</p></div></div>"#.to_owned()
        });
    let upload = if page.document.is_none() {
        r#"<aside class="upload-panel" aria-labelledby="upload-file">
<h2 id="upload-file">Add policy document</h2>
<p>Add the current document for this policy.</p>
<form class="upload-form" method="post" action="/policy-document-uploads/files" enctype="multipart/form-data">
<label for="file">Choose file</label>
<input id="file" name="file" type="file" required>
<button type="submit">Upload document</button>
</form>
</aside>"#
            .to_owned()
    } else {
        r#"<aside class="upload-panel" aria-labelledby="replace-document"><h2 id="replace-document">Replace this document</h2><p>Archive the current document before uploading a replacement.</p></aside>"#.to_owned()
    };

    format!(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Policy document management</title>
<style>
:root {{ color-scheme: dark; --canvas: oklch(17% 0.012 170); --surface: oklch(24% 0.014 170); --surface-raised: oklch(30% 0.018 170); --line: oklch(39% 0.018 170); --ink: oklch(94% 0.01 150); --muted: oklch(76% 0.015 155); --accent: oklch(78% 0.09 174); --signal: oklch(78% 0.08 48); --danger: oklch(66% 0.18 28); }}
* {{ box-sizing: border-box; }}
body {{ margin: 0; min-height: 100vh; background: var(--canvas); color: var(--ink); font-family: Inter, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif; }}
.site-header {{ border-bottom: 1px solid var(--line); padding: 14px max(16px, calc((100vw - 1080px) / 2)); }}
.wordmark {{ color: var(--ink); font-size: .78rem; font-weight: 750; letter-spacing: .08em; }}
.wordmark span {{ color: var(--muted); font-weight: 550; }}
main {{ width: min(1080px, calc(100% - 32px)); margin: 0 auto; padding: 52px 0 72px; }}
.page-header {{ display: grid; grid-template-columns: minmax(0, 1fr) minmax(260px, 360px); gap: 32px; align-items: end; }}
.eyebrow {{ margin: 0 0 10px; color: var(--accent); font-size: .78rem; font-weight: 700; }}
h1 {{ max-width: 20ch; margin: 0 0 10px; font-size: clamp(1.8rem, 4vw, 2.5rem); line-height: 1.05; overflow-wrap: anywhere; }}
h2 {{ margin: 0; font-size: 1.1rem; }}
p {{ color: var(--muted); line-height: 1.55; max-width: 68ch; }}
.policy-context {{ margin: 0; padding: 14px 16px; border: 1px solid var(--line); border-radius: 6px; }}
.policy-context p {{ margin: 0 0 8px; color: var(--muted); font-size: .74rem; font-weight: 700; }}
.policy-context strong {{ display: block; overflow-wrap: anywhere; }}
.workspace {{ display: grid; grid-template-columns: minmax(0, 1.65fr) minmax(280px, .85fr); gap: 28px; margin-top: 36px; align-items: start; }}
.panel {{ padding: 18px 0 0; border-top: 1px solid var(--line); }}
.panel-heading {{ display: flex; justify-content: space-between; gap: 16px; align-items: baseline; margin-bottom: 14px; }}
.count {{ color: var(--muted); font-size: .78rem; font-weight: 650; }}
.upload-panel {{ border: 1px solid var(--line); background: var(--surface); border-radius: 8px; padding: 22px; }}
.upload-panel p {{ margin: 8px 0 20px; font-size: .9rem; }}
.upload-form {{ display: grid; gap: 14px; margin: 0; }}
label {{ color: var(--muted); font-size: .8125rem; font-weight: 620; }}
input[type="file"] {{ width: 100%; border: 1px solid var(--line); background: var(--surface-raised); border-radius: 6px; padding: 10px; color: var(--ink); }}
input[type="file"]::file-selector-button {{ margin-right: 10px; border: 0; border-radius: 4px; background: var(--ink); color: var(--canvas); padding: 7px 10px; font: inherit; font-weight: 700; cursor: pointer; }}
input[type="file"]:focus-visible, button:focus-visible, .button:focus-visible {{ outline: 2px solid var(--signal); outline-offset: 2px; }}
button, .button {{ display: inline-block; justify-self: start; border: 0; border-radius: 6px; background: var(--accent); color: var(--canvas); padding: 10px 16px; font-weight: 700; cursor: pointer; text-decoration: none; }}
button:hover, .button:hover {{ background: oklch(72% 0.09 174); }}
table {{ width: 100%; border-collapse: collapse; margin-top: 12px; }}
th, td {{ padding: 12px 10px; text-align: left; vertical-align: middle; border-bottom: 1px solid var(--line); }}
th {{ color: var(--muted); font-size: .8125rem; font-weight: 620; }}
td {{ font-size: .95rem; }}
.filename {{ color: var(--ink); font-weight: 650; overflow-wrap: anywhere; }}
.status {{ display: inline-block; border-radius: 4px; background: var(--surface-raised); color: var(--muted); padding: 5px 8px; font-size: .76rem; font-weight: 650; }}
.actions {{ display: flex; flex-wrap: wrap; gap: 8px; align-items: center; }}
.actions form {{ display: inline; margin: 0; }}
.icon-button {{ display: inline-grid; width: 40px; height: 40px; place-items: center; padding: 0; background: transparent; color: var(--muted); }}
.icon-button svg {{ width: 18px; height: 18px; stroke: currentColor; }}
.icon-button:hover {{ background: var(--surface-raised); color: var(--ink); }}
.danger-button {{ color: var(--danger); }}
.danger-button:hover {{ background: oklch(25% 0.035 28); color: var(--danger); }}
.sr-only {{ position: absolute; width: 1px; height: 1px; padding: 0; margin: -1px; overflow: hidden; clip: rect(0, 0, 0, 0); white-space: nowrap; border: 0; }}
.empty {{ min-height: 210px; display: grid; place-content: center; justify-items: center; border: 1px dashed var(--line); border-radius: 8px; padding: 28px; text-align: center; }}
.empty svg {{ width: 28px; height: 28px; margin-bottom: 14px; color: var(--accent); }}
.empty strong {{ display: block; margin-bottom: 5px; }}
.empty p {{ margin: 0; font-size: .9rem; }}
.notice {{ border: 1px solid color-mix(in oklch, var(--accent) 50%, var(--line)); background: oklch(27% .03 170); border-radius: 8px; padding: 16px; margin-top: 24px; }}
.notice p {{ margin: 6px 0 0; }}
@media (max-width: 760px) {{ main {{ padding-top: 36px; }} .page-header, .workspace {{ grid-template-columns: 1fr; }} .upload-panel {{ grid-row: 1; }} thead {{ position: absolute; width: 1px; height: 1px; overflow: hidden; clip: rect(0, 0, 0, 0); }} table, tbody, tr, td {{ display: block; }} tr {{ padding: 12px 0; border-bottom: 1px solid var(--line); }} td {{ display: grid; grid-template-columns: 108px minmax(0, 1fr); gap: 12px; padding: 6px 0; border: 0; }} td::before {{ content: attr(data-label); color: var(--muted); font-size: .76rem; font-weight: 650; }} }}
@media (prefers-reduced-motion: reduce) {{ *, *::before, *::after {{ transition: none !important; }} }}
</style>
</head>
<body>
<header class="site-header"><div class="wordmark">PROOFPLANE <span>/ POLICY DOCUMENTS</span></div></header>
<main>
<header class="page-header"><div><p class="eyebrow">SCOPED POLICY DOCUMENT</p><h1>Policy document</h1><p>Manage the current document for this policy, then return to your MCP client to continue.</p></div><aside class="policy-context" aria-label="Policy target"><p>POLICY</p><strong>{policy_name}</strong></aside></header>
{message}
<div class="workspace">
<section class="panel" aria-labelledby="current-document"><div class="panel-heading"><h2 id="current-document">Current document</h2><span class="count">{count}</span></div>{document}</section>
{upload}
</div>
</main>
</body>
</html>"#,
        policy_name = escape_html(&page.policy_name),
        count = if page.document.is_some() {
            "1 current"
        } else {
            "None"
        },
    )
}

fn document_row(document: &PolicyUploadDocumentResponse) -> String {
    format!(
        r#"<table><thead><tr><th>Filename</th><th>Size</th><th>Status</th><th>Actions</th></tr></thead><tbody><tr><td class="filename" data-label="File">{}</td><td data-label="Size">{}</td><td data-label="Status"><span class="status">{}</span></td><td data-label="Actions">{}</td></tr></tbody></table>"#,
        escape_html(&document.filename),
        format_bytes(document.content_length),
        escape_html(document_status_label(document.upload_status)),
        document_actions(document),
    )
}

fn document_actions(document: &PolicyUploadDocumentResponse) -> String {
    let mut actions = Vec::new();
    if document.upload_status == DocumentUploadStatus::Uploaded {
        actions.push(format!(
            r#"<a class="button icon-button" href="/policy-document-uploads/files/{}/download" aria-label="Download policy document"><svg viewBox="0 0 24 24" aria-hidden="true" fill="none" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 3v12"/><path d="m7 10 5 5 5-5"/><path d="M5 21h14"/></svg><span class="sr-only">Download</span></a>"#,
            document.id
        ));
    }
    if matches!(
        document.upload_status,
        DocumentUploadStatus::Uploaded
            | DocumentUploadStatus::ContainsVirus
            | DocumentUploadStatus::FailedUpload
    ) {
        actions.push(format!(
            r#"<form method="post" action="/policy-document-uploads/files/{}/archive" onsubmit="return confirm('Archive this policy document?');"><button class="icon-button danger-button" type="submit" aria-label="Archive policy document"><svg viewBox="0 0 24 24" aria-hidden="true" fill="none" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M3 6h18"/><path d="M8 6V4h8v2"/><path d="M19 6l-1 14H6L5 6"/><path d="M10 11v5"/><path d="M14 11v5"/></svg><span class="sr-only">Archive</span></button></form>"#,
            document.id
        ));
    }

    if actions.is_empty() {
        String::new()
    } else {
        format!(r#"<div class="actions">{}</div>"#, actions.join(""))
    }
}

fn document_status_label(status: DocumentUploadStatus) -> &'static str {
    match status {
        DocumentUploadStatus::PendingUpload => "Uploading",
        DocumentUploadStatus::Finalizing => "Scanning",
        DocumentUploadStatus::Uploaded => "Uploaded",
        DocumentUploadStatus::ContainsVirus => "Upload failed",
        DocumentUploadStatus::FailedUpload => "Upload failed",
    }
}

fn upload_error_response(page: &PolicyUploadPage, error: ApiError) -> Response {
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

fn conflict_response(page: &PolicyUploadPage, message: &str) -> Response {
    (
        StatusCode::CONFLICT,
        Html(render_upload_page(page, Some(message))),
    )
        .into_response()
}

fn redirect_to_management() -> Response {
    let mut response = StatusCode::SEE_OTHER.into_response();
    response
        .headers_mut()
        .insert(LOCATION, HeaderValue::from_static(POLICY_UPLOAD_PATH));
    response
}

fn unavailable_response() -> Response {
    (StatusCode::NOT_FOUND, Html(unavailable_page())).into_response()
}

fn unavailable_page() -> String {
    r#"<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><title>Policy document link unavailable</title><style>:root{color-scheme:dark;--canvas:oklch(17% .012 170);--line:oklch(39% .018 170);--ink:oklch(94% .01 150);--muted:oklch(76% .015 155);--accent:oklch(78% .09 174)}*{box-sizing:border-box}body{margin:0;min-height:100vh;background:var(--canvas);color:var(--ink);font-family:Inter,ui-sans-serif,system-ui,-apple-system,BlinkMacSystemFont,"Segoe UI",sans-serif}header{border-bottom:1px solid var(--line);padding:14px max(16px,calc((100vw - 1080px)/2));font-size:.78rem;font-weight:750;letter-spacing:.08em}header span{color:var(--muted);font-weight:550}main{width:min(690px,calc(100% - 32px));min-height:calc(100vh - 49px);margin:0 auto;display:grid;align-content:center;padding:40px 0}.eyebrow{margin:0 0 10px;color:var(--accent);font-size:.78rem;font-weight:700}h1{max-width:20ch;margin:0 0 12px;font-size:clamp(1.8rem,4vw,2.5rem);line-height:1.05}p{max-width:58ch;margin:0;color:var(--muted);line-height:1.55}</style></head><body><header>PROOFPLANE <span>/ POLICY DOCUMENTS</span></header><main><p class="eyebrow">LINK UNAVAILABLE</p><h1>This policy document link is no longer available</h1><p>Return to your MCP client and request a new policy document link.</p></main></body></html>"#.to_owned()
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

impl From<PolicyDetail> for PolicyUploadPage {
    fn from(detail: PolicyDetail) -> Self {
        Self {
            policy_name: detail.name,
            document: detail
                .document
                .map(|document| PolicyUploadDocumentResponse {
                    id: document.id,
                    filename: document.filename,
                    content_length: document.content_length,
                    upload_status: document.upload_status,
                }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        document_actions, document_status_label, render_upload_page, PolicyUploadDocumentResponse,
        PolicyUploadPage,
    };
    use crate::domain::DocumentUploadStatus;

    #[test]
    fn policy_page_escapes_identity_and_exposes_accessible_responsive_actions() {
        let html = render_upload_page(
            &PolicyUploadPage {
                policy_name: "Security <script>".to_owned(),
                document: Some(PolicyUploadDocumentResponse {
                    id: uuid::Uuid::nil().into(),
                    filename: "controls <final>.pdf".to_owned(),
                    content_length: 2048,
                    upload_status: DocumentUploadStatus::Uploaded,
                }),
            },
            None,
        );

        for expected in [
            "Security &lt;script&gt;",
            "controls &lt;final&gt;.pdf",
            "data-label=\"Status\"",
            "aria-label=\"Download policy document\"",
            "aria-label=\"Archive policy document\"",
            "prefers-reduced-motion",
        ] {
            assert!(html.contains(expected), "missing {expected}");
        }
        assert!(!html.contains("<script>"));
        assert!(!html.contains("Upload document"));
    }

    #[test]
    fn empty_policy_page_offers_exactly_one_file_upload() {
        let html = render_upload_page(
            &PolicyUploadPage {
                policy_name: "Security policy".to_owned(),
                document: None,
            },
            None,
        );

        assert!(html.contains("name=\"file\" type=\"file\" required"));
        assert!(html.contains("Upload document"));
        assert!(html.contains("No policy document yet"));
    }

    #[test]
    fn policy_document_actions_follow_lifecycle_eligibility() {
        for (status, downloadable, archivable) in [
            (DocumentUploadStatus::PendingUpload, false, false),
            (DocumentUploadStatus::Finalizing, false, false),
            (DocumentUploadStatus::ContainsVirus, false, true),
            (DocumentUploadStatus::FailedUpload, false, true),
            (DocumentUploadStatus::Uploaded, true, true),
        ] {
            let actions = document_actions(&PolicyUploadDocumentResponse {
                id: uuid::Uuid::nil().into(),
                filename: "policy.txt".to_owned(),
                content_length: 1,
                upload_status: status,
            });

            assert_eq!(
                actions.contains("Download policy document"),
                downloadable,
                "download eligibility for {status:?}"
            );
            assert_eq!(
                actions.contains("Archive policy document"),
                archivable,
                "archive eligibility for {status:?}"
            );
        }
    }

    #[test]
    fn policy_page_uses_user_facing_status_labels() {
        assert_eq!(
            document_status_label(DocumentUploadStatus::PendingUpload),
            "Uploading"
        );
        assert_eq!(
            document_status_label(DocumentUploadStatus::Finalizing),
            "Scanning"
        );
        assert_eq!(
            document_status_label(DocumentUploadStatus::Uploaded),
            "Uploaded"
        );
        assert_eq!(
            document_status_label(DocumentUploadStatus::ContainsVirus),
            "Upload failed"
        );
        assert_eq!(
            document_status_label(DocumentUploadStatus::FailedUpload),
            "Upload failed"
        );
    }
}
