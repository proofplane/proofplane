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
use secrecy::SecretString;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    application::{
        commands::{
            claim_auditor_auth_transaction::{
                ClaimAuditorAuthTransaction, ClaimAuditorAuthTransactionHandler,
            },
            complete_auditor_authentication::{
                CompleteAuditorAuthentication, CompleteAuditorAuthenticationError,
                CompleteAuditorAuthenticationHandler,
            },
            revoke_auditor_session::{
                RevokeAuditorSession, RevokeAuditorSessionError, RevokeAuditorSessionHandler,
            },
            start_auditor_auth_transaction::{
                StartAuditorAuthTransaction, StartAuditorAuthTransactionError,
                StartAuditorAuthTransactionHandler,
            },
        },
        queries::{
            read_auditor_portal::{ReadAuditorPortal, ReadAuditorPortalHandler},
            resolve_auditor_grant_by_secret::{
                ResolveAuditorGrantBySecret, ResolveAuditorGrantBySecretHandler,
            },
            resolve_auditor_session_by_digest::{
                ResolveAuditorSessionByDigest, ResolveAuditorSessionByDigestError,
                ResolveAuditorSessionByDigestHandler, ResolvedAuditorSession,
            },
        },
        ExecutionMetadata,
    },
    domain::{AuditorSessionTransition, Document, EvidenceSubmission, EvidenceSubmissionId},
    object_storage::ObjectStream,
    observability::audit::{AuditActor, AuditClientType, AuditEvent, AuditObject, AuditOutcome},
    read_models::{
        AuditorPortalControl, AuditorPortalDocument, AuditorPortalEvidence, AuditorPortalPolicy,
        AuditorPortalPolicyDocument, AuditorPortalPolicyDocumentStatus, AuditorPortalPolicySummary,
        AuditorPortalReadModel, AuditorPortalSubmission, ControlSummary, EvidenceDetail,
        FrameworkRequirementDetail,
    },
    routes::{error::ApiError, request_context::RequestId},
    services::document_downloads::{DocumentDownloadService, DownloadError},
};
use chrono::{DateTime, Utc};

const AUDITOR_SESSION_COOKIE: &str = "proofplane_auditor_session";
const REFERRER_POLICY: HeaderName = HeaderName::from_static("referrer-policy");

#[derive(Clone)]
pub struct AuditorAccessState {
    pub resolve_grant: ResolveAuditorGrantBySecretHandler,
    pub start_auth: StartAuditorAuthTransactionHandler,
    pub claim_auth: ClaimAuditorAuthTransactionHandler,
    pub complete_auth: CompleteAuditorAuthenticationHandler,
    pub resolve_session: ResolveAuditorSessionByDigestHandler,
    pub revoke_session: RevokeAuditorSessionHandler,
    pub read_portal: ReadAuditorPortalHandler,
    pub downloads: DocumentDownloadService,
    pub secure_cookie: bool,
}

#[derive(Debug, Deserialize)]
struct InvitePayload {
    token: String,
}

#[derive(Debug, Deserialize)]
struct InviteQuery {
    token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Auth0CallbackQuery {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
}

#[derive(Debug, Serialize)]
struct StatusResponse {
    status: &'static str,
}

pub fn router(state: AuditorAccessState) -> Router {
    Router::new()
        .route("/auditor-access/portal", get(portal_page))
        .route("/auditor-access/portal/data", get(portal_data))
        .route("/auditor-access/portal/policies", get(policies_page))
        .route(
            "/auditor-access/portal/policies/{policy_id}",
            get(policy_page),
        )
        .route(
            "/auditor-access/portal/controls/{control_id}",
            get(standalone_control_page),
        )
        .route(
            "/auditor-access/portal/framework-requirements/{requirement_id}",
            get(requirement_page),
        )
        .route(
            "/auditor-access/portal/framework-requirements/{requirement_id}/controls/{control_id}",
            get(control_page),
        )
        .route(
            "/auditor-access/portal/{*download_path}",
            get(download_document),
        )
        .route("/auditor-access/{workspace_id}", get(open_invite))
        .route("/auditor-access/{workspace_id}/login", post(start_login))
        .route("/auditor-access/auth0/callback", get(auth0_callback))
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
    let grant = match state
        .resolve_grant
        .handle(ResolveAuditorGrantBySecret {
            workspace_id: workspace_id.into(),
            raw_secret: SecretString::from(token.clone()),
        })
        .await
    {
        Ok(Some(grant)) => grant,
        Ok(None) => return Ok(unavailable_response()),
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

async fn start_login(
    State(state): State<AuditorAccessState>,
    Extension(request_id): Extension<RequestId>,
    Path(workspace_id): Path<Uuid>,
    Form(payload): Form<InvitePayload>,
) -> Result<Response, ApiError> {
    let token = payload.token.trim();
    let grant = match state
        .resolve_grant
        .handle(ResolveAuditorGrantBySecret {
            workspace_id: workspace_id.into(),
            raw_secret: SecretString::from(token),
        })
        .await
    {
        Ok(Some(grant)) => grant,
        Ok(None) => {
            audit_auth_start_failed(request_id.0, "grant_unavailable", AuditOutcome::Denied);
            return Ok(unavailable_response());
        }
        Err(error) => {
            audit_auth_start_failed(request_id.0, "grant_persistence", AuditOutcome::Failure);
            return Err(grant_error(error));
        }
    };
    let start = match state
        .start_auth
        .handle(
            StartAuditorAuthTransaction {
                workspace_id: grant.workspace_id,
                grant_id: grant.id,
            },
            ExecutionMetadata::for_request(request_id.0),
        )
        .await
    {
        Ok(start) => start,
        Err(StartAuditorAuthTransactionError::Unavailable) => {
            audit_auth_start_failed(request_id.0, "grant_unavailable", AuditOutcome::Denied);
            return Ok(unavailable_response());
        }
        Err(error) => {
            audit_auth_start_failed(
                request_id.0,
                "transaction_unavailable",
                AuditOutcome::Failure,
            );
            tracing::error!(%error, "auditor authentication start failed");
            return Err(ApiError::Internal);
        }
    };

    audit_auth_started(
        request_id.0,
        workspace_id,
        Uuid::from(grant.id),
        Uuid::from(start.transaction_id),
    );

    let location =
        HeaderValue::from_str(start.redirect_url().as_str()).map_err(|_| ApiError::Internal)?;
    let mut response = StatusCode::SEE_OTHER.into_response();
    response.headers_mut().insert(LOCATION, location);
    Ok(response)
}

async fn auth0_callback(
    State(state): State<AuditorAccessState>,
    Extension(request_id): Extension<RequestId>,
    query: Result<Query<Auth0CallbackQuery>, QueryRejection>,
) -> Result<Response, ApiError> {
    let Query(query) = match query {
        Ok(query) => query,
        Err(_) => {
            audit_auth_failed(request_id.0, "invalid_callback");
            return Ok(authentication_rejected_response());
        }
    };
    if query.error.is_some() {
        if let Some(callback_state) = query.state.as_deref() {
            let _ = state
                .claim_auth
                .handle(
                    ClaimAuditorAuthTransaction {
                        state: SecretString::from(callback_state),
                    },
                    ExecutionMetadata::for_request(request_id.0),
                )
                .await;
        }
        audit_auth_failed(request_id.0, "provider_rejected");
        return Ok(authentication_rejected_response());
    }
    let Some(code) = query.code.filter(|value| !value.trim().is_empty()) else {
        if let Some(callback_state) = query.state.as_deref() {
            let _ = state
                .claim_auth
                .handle(
                    ClaimAuditorAuthTransaction {
                        state: SecretString::from(callback_state),
                    },
                    ExecutionMetadata::for_request(request_id.0),
                )
                .await;
        }
        audit_auth_failed(request_id.0, "invalid_callback");
        return Ok(authentication_rejected_response());
    };
    let Some(callback_state) = query.state.filter(|value| !value.trim().is_empty()) else {
        audit_auth_failed(request_id.0, "invalid_callback");
        return Ok(authentication_rejected_response());
    };

    let completed = match state
        .complete_auth
        .handle(
            CompleteAuditorAuthentication {
                state: SecretString::from(callback_state),
                authorization_code: SecretString::from(code),
            },
            ExecutionMetadata::for_request(request_id.0),
        )
        .await
    {
        Ok(completed) => completed,
        Err(CompleteAuditorAuthenticationError::Rejected) => {
            audit_auth_failed(request_id.0, "rejected");
            return Ok(authentication_rejected_response());
        }
        Err(CompleteAuditorAuthenticationError::GrantUnavailable) => {
            audit_auth_failed(request_id.0, "grant_unavailable");
            return Ok(unavailable_response());
        }
        Err(CompleteAuditorAuthenticationError::ProviderUnavailable) => {
            audit_auth_failed(request_id.0, "provider_unavailable");
            return Ok(authentication_unavailable_response());
        }
        Err(CompleteAuditorAuthenticationError::PersistenceUnavailable) => {
            audit_auth_failed(request_id.0, "persistence_unavailable");
            return Ok(authentication_unavailable_response());
        }
    };

    audit_auth_completed(
        request_id.0,
        Uuid::from(completed.grant.workspace_id),
        Uuid::from(completed.transaction_id),
        &completed.identity.subject,
    );
    audit_auth_session_created(
        request_id.0,
        Uuid::from(completed.grant.workspace_id),
        Uuid::from(completed.created.session.id),
        &completed.identity.subject,
    );

    let mut response = StatusCode::SEE_OTHER.into_response();
    response
        .headers_mut()
        .insert(LOCATION, HeaderValue::from_static("/auditor-access/portal"));
    response.headers_mut().insert(
        SET_COOKIE,
        HeaderValue::from_str(&session_cookie(
            &completed.created.raw_session,
            state.secure_cookie,
        ))
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
        match state
            .revoke_session
            .handle(
                RevokeAuditorSession {
                    raw_session: SecretString::from(raw_session),
                },
                ExecutionMetadata::for_request(request_id.0),
            )
            .await
        {
            Ok(result) if result.transition == AuditorSessionTransition::Revoked => {
                audit(
                    "auditor_access_session.revoked",
                    "logout_auditor_access_session",
                    request_id.0,
                    Uuid::from(result.session.workspace_id),
                    Uuid::from(result.session.id),
                    &result.session.auditor_email,
                );
            }
            Ok(_) | Err(RevokeAuditorSessionError::Unavailable) => {}
            Err(RevokeAuditorSessionError::Repository(error)) => {
                tracing::error!(%error, "auditor access session repository failure");
                return Err(ApiError::Internal);
            }
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
    let session = match resolve_session(&state, raw_session).await? {
        Some(session) => session,
        None => return Ok(unavailable_response()),
    };
    let model = read_portal(&state, &session, request_id.0).await?;

    audit_portal_read(
        request_id.0,
        Uuid::from(session.workspace_id),
        Uuid::from(session.id),
        &session.auditor_email,
    );

    Ok(Html(render_portal_page(&model)).into_response())
}

async fn policies_page(
    State(state): State<AuditorAccessState>,
    Extension(request_id): Extension<RequestId>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let Some(raw_session) = auditor_session_cookie(&headers) else {
        return Ok(unavailable_response());
    };
    let session = match resolve_session(&state, raw_session).await? {
        Some(session) => session,
        None => return Ok(unavailable_response()),
    };
    let model = read_portal(&state, &session, request_id.0).await?;

    audit_portal_read(
        request_id.0,
        Uuid::from(session.workspace_id),
        Uuid::from(session.id),
        &session.auditor_email,
    );
    audit_policy_catalog_read(
        request_id.0,
        Uuid::from(session.workspace_id),
        Uuid::from(session.id),
        &session.auditor_email,
    );

    Ok(Html(render_policies_page(&model)).into_response())
}

async fn policy_page(
    State(state): State<AuditorAccessState>,
    Extension(request_id): Extension<RequestId>,
    Path(policy_id): Path<String>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let Ok(policy_id) = Uuid::parse_str(&policy_id) else {
        return Ok(unavailable_response());
    };
    let Some(raw_session) = auditor_session_cookie(&headers) else {
        return Ok(unavailable_response());
    };
    let session = match resolve_session(&state, raw_session).await? {
        Some(session) => session,
        None => return Ok(unavailable_response()),
    };
    let model = read_portal(&state, &session, request_id.0).await?;

    audit_portal_read(
        request_id.0,
        Uuid::from(session.workspace_id),
        Uuid::from(session.id),
        &session.auditor_email,
    );
    audit_policy_catalog_read(
        request_id.0,
        Uuid::from(session.workspace_id),
        Uuid::from(session.id),
        &session.auditor_email,
    );

    match render_policy_page(&model, policy_id) {
        Some(page) => Ok(Html(page).into_response()),
        None => Ok(unavailable_response()),
    }
}

async fn standalone_control_page(
    State(state): State<AuditorAccessState>,
    Extension(request_id): Extension<RequestId>,
    Path(control_id): Path<String>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let Ok(control_id) = Uuid::parse_str(&control_id) else {
        return Ok(unavailable_response());
    };
    let Some(raw_session) = auditor_session_cookie(&headers) else {
        return Ok(unavailable_response());
    };
    let session = match resolve_session(&state, raw_session).await? {
        Some(session) => session,
        None => return Ok(unavailable_response()),
    };
    let model = read_portal(&state, &session, request_id.0).await?;

    audit_portal_read(
        request_id.0,
        Uuid::from(session.workspace_id),
        Uuid::from(session.id),
        &session.auditor_email,
    );

    match render_standalone_control_page(&model, control_id) {
        Some(page) => Ok(Html(page).into_response()),
        None => Ok(unavailable_response()),
    }
}

async fn requirement_page(
    State(state): State<AuditorAccessState>,
    Extension(request_id): Extension<RequestId>,
    Path(requirement_id): Path<Uuid>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let Some(raw_session) = auditor_session_cookie(&headers) else {
        return Ok(unavailable_response());
    };
    let session = match resolve_session(&state, raw_session).await? {
        Some(session) => session,
        None => return Ok(unavailable_response()),
    };
    let model = read_portal(&state, &session, request_id.0).await?;

    audit_portal_read(
        request_id.0,
        Uuid::from(session.workspace_id),
        Uuid::from(session.id),
        &session.auditor_email,
    );

    match render_requirement_page(&model, requirement_id) {
        Some(page) => Ok(Html(page).into_response()),
        None => Ok(unavailable_response()),
    }
}

async fn control_page(
    State(state): State<AuditorAccessState>,
    Extension(request_id): Extension<RequestId>,
    Path((requirement_id, control_id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let Some(raw_session) = auditor_session_cookie(&headers) else {
        return Ok(unavailable_response());
    };
    let session = match resolve_session(&state, raw_session).await? {
        Some(session) => session,
        None => return Ok(unavailable_response()),
    };
    let model = read_portal(&state, &session, request_id.0).await?;

    audit_portal_read(
        request_id.0,
        Uuid::from(session.workspace_id),
        Uuid::from(session.id),
        &session.auditor_email,
    );

    match render_control_page(&model, requirement_id, control_id) {
        Some(page) => Ok(Html(page).into_response()),
        None => Ok(unavailable_response()),
    }
}

async fn portal_data(
    State(state): State<AuditorAccessState>,
    Extension(request_id): Extension<RequestId>,
    headers: HeaderMap,
) -> Result<Json<AuditorPortalReadModelResponse>, ApiError> {
    let raw_session = auditor_session_cookie(&headers).ok_or(ApiError::NotFound)?;
    let session = resolve_session(&state, raw_session)
        .await?
        .ok_or(ApiError::NotFound)?;
    let model = read_portal(&state, &session, request_id.0).await?;

    audit_portal_read(
        request_id.0,
        Uuid::from(session.workspace_id),
        Uuid::from(session.id),
        &session.auditor_email,
    );
    audit_policy_catalog_read(
        request_id.0,
        Uuid::from(session.workspace_id),
        Uuid::from(session.id),
        &session.auditor_email,
    );

    Ok(Json(model.into()))
}

async fn download_document(
    State(state): State<AuditorAccessState>,
    Extension(request_id): Extension<RequestId>,
    Path(download_path): Path<String>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let identity = parse_download_path(&download_path)?;
    let raw_session = auditor_session_cookie(&headers).ok_or(ApiError::NotFound)?;
    let session = resolve_session(&state, raw_session)
        .await?
        .ok_or(ApiError::NotFound)?;
    let (document, object) = match identity {
        AuditorDownloadIdentity::Evidence {
            submission_id,
            document_id,
        } => {
            let downloaded = state
                .downloads
                .download_for_workspace_within_period(
                    session.workspace_id,
                    session.period.start,
                    session.period.end,
                    submission_id.into(),
                    document_id.into(),
                )
                .await
                .map_err(download_error)?;

            AuditEvent::new(
                "auditor_document.downloaded",
                AuditOutcome::Success,
                AuditActor::System {
                    name: "auditor_browser",
                },
                AuditClientType::Rest,
                "download_auditor_document",
            )
            .workspace_id(Uuid::from(downloaded.audit.workspace_id))
            .request_id(request_id.0)
            .metadata("auditor_email", &session.auditor_email)
            .metadata(
                "evidence_submission_id",
                Uuid::from(downloaded.audit.submission_id),
            )
            .metadata(
                "evidence_document_id",
                Uuid::from(downloaded.audit.document_id),
            )
            .object(AuditObject::new(
                "evidence_document",
                downloaded.audit.document_id.into(),
            ))
            .emit();

            (downloaded.document, downloaded.object)
        }
        AuditorDownloadIdentity::Policy {
            policy_id,
            document_id,
        } => {
            let downloaded = state
                .downloads
                .download_policy_for_workspace(
                    session.workspace_id,
                    policy_id.into(),
                    document_id.into(),
                )
                .await
                .map_err(download_error)?;

            AuditEvent::new(
                "auditor_policy_document.downloaded",
                AuditOutcome::Success,
                AuditActor::System {
                    name: "auditor_browser",
                },
                AuditClientType::Rest,
                "download_auditor_policy_document",
            )
            .workspace_id(Uuid::from(session.workspace_id))
            .request_id(request_id.0)
            .metadata("auditor_email", &session.auditor_email)
            .metadata("policy_id", policy_id)
            .metadata("policy_document_id", document_id)
            .object(AuditObject::new("policy_document", document_id))
            .emit();

            (downloaded.document, downloaded.object)
        }
    };

    stream_download(document, object)
}

fn stream_download(document: Document, object: ObjectStream) -> Result<Response, ApiError> {
    let disposition = crate::routes::document_downloads::content_disposition(&document.filename);
    let mut response = Body::from_stream(object.chunks).into_response();
    *response.status_mut() = StatusCode::OK;
    let headers = response.headers_mut();
    headers.insert(
        CONTENT_TYPE,
        HeaderValue::from_str(&document.content_type).map_err(|_| ApiError::Internal)?,
    );
    headers.insert(
        CONTENT_LENGTH,
        HeaderValue::from_str(&document.content_length.to_string())
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AuditorDownloadIdentity {
    Evidence {
        submission_id: Uuid,
        document_id: Uuid,
    },
    Policy {
        policy_id: Uuid,
        document_id: Uuid,
    },
}

fn parse_download_path(path: &str) -> Result<AuditorDownloadIdentity, ApiError> {
    let segments = path.split('/').collect::<Vec<_>>();
    match segments.as_slice() {
        ["evidence-submissions", submission_id, "documents", document_id, "download"] => {
            let submission_id = Uuid::parse_str(submission_id).map_err(|_| ApiError::NotFound)?;
            let document_id = Uuid::parse_str(document_id).map_err(|_| ApiError::NotFound)?;
            Ok(AuditorDownloadIdentity::Evidence {
                submission_id,
                document_id,
            })
        }
        ["policies", policy_id, "documents", document_id, "download"] => {
            let policy_id = Uuid::parse_str(policy_id).map_err(|_| ApiError::NotFound)?;
            let document_id = Uuid::parse_str(document_id).map_err(|_| ApiError::NotFound)?;
            Ok(AuditorDownloadIdentity::Policy {
                policy_id,
                document_id,
            })
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
<p class="lede">Proofplane will verify this email before opening the read-only evidence portal.</p>
{}
<form class="panel form-panel" method="post" action="/auditor-access/{}/login">
<input type="hidden" name="token" value="{}">
<button type="submit">Continue</button>
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
    let total = model.framework_requirements.len();
    let requirements = &model.framework_requirements;

    let framework_nav = model.framework_requirements.iter().fold(
        Vec::<(&str, &str)>::new(),
        |mut frameworks, requirement| {
            if !frameworks
                .iter()
                .any(|(code, _)| *code == requirement.framework_code)
            {
                frameworks.push((&requirement.framework_code, &requirement.framework_name));
            }
            frameworks
        },
    );
    let framework_links = if framework_nav.len() > 1 {
        format!(
            r#"<nav class="framework-nav" aria-label="Frameworks">{}</nav>"#,
            framework_nav
                .iter()
                .map(|(code, name)| format!(
                    r##"<a href="#framework-{}">{} <span>{}</span></a>"##,
                    escape_html(code),
                    escape_html(name),
                    escape_html(code),
                ))
                .collect::<Vec<_>>()
                .join("")
        )
    } else {
        String::new()
    };

    let rows = if requirements.is_empty() {
        r#"<div class="empty-state"><h2>No framework requirements</h2><p>No framework requirements are available for this auditor portal.</p></div>"#.to_owned()
    } else {
        let mut output = String::new();
        let mut current_framework = "";
        for requirement in requirements {
            if current_framework != requirement.framework_code {
                if !current_framework.is_empty() {
                    output.push_str("</tbody></table></section>");
                }
                current_framework = &requirement.framework_code;
                output.push_str(&format!(
                    r#"<section class="framework-section" id="framework-{}" aria-labelledby="framework-title-{}"><div class="section-heading"><p class="eyebrow">Framework</p><h2 id="framework-title-{}">{}</h2><p>{}</p></div><table class="ledger"><thead><tr><th>Requirement</th><th class="numeric">Mapped controls</th><th class="numeric">Evidence</th><th class="numeric">Evidence submissions</th><th>Coverage</th><th><span class="sr-only">Open</span></th></tr></thead><tbody>"#,
                    escape_html(&requirement.framework_code),
                    escape_html(&requirement.framework_code),
                    escape_html(&requirement.framework_code),
                    escape_html(&requirement.framework_name),
                    escape_html(&requirement.framework_code),
                ));
            }
            output.push_str(&render_requirement_row(model, requirement));
        }
        output.push_str("</tbody></table></section>");
        output
    };

    render_shell(
        "Framework requirements",
        &format!(
            r#"{}<main class="portal" id="main-content">
<nav class="breadcrumbs" aria-label="Breadcrumb"><span aria-current="page">Framework requirements</span></nav>
<header class="page-header">
<div><p class="eyebrow">Auditor portal</p><h1>Framework requirements</h1><p class="lede">Trace each requirement to its controls and submitted evidence.</p></div>
<dl class="page-stats"><div><dt>Requirements</dt><dd>{}</dd></div><div><dt>Controls</dt><dd>{}</dd></div></dl>
</header>
{}
{}
</main>"#,
            render_portal_bar(model, PortalDestination::FrameworkRequirements),
            total,
            model.controls.len(),
            framework_links,
            rows,
        ),
    )
}

fn render_policies_page(model: &AuditorPortalReadModel) -> String {
    let rows = if model.policies.is_empty() {
        r#"<div class="empty-state"><h2>No policies available</h2><p>This workspace does not have any active policies to review.</p></div>"#.to_owned()
    } else {
        model
            .policies
            .iter()
            .map(render_policy_row)
            .collect::<Vec<_>>()
            .join("")
    };

    render_shell(
        "Policies",
        &format!(
            r#"{}<main class="portal" id="main-content">
<nav class="breadcrumbs" aria-label="Breadcrumb"><span aria-current="page">Policies</span></nav>
<header class="page-header">
<div><p class="eyebrow">Auditor portal</p><h1>Policies</h1><p class="lede">Review policies, their control mappings, and current document availability.</p></div>
<dl class="page-stats"><div><dt>Policies</dt><dd>{}</dd></div><div><dt>Mapped controls</dt><dd>{}</dd></div></dl>
</header>
{}
</main>"#,
            render_portal_bar(model, PortalDestination::Policies),
            model.policies.len(),
            model
                .policies
                .iter()
                .map(|policy| policy.controls.len())
                .sum::<usize>(),
            if model.policies.is_empty() {
                rows
            } else {
                format!(
                    r#"<table class="ledger policy-ledger"><thead><tr><th>Policy</th><th class="numeric">Mapped controls</th><th>Document</th><th><span class="sr-only">Open</span></th></tr></thead><tbody>{}</tbody></table>"#,
                    rows
                )
            },
        ),
    )
}

fn render_policy_row(policy: &AuditorPortalPolicy) -> String {
    let description = policy.description.as_deref().unwrap_or("No description");
    format!(
        r#"<tr class="linked-row"><td data-label="Policy"><a class="row-link policy-row-link" href="/auditor-access/portal/policies/{}"><strong>{}</strong><small class="clamped-description">{}</small></a></td><td class="numeric" data-label="Mapped controls">{}</td><td data-label="Document"><span class="coverage {}">{}</span></td><td class="row-arrow" aria-hidden="true">›</td></tr>"#,
        Uuid::from(policy.id),
        escape_html(&policy.name),
        escape_html(description),
        policy.controls.len(),
        policy_document_tone(
            policy
                .document
                .as_ref()
                .map(|document| document.upload_status)
        ),
        policy_document_status(
            policy
                .document
                .as_ref()
                .map(|document| document.upload_status)
        ),
    )
}

fn render_policy_page(model: &AuditorPortalReadModel, policy_id: Uuid) -> Option<String> {
    let policy = model
        .policies
        .iter()
        .find(|policy| Uuid::from(policy.id) == policy_id)?;
    let description = policy
        .description
        .as_deref()
        .map(escape_html)
        .unwrap_or_else(|| "No description".to_owned());
    let controls = if policy.controls.is_empty() {
        r#"<div class="empty-state"><h2>No controls attached</h2><p>This policy is not attached to any controls.</p></div>"#.to_owned()
    } else {
        format!(
            r#"<table class="ledger control-ledger"><thead><tr><th>Control</th><th><span class="sr-only">Open</span></th></tr></thead><tbody>{}</tbody></table>"#,
            policy
                .controls
                .iter()
                .map(|control| render_policy_control_row(model, control))
                .collect::<Vec<_>>()
                .join("")
        )
    };

    Some(render_shell(
        &policy.name,
        &format!(
            r#"{}<main class="portal" id="main-content">
<nav class="breadcrumbs" aria-label="Breadcrumb"><a href="/auditor-access/portal/policies">Policies</a><span aria-hidden="true">›</span><span aria-current="page">{}</span></nav>
<header class="control-detail-header"><div><p class="eyebrow">Policy</p><h1>{}</h1><p class="lede">{}</p></div><div class="header-aside"><dl class="page-stats"><div><dt>Mapped controls</dt><dd>{}</dd></div><div><dt>Document</dt><dd class="status-stat">{}</dd></div></dl>{}</div></header>
<section class="detail-ledger" aria-labelledby="policy-controls-title"><div class="section-heading"><p class="eyebrow">Control mappings</p><h2 id="policy-controls-title">Mapped controls ({})</h2></div>{}</section>
</main>"#,
            render_portal_bar(model, PortalDestination::Policies),
            escape_html(&policy.name),
            escape_html(&policy.name),
            description,
            policy.controls.len(),
            policy_document_status(
                policy
                    .document
                    .as_ref()
                    .map(|document| document.upload_status)
            ),
            render_policy_document_action(policy),
            policy.controls.len(),
            controls,
        ),
    ))
}

fn render_policy_control_row(model: &AuditorPortalReadModel, control: &ControlSummary) -> String {
    format!(
        r#"<tr class="linked-row"><td data-label="Control"><a class="row-link" href="{}"><strong>{}</strong><span>{}</span></a></td><td class="row-arrow" aria-hidden="true">›</td></tr>"#,
        control_detail_path(model, Uuid::from(control.id)),
        escape_html(&control.code),
        escape_html(&control.title),
    )
}

fn render_policy_document_action(policy: &AuditorPortalPolicy) -> String {
    let Some(document) = &policy.document else {
        return String::new();
    };
    if !document.download_eligible {
        return String::new();
    }

    format!(
        r#"<a class="button" href="/auditor-access/portal/policies/{}/documents/{}/download" aria-label="Download policy document {}">Download document</a>"#,
        Uuid::from(policy.id),
        Uuid::from(document.id),
        escape_html(&document.filename),
    )
}

fn control_detail_path(model: &AuditorPortalReadModel, control_id: Uuid) -> String {
    let requirement_id = model
        .controls
        .iter()
        .find(|control| Uuid::from(control.id) == control_id)
        .and_then(|control| control.framework_requirements.first())
        .map(|requirement| Uuid::from(requirement.id));
    match requirement_id {
        Some(requirement_id) => format!(
            "/auditor-access/portal/framework-requirements/{requirement_id}/controls/{control_id}"
        ),
        None => format!("/auditor-access/portal/controls/{control_id}"),
    }
}

fn render_requirement_row(
    model: &AuditorPortalReadModel,
    requirement: &FrameworkRequirementDetail,
) -> String {
    let controls = controls_for_requirement(model, Uuid::from(requirement.id));
    let evidence_count = controls
        .iter()
        .map(|control| control.evidence.len())
        .sum::<usize>();
    let submission_count = controls
        .iter()
        .flat_map(|control| &control.evidence)
        .map(|evidence| evidence.submissions.len())
        .sum::<usize>();
    let submitted_evidence_count = controls
        .iter()
        .flat_map(|control| &control.evidence)
        .filter(|evidence| !evidence.submissions.is_empty())
        .count();
    let (coverage, tone) = coverage_state(controls.len(), evidence_count, submitted_evidence_count);
    let href = format!(
        "/auditor-access/portal/framework-requirements/{}",
        Uuid::from(requirement.id)
    );

    format!(
        r#"<tr class="linked-row"><td data-label="Requirement"><a class="row-link" href="{}"><strong>{}</strong><span>{}</span></a></td><td class="numeric" data-label="Mapped controls">{}</td><td class="numeric" data-label="Evidence">{}</td><td class="numeric" data-label="Evidence submissions">{}</td><td data-label="Coverage"><span class="coverage {}">{}</span></td><td class="row-arrow" aria-hidden="true">›</td></tr>"#,
        href,
        escape_html(&requirement.code),
        escape_html(&requirement.title),
        controls.len(),
        evidence_count,
        submission_count,
        tone,
        coverage,
    )
}

fn render_requirement_page(model: &AuditorPortalReadModel, requirement_id: Uuid) -> Option<String> {
    let requirement = model
        .framework_requirements
        .iter()
        .find(|requirement| Uuid::from(requirement.id) == requirement_id)?;
    let controls = controls_for_requirement(model, requirement_id);
    let rows = if controls.is_empty() {
        r#"<div class="empty-state"><h2>No controls mapped</h2><p>This requirement does not have any mapped controls.</p></div>"#.to_owned()
    } else {
        controls
            .iter()
            .map(|control| render_control_row(requirement, control))
            .collect::<Vec<_>>()
            .join("")
    };

    Some(render_shell(
        &format!("{} {}", requirement.code, requirement.title),
        &format!(
            r#"{}<main class="portal" id="main-content">
<nav class="breadcrumbs" aria-label="Breadcrumb"><a href="/auditor-access/portal">Framework requirements</a><span aria-hidden="true">›</span><span>{}</span><span aria-hidden="true">›</span><span aria-current="page">{}</span></nav>
<header class="detail-header"><p class="eyebrow">Framework requirement</p><h1><span class="detail-code">{}</span>{}</h1><p class="lede">{}</p></header>
<div class="requirement-layout">
<aside class="context-panel" aria-labelledby="context-title"><h2 id="context-title">Requirement context</h2><dl><div><dt>Framework</dt><dd>{}</dd></div><div><dt>Framework code</dt><dd>{}</dd></div><div><dt>Requirement</dt><dd>{}</dd></div></dl></aside>
<section class="detail-ledger" aria-labelledby="controls-title"><div class="section-heading"><p class="eyebrow">Mapped controls</p><h2 id="controls-title">Controls ({})</h2></div>{}</section>
</div></main>"#,
            render_portal_bar(model, PortalDestination::FrameworkRequirements),
            escape_html(&requirement.framework_name),
            escape_html(&requirement.code),
            escape_html(&requirement.code),
            escape_html(&requirement.title),
            escape_html(&requirement.description),
            escape_html(&requirement.framework_name),
            escape_html(&requirement.framework_code),
            escape_html(&requirement.code),
            controls.len(),
            if controls.is_empty() {
                rows
            } else {
                format!(
                    r#"<table class="ledger control-ledger"><thead><tr><th>Control</th><th class="numeric">Evidence</th><th class="numeric">Submissions</th><th>Coverage</th><th><span class="sr-only">Open</span></th></tr></thead><tbody>{}</tbody></table>"#,
                    rows
                )
            },
        ),
    ))
}

fn render_control_row(
    requirement: &FrameworkRequirementDetail,
    control: &AuditorPortalControl,
) -> String {
    let evidence_count = control.evidence.len();
    let submission_count = control
        .evidence
        .iter()
        .map(|evidence| evidence.submissions.len())
        .sum::<usize>();
    let submitted_evidence_count = control
        .evidence
        .iter()
        .filter(|evidence| !evidence.submissions.is_empty())
        .count();
    let (coverage, tone) = coverage_state(1, evidence_count, submitted_evidence_count);
    format!(
        r#"<tr class="linked-row"><td data-label="Control"><a class="row-link" href="/auditor-access/portal/framework-requirements/{}/controls/{}"><strong>{}</strong><span>{}</span></a></td><td class="numeric" data-label="Evidence">{}</td><td class="numeric" data-label="Evidence submissions">{}</td><td data-label="Coverage"><span class="coverage {}">{}</span></td><td class="row-arrow" aria-hidden="true">›</td></tr>"#,
        Uuid::from(requirement.id),
        Uuid::from(control.id),
        escape_html(&control.code),
        escape_html(&control.title),
        evidence_count,
        submission_count,
        tone,
        coverage,
    )
}

fn render_control_page(
    model: &AuditorPortalReadModel,
    requirement_id: Uuid,
    control_id: Uuid,
) -> Option<String> {
    let requirement = model
        .framework_requirements
        .iter()
        .find(|requirement| Uuid::from(requirement.id) == requirement_id)?;
    let control = controls_for_requirement(model, requirement_id)
        .into_iter()
        .find(|control| Uuid::from(control.id) == control_id)?;
    Some(render_control_detail_page(
        model,
        Some(requirement),
        control,
    ))
}

fn render_standalone_control_page(
    model: &AuditorPortalReadModel,
    control_id: Uuid,
) -> Option<String> {
    let control = model
        .controls
        .iter()
        .find(|control| Uuid::from(control.id) == control_id)?;
    Some(render_control_detail_page(model, None, control))
}

fn render_control_detail_page(
    model: &AuditorPortalReadModel,
    requirement: Option<&FrameworkRequirementDetail>,
    control: &AuditorPortalControl,
) -> String {
    let submission_count = control
        .evidence
        .iter()
        .map(|evidence| evidence.submissions.len())
        .sum::<usize>();
    let evidence_blocks = if control.evidence.is_empty() {
        r#"<div class="empty-state"><h2>No evidence mapped</h2><p>No evidence is mapped to this control.</p></div>"#.to_owned()
    } else {
        control
            .evidence
            .iter()
            .rev()
            .enumerate()
            .map(|(index, evidence)| render_evidence(evidence, index == 0))
            .collect::<Vec<_>>()
            .join("")
    };
    let policies = render_attached_policies(control);
    let breadcrumbs = requirement.map_or_else(
        || {
            format!(
                r#"<nav class="breadcrumbs" aria-label="Breadcrumb"><a href="/auditor-access/portal">Framework requirements</a><span aria-hidden="true">›</span><span aria-current="page">{}</span></nav>"#,
                escape_html(&control.code)
            )
        },
        |requirement| {
            format!(
                r#"<nav class="breadcrumbs" aria-label="Breadcrumb"><a href="/auditor-access/portal">Framework requirements</a><span aria-hidden="true">›</span><a href="/auditor-access/portal/framework-requirements/{}">{}</a><span aria-hidden="true">›</span><span aria-current="page">{}</span></nav>"#,
                Uuid::from(requirement.id),
                escape_html(&requirement.code),
                escape_html(&control.code),
            )
        },
    );

    render_shell(
        &format!("{} {}", control.code, control.title),
        &format!(
            r#"{}<main class="portal" id="main-content">
{}
<header class="control-detail-header"><div><p class="eyebrow">Control</p><h1><span class="detail-code">{}</span>{}</h1><p class="lede">{}</p></div><dl class="page-stats"><div><dt>Evidence</dt><dd>{}</dd></div><div><dt>Evidence submissions</dt><dd>{}</dd></div></dl></header>
<section class="attached-policies" aria-labelledby="attached-policies-title"><div class="section-heading"><p class="eyebrow">Policy mappings</p><h2 id="attached-policies-title">Attached policies ({})</h2></div>{}</section>
<section class="evidence-dossier" aria-labelledby="evidence-title"><div class="section-heading"><p class="eyebrow">Evidence</p><h2 id="evidence-title">Submission history</h2></div>{}</section>
</main>"#,
            render_portal_bar(model, PortalDestination::FrameworkRequirements),
            breadcrumbs,
            escape_html(&control.code),
            escape_html(&control.title),
            escape_html(&control.description),
            control.evidence.len(),
            submission_count,
            control.policies.len(),
            policies,
            evidence_blocks,
        ),
    )
}

fn render_attached_policies(control: &AuditorPortalControl) -> String {
    if control.policies.is_empty() {
        return r#"<div class="empty-state"><h3>No policies attached</h3><p>No active policies are attached to this control.</p></div>"#.to_owned();
    }

    format!(
        r#"<table class="ledger policy-ledger"><thead><tr><th>Policy</th><th>Document</th><th><span class="sr-only">Open</span></th></tr></thead><tbody>{}</tbody></table>"#,
        control
            .policies
            .iter()
            .map(|policy| {
                let description = policy.description.as_deref().unwrap_or("No description");
                format!(
                    r#"<tr class="linked-row"><td data-label="Policy"><a class="row-link policy-row-link" href="/auditor-access/portal/policies/{}"><strong>{}</strong><small class="clamped-description">{}</small></a></td><td data-label="Document"><span class="coverage {}">{}</span></td><td class="row-arrow" aria-hidden="true">›</td></tr>"#,
                    Uuid::from(policy.id),
                    escape_html(&policy.name),
                    escape_html(description),
                    policy_document_tone(policy.document.map(|document| document.upload_status)),
                    policy_document_status(policy.document.map(|document| document.upload_status)),
                )
            })
            .collect::<Vec<_>>()
            .join("")
    )
}

fn render_evidence(evidence: &AuditorPortalEvidence, open: bool) -> String {
    let submissions = if evidence.submissions.is_empty() {
        r#"<p class="empty compact">Awaiting submission. No evidence has been received.</p>"#
            .to_owned()
    } else {
        format!(
            r#"<table class="submission-table"><caption class="sr-only">Evidence submissions</caption><thead><tr><th>File</th><th>Received</th><th>Valid from</th><th>Valid until</th><th class="action-column">Actions</th></tr></thead><tbody>{}</tbody></table>"#,
            evidence
                .submissions
                .iter()
                .map(render_submission_row)
                .collect::<Vec<_>>()
                .join(""),
        )
    };

    format!(
        r#"<details class="request-disclosure" {}><summary><span class="summary-title"><strong>{}</strong><small>{}</small></span><span class="summary-meta"><span>{} submissions</span><span class="status-chip">{}</span><span class="disclosure-action"><span class="when-closed sr-only">Expand evidence</span><span class="when-open sr-only">Collapse evidence</span><span class="disclosure-chevron" aria-hidden="true"></span></span></span></summary><div class="request-body">{}</div></details>"#,
        if open { "open" } else { "" },
        escape_html(&evidence.evidence.title),
        escape_html(&evidence.evidence.description),
        evidence.submissions.len(),
        escape_html(evidence.evidence.status.as_str()),
        submissions,
    )
}

fn controls_for_requirement(
    model: &AuditorPortalReadModel,
    requirement_id: Uuid,
) -> Vec<&AuditorPortalControl> {
    model
        .controls
        .iter()
        .filter(|control| {
            control
                .framework_requirements
                .iter()
                .any(|requirement| Uuid::from(requirement.id) == requirement_id)
        })
        .collect()
}

fn coverage_state(
    control_count: usize,
    request_count: usize,
    submitted_request_count: usize,
) -> (&'static str, &'static str) {
    if control_count == 0 {
        ("No controls mapped", "gap")
    } else if request_count == 0 {
        ("No evidence mapped", "gap")
    } else if submitted_request_count < request_count {
        ("Awaiting submission", "gap")
    } else {
        ("Evidence on file", "available")
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PortalDestination {
    FrameworkRequirements,
    Policies,
}

fn render_portal_bar(model: &AuditorPortalReadModel, destination: PortalDestination) -> String {
    let framework_current = if destination == PortalDestination::FrameworkRequirements {
        r#" class="current" aria-current="page""#
    } else {
        ""
    };
    let policies_current = if destination == PortalDestination::Policies {
        r#" class="current" aria-current="page""#
    } else {
        ""
    };
    format!(
        r##"<a class="skip-link" href="#main-content">Skip to main content</a><header class="portal-bar"><a class="portal-brand" href="/auditor-access/portal" aria-label="Proofplane auditor portal"><span aria-hidden="true">◇</span>Proofplane</a><div class="portal-identity"><span>{}</span><span class="readonly">Read-only</span><span class="auditor-email">{}</span><form method="post" action="/auditor-access/logout"><button class="sign-out" type="submit">Sign out</button></form></div></header><nav class="portal-nav" aria-label="Auditor portal"><div><a href="/auditor-access/portal"{}>Framework requirements</a><a href="/auditor-access/portal/policies"{}>Policies</a></div></nav>"##,
        escape_html(&model.workspace_name),
        escape_html(&model.auditor_email),
        framework_current,
        policies_current,
    )
}

fn policy_document_status(status: Option<crate::domain::DocumentUploadStatus>) -> &'static str {
    use crate::domain::DocumentUploadStatus;

    match status {
        None => "Missing",
        Some(DocumentUploadStatus::PendingUpload) => "Uploading",
        Some(DocumentUploadStatus::Finalizing) => "Scanning",
        Some(DocumentUploadStatus::Uploaded) => "Uploaded",
        Some(DocumentUploadStatus::ContainsVirus | DocumentUploadStatus::FailedUpload) => {
            "Upload failed"
        }
    }
}

fn policy_document_tone(status: Option<crate::domain::DocumentUploadStatus>) -> &'static str {
    match status {
        Some(crate::domain::DocumentUploadStatus::Uploaded) => "available",
        _ => "gap",
    }
}

fn render_submission_row(submission: &AuditorPortalSubmission) -> String {
    format!(
        r#"<tr><td class="filename" data-label="File">{}</td><td data-label="Received">{}</td><td data-label="Valid from">{}</td><td data-label="Valid until">{}</td><td class="action-column" data-label="Actions">{}</td></tr>"#,
        escape_html(&submission.document.filename),
        format_datetime(submission.submission.received_at),
        format_date(submission.submission.valid_from),
        format_date(submission.submission.valid_until),
        render_document_action(submission.submission.id, &submission.document),
    )
}

fn render_document_action(
    submission_id: EvidenceSubmissionId,
    document: &AuditorPortalDocument,
) -> String {
    if document.download_eligible {
        format!(
            r#"<a class="button icon-button" href="/auditor-access/portal/evidence-submissions/{}/documents/{}/download" aria-label="Download evidence file"><svg viewBox="0 0 24 24" aria-hidden="true" fill="none" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 3v12"/><path d="m7 10 5 5 5-5"/><path d="M5 21h14"/></svg><span class="sr-only">Download</span></a>"#,
            Uuid::from(submission_id),
            Uuid::from(document.document_id),
        )
    } else {
        r#"<span class="unavailable">Unavailable</span>"#.to_owned()
    }
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
.status-chip {{ display: inline-flex; align-items: center; gap: 7px; }}
.status-chip::before {{ content: ""; flex: 0 0 6px; width: 6px; height: 6px; border-radius: 50%; background: currentColor; }}
.request-list {{ display: grid; gap: 14px; margin-top: 18px; }}
.request {{ padding: 18px; background: var(--surface-raised); }}
.request-heading {{ display: flex; justify-content: space-between; gap: 16px; align-items: start; }}
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

/* Auditor review hierarchy */
body {{ line-height: 1.6; letter-spacing: 0.01em; }}
.sr-only {{ position: absolute; width: 1px; height: 1px; padding: 0; margin: -1px; overflow: hidden; clip: rect(0, 0, 0, 0); white-space: nowrap; border: 0; }}
.skip-link {{ position: fixed; top: 8px; left: 8px; z-index: 20; transform: translateY(-160%); border-radius: 6px; background: var(--accent); color: var(--canvas); padding: 10px 14px; font-weight: 700; text-decoration: none; transition: transform 180ms cubic-bezier(.25,1,.5,1); }}
.skip-link:focus {{ transform: translateY(0); }}
.portal-bar {{ min-height: 64px; display: flex; align-items: center; justify-content: space-between; gap: 24px; padding: 0 max(24px, calc((100vw - 1280px) / 2)); border-bottom: 1px solid var(--line); background: oklch(15% 0.012 170); }}
.portal-brand {{ display: inline-flex; align-items: center; gap: 9px; color: var(--ink); font-size: 1.125rem; font-weight: 680; text-decoration: none; }}
.portal-brand span {{ color: var(--accent); font-size: 1.4rem; }}
.portal-identity {{ display: flex; align-items: center; gap: 16px; color: var(--muted); font-size: 0.8125rem; }}
.readonly {{ display: inline-flex; align-items: center; gap: 6px; color: var(--ink); }}
.readonly::before {{ content: ""; width: 7px; height: 7px; border-radius: 50%; background: var(--accent); }}
.portal-identity form {{ margin: 0; }}
.sign-out {{ min-height: 44px; background: transparent; color: var(--accent); padding: 8px 4px; }}
.sign-out:hover {{ background: transparent; color: var(--ink); }}
.portal-nav {{ border-bottom: 1px solid var(--line); background: oklch(15% 0.012 170); }}
.portal-nav > div {{ display: flex; width: min(1280px, calc(100% - 48px)); margin: 0 auto; gap: 4px; }}
.portal-nav a {{ display: inline-flex; min-height: 44px; align-items: center; border-bottom: 2px solid transparent; padding: 0 12px; color: var(--muted); font-size: 0.8125rem; font-weight: 620; text-decoration: none; }}
.portal-nav a:hover {{ color: var(--ink); background: var(--surface); }}
.portal-nav a.current {{ border-bottom-color: var(--accent); color: var(--accent); }}
.portal-nav a:focus-visible, .breadcrumbs a:focus-visible, .row-link:focus-visible {{ outline: 2px solid var(--signal); outline-offset: -2px; }}
main.portal {{ width: min(1280px, calc(100% - 48px)); padding: 28px 0 64px; }}
.breadcrumbs {{ display: flex; align-items: center; flex-wrap: wrap; gap: 10px; min-height: 32px; margin-bottom: 32px; color: var(--muted); font-size: 0.8125rem; }}
.breadcrumbs a {{ color: var(--accent); text-decoration: none; }}
.breadcrumbs a:hover {{ color: var(--ink); }}
.page-header, .control-detail-header {{ display: flex; align-items: end; justify-content: space-between; gap: 40px; padding-bottom: 32px; border-bottom: 1px solid var(--line); margin-bottom: 32px; }}
.page-header h1, .detail-header h1, .control-detail-header h1 {{ font-size: 2rem; line-height: 1.15; margin-bottom: 10px; text-wrap: balance; }}
.detail-header {{ max-width: 860px; padding-bottom: 32px; margin-bottom: 0; }}
.detail-code {{ display: inline-block; margin-right: 12px; color: var(--accent); font-variant-numeric: tabular-nums; }}
.page-stats {{ display: flex; flex: 0 0 auto; gap: 0; margin: 0; }}
.page-stats div {{ min-width: 132px; padding: 0 24px; border-inline-start: 1px solid var(--line); }}
.page-stats dd {{ margin-top: 7px; color: var(--ink); font-size: 1.25rem; font-weight: 680; font-variant-numeric: tabular-nums; }}
.framework-nav {{ display: flex; gap: 8px; flex-wrap: wrap; margin-bottom: 28px; }}
.framework-nav a {{ display: inline-flex; gap: 8px; min-height: 44px; align-items: center; border: 1px solid var(--line); border-radius: 6px; padding: 8px 12px; color: var(--ink); text-decoration: none; }}
.framework-nav a:hover {{ background: var(--surface); border-color: var(--muted); }}
.framework-nav span {{ color: var(--muted); }}
.framework-section + .framework-section {{ margin-top: 48px; }}
.section-heading {{ margin-bottom: 18px; }}
.section-heading h2 {{ font-size: 1.25rem; margin-bottom: 4px; }}
.section-heading p:last-child {{ margin-bottom: 0; }}
.ledger {{ width: 100%; margin: 0; border-top: 1px solid var(--line); border-bottom: 1px solid var(--line); font-variant-numeric: tabular-nums; }}
.ledger th {{ background: oklch(19% 0.013 170); padding: 12px 14px; color: var(--muted); font-size: 0.75rem; text-transform: none; }}
.ledger td {{ padding: 0 14px; background: transparent; }}
.ledger th.numeric, .ledger td.numeric {{ width: 150px; text-align: right; }}
.ledger .linked-row {{ position: relative; padding: 0; border: 0; }}
.ledger .linked-row:hover td {{ background: var(--surface); }}
.ledger .linked-row:has(.row-link:focus-visible) td {{ background: var(--surface); }}
.row-link {{ display: grid; grid-template-columns: minmax(72px, auto) 1fr; gap: 12px; align-items: baseline; min-height: 64px; padding: 18px 0; color: var(--ink); text-decoration: none; }}
.row-link::after {{ content: ""; position: absolute; inset: 0; }}
.row-link strong {{ color: var(--accent); font-size: 0.9375rem; }}
.row-link span {{ color: var(--ink); }}
.row-link small {{ grid-column: 2; color: var(--muted); font-size: 0.8125rem; line-height: 1.45; }}
.row-arrow {{ width: 32px; color: var(--muted); font-size: 1.35rem; text-align: right; }}
.coverage {{ display: inline-flex; align-items: center; min-height: 28px; border-radius: 4px; padding: 4px 8px; font-size: 0.8125rem; font-weight: 620; white-space: nowrap; }}
.coverage.gap {{ background: oklch(24% 0.035 48); color: var(--signal); }}
.coverage.available {{ background: oklch(24% 0.035 170); color: var(--accent); }}
.pagination {{ display: flex; align-items: center; justify-content: space-between; gap: 24px; padding-top: 24px; color: var(--muted); font-size: 0.8125rem; font-variant-numeric: tabular-nums; }}
.pagination div {{ display: flex; align-items: center; gap: 16px; }}
.pagination a, .pagination div > span:first-child:last-child {{ min-height: 44px; display: inline-flex; align-items: center; color: var(--accent); text-decoration: none; }}
.pagination [aria-disabled="true"] {{ color: var(--muted); opacity: .55; }}
.requirement-layout {{ display: grid; grid-template-columns: minmax(190px, 260px) minmax(0, 1fr); gap: 48px; padding-top: 32px; border-top: 1px solid var(--line); }}
.context-panel {{ padding-right: 28px; border-inline-end: 1px solid var(--line); }}
.context-panel h2 {{ font-size: 0.8125rem; color: var(--muted); text-transform: uppercase; letter-spacing: .08em; }}
.context-panel dl {{ display: grid; gap: 20px; margin: 24px 0 0; }}
.context-panel dd {{ color: var(--ink); }}
.detail-ledger .ledger {{ margin-top: 20px; }}
.control-ledger .row-link {{ grid-template-columns: minmax(72px, auto) 1fr; }}
.control-ledger .row-link strong {{ white-space: nowrap; }}
.policy-ledger .policy-row-link {{ grid-template-columns: 1fr; gap: 4px; }}
.policy-ledger .policy-row-link strong {{ color: var(--ink); font-size: 0.9375rem; }}
.policy-ledger .policy-row-link small {{ grid-column: 1; max-width: 72ch; }}
.clamped-description {{ display: -webkit-box; overflow: hidden; -webkit-box-orient: vertical; -webkit-line-clamp: 2; }}
.evidence-dossier {{ max-width: 1180px; }}
.attached-policies {{ max-width: 1180px; margin-bottom: 48px; }}
.header-aside {{ display: flex; flex: 0 0 auto; align-items: end; gap: 28px; }}
.header-aside .button {{ display: inline-flex; align-items: center; min-height: 44px; }}
.status-stat {{ max-width: 160px; font-size: 1rem !important; }}
.request-disclosure {{ border-top: 1px solid var(--line); }}
.request-disclosure:last-child {{ border-bottom: 1px solid var(--line); }}
.request-disclosure summary {{ display: grid; grid-template-columns: minmax(280px, 1fr) auto; gap: 24px; align-items: center; min-height: 68px; padding: 14px 16px; cursor: pointer; list-style: none; }}
.request-disclosure summary::-webkit-details-marker {{ display: none; }}
.request-disclosure summary:hover {{ background: var(--surface); }}
.request-disclosure summary:focus-visible {{ outline: 2px solid var(--signal); outline-offset: -2px; }}
.summary-title {{ display: grid; align-content: center; gap: 3px; }}
.summary-title small {{ color: var(--muted); font-size: 0.8125rem; }}
.summary-meta {{ display: flex; align-items: center; justify-content: end; gap: 20px; color: var(--muted); font-size: 0.8125rem; font-variant-numeric: tabular-nums; }}
.disclosure-action {{ display: inline-grid; place-items: center; flex: 0 0 36px; width: 36px; height: 36px; border: 1px solid var(--line); border-radius: 6px; color: var(--accent); }}
.request-disclosure summary:hover .disclosure-action {{ border-color: var(--accent); background: oklch(24% 0.035 170); }}
.when-open {{ display: none; }}
.request-disclosure[open] .when-open {{ display: inline; }}
.request-disclosure[open] .when-closed {{ display: none; }}
.disclosure-chevron {{ display: block; width: 8px; height: 8px; border-right: 2px solid currentColor; border-bottom: 2px solid currentColor; transform: translateY(-2px) rotate(45deg); transition: transform 180ms cubic-bezier(.25,1,.5,1); }}
.request-disclosure[open] .disclosure-chevron {{ transform: translateY(2px) rotate(225deg); }}
.request-body {{ padding: 4px 16px 28px 40px; }}
.request-body > p {{ max-width: 70ch; }}
.submission-table {{ margin-top: 0; }}
.submission-table th {{
  background: transparent;
  color: var(--muted);
  font-size: 0.8125rem;
  font-weight: 620;
}}
.submission-table td {{ font-variant-numeric: tabular-nums; }}
.submission-table .filename {{ color: var(--ink); font-weight: 650; overflow-wrap: anywhere; }}
.submission-table .action-column {{ width: 1%; text-align: right; white-space: nowrap; }}
.submission-table .unavailable {{ display: inline-grid; height: 40px; place-items: center; color: var(--muted); font-size: 0.8125rem; }}
.icon-button {{
  display: inline-grid;
  width: 40px;
  height: 40px;
  place-items: center;
  padding: 0;
  background: transparent;
  color: var(--muted);
  transition: background-color 160ms cubic-bezier(.25,1,.5,1), color 160ms cubic-bezier(.25,1,.5,1);
}}
.icon-button svg {{ width: 18px; height: 18px; stroke: currentColor; }}
.icon-button:hover {{ background: var(--surface-raised); color: var(--ink); }}
.checksum {{ font-family: "IBM Plex Mono", "SFMono-Regular", Consolas, monospace; font-size: 0.8125rem; font-variant-ligatures: none; }}
.empty-state {{ padding: 40px 0; border-top: 1px solid var(--line); border-bottom: 1px solid var(--line); }}
.empty-state h2 {{ font-size: 1.25rem; }}

@media (max-width: 1100px) {{
  .auditor-email {{ display: none; }}
  .requirement-layout {{ grid-template-columns: 1fr; gap: 28px; }}
  .context-panel {{ padding: 0 0 24px; border-inline-end: 0; border-bottom: 1px solid var(--line); }}
  .context-panel dl {{ grid-template-columns: repeat(3, 1fr); }}
  .header-aside {{ display: grid; gap: 20px; align-items: start; }}
}}

@media (max-width: 900px) {{
  .request-disclosure summary {{ grid-template-columns: 1fr; gap: 8px; }}
  .summary-meta {{ justify-content: start; flex-wrap: wrap; gap: 8px 16px; }}
}}

@media (max-width: 720px) {{
  .portal-bar {{ min-height: 56px; padding: 0 12px; }}
  .portal-nav > div {{ width: calc(100% - 24px); overflow-x: auto; }}
  .portal-nav a {{ flex: 0 0 auto; }}
  .portal-identity {{ gap: 10px; }}
  .portal-identity > span:first-child {{ display: none; }}
  .portal-brand {{ font-size: 1rem; }}
  main.portal {{ width: min(100% - 24px, 1280px); padding-top: 20px; }}
  .breadcrumbs {{ margin-bottom: 24px; }}
  .page-header, .control-detail-header {{ display: grid; gap: 20px; align-items: start; }}
  .page-stats {{ width: 100%; border-top: 1px solid var(--line); padding-top: 16px; }}
  .page-stats div {{ flex: 1; min-width: 0; padding: 0 16px 0 0; border: 0; }}
  .page-stats div + div {{ padding-left: 16px; border-inline-start: 1px solid var(--line); }}
  .ledger, .ledger tbody {{ display: block; border: 0; }}
  .ledger thead {{ display: none; }}
  .ledger tr {{ display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 12px 16px; padding: 18px 0; border-top: 1px solid var(--line); }}
  .ledger tr:last-child {{ border-bottom: 1px solid var(--line); }}
  .ledger td, .ledger td.numeric {{ display: block; width: auto; padding: 0; border: 0; text-align: left; }}
  .ledger td::before {{ margin-bottom: 4px; }}
  .ledger td:first-child {{ grid-column: 1 / -1; }}
  .ledger .row-arrow {{ display: none; }}
  .row-link {{ min-height: 44px; padding: 0; }}
  .coverage {{ white-space: normal; }}
  .pagination {{ align-items: start; }}
  .pagination div {{ gap: 10px; flex-wrap: wrap; justify-content: end; }}
  .context-panel dl {{ grid-template-columns: 1fr; }}
  .request-disclosure summary {{ padding-left: 28px; }}
  .request-body {{ padding-left: 12px; padding-right: 12px; }}
  .submission-table tr {{ display: block; padding: 16px 0; }}
  .submission-table .action-column {{ width: auto; text-align: left; }}
}}

@media (prefers-reduced-motion: reduce) {{
  *, *::before, *::after {{ scroll-behavior: auto !important; transition-duration: .01ms !important; }}
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

fn authentication_rejected_response() -> Response {
    (
        StatusCode::BAD_REQUEST,
        Html(render_shell(
            "Auditor verification failed",
            r#"<main class="narrow">
<p class="eyebrow">Verification failed</p>
<h1>We couldn&#39;t verify this access request</h1>
<p class="lede">Return to your invitation and try again.</p>
</main>"#,
        )),
    )
        .into_response()
}

fn authentication_unavailable_response() -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Html(render_shell(
            "Auditor verification unavailable",
            r#"<main class="narrow">
<p class="eyebrow">Verification unavailable</p>
<h1>Email verification is temporarily unavailable</h1>
<p class="lede">Please try again from your invitation.</p>
</main>"#,
        )),
    )
        .into_response()
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

fn audit_auth_started(request_id: Uuid, workspace_id: Uuid, grant_id: Uuid, transaction_id: Uuid) {
    AuditEvent::new(
        "auditor_access_auth.started",
        AuditOutcome::Success,
        AuditActor::System {
            name: "auditor_browser",
        },
        AuditClientType::Rest,
        "start_auditor_authentication",
    )
    .workspace_id(workspace_id)
    .request_id(request_id)
    .metadata("transaction_id", transaction_id)
    .object(AuditObject::new("auditor_access_grant", grant_id))
    .emit();
}

fn audit_auth_start_failed(request_id: Uuid, category: &'static str, outcome: AuditOutcome) {
    AuditEvent::new(
        "auditor_access_auth.started",
        outcome,
        AuditActor::System {
            name: "auditor_browser",
        },
        AuditClientType::Rest,
        "start_auditor_authentication",
    )
    .request_id(request_id)
    .metadata("failure_category", category)
    .emit();
}

fn audit_auth_completed(
    request_id: Uuid,
    workspace_id: Uuid,
    transaction_id: Uuid,
    auth0_subject: &str,
) {
    AuditEvent::new(
        "auditor_access_auth.completed",
        AuditOutcome::Success,
        AuditActor::System {
            name: "auditor_browser",
        },
        AuditClientType::Rest,
        "complete_auditor_authentication",
    )
    .workspace_id(workspace_id)
    .request_id(request_id)
    .metadata("auth0_subject", auth0_subject)
    .object(AuditObject::new("auditor_auth_transaction", transaction_id))
    .emit();
}

fn audit_auth_session_created(
    request_id: Uuid,
    workspace_id: Uuid,
    session_id: Uuid,
    auth0_subject: &str,
) {
    AuditEvent::new(
        "auditor_access_session.created",
        AuditOutcome::Success,
        AuditActor::System {
            name: "auditor_browser",
        },
        AuditClientType::Rest,
        "create_auditor_access_session",
    )
    .workspace_id(workspace_id)
    .request_id(request_id)
    .metadata("auth0_subject", auth0_subject)
    .object(AuditObject::new("auditor_access_session", session_id))
    .emit();
}

fn audit_auth_failed(request_id: Uuid, category: &'static str) {
    AuditEvent::new(
        "auditor_access_auth.completed",
        AuditOutcome::Failure,
        AuditActor::System {
            name: "auditor_browser",
        },
        AuditClientType::Rest,
        "complete_auditor_authentication",
    )
    .request_id(request_id)
    .metadata("failure_category", category)
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

fn audit_policy_catalog_read(
    request_id: Uuid,
    workspace_id: Uuid,
    session_id: Uuid,
    auditor_email: &str,
) {
    AuditEvent::new(
        "auditor_policy_catalog.read",
        AuditOutcome::Success,
        AuditActor::System {
            name: "auditor_browser",
        },
        AuditClientType::Rest,
        "read_auditor_policy_catalog",
    )
    .workspace_id(workspace_id)
    .request_id(request_id)
    .metadata("auditor_email", auditor_email)
    .object(AuditObject::new("auditor_access_session", session_id))
    .emit();
}

fn grant_error(error: crate::persistence::Error) -> ApiError {
    tracing::error!(%error, "auditor access grant repository failure");
    ApiError::Internal
}

fn session_error(error: ResolveAuditorSessionByDigestError) -> ApiError {
    match error {
        ResolveAuditorSessionByDigestError::Repository(error) => {
            tracing::error!(%error, "auditor access session repository failure");
            ApiError::Internal
        }
    }
}

async fn resolve_session(
    state: &AuditorAccessState,
    raw_session: &str,
) -> Result<Option<ResolvedAuditorSession>, ApiError> {
    state
        .resolve_session
        .handle(ResolveAuditorSessionByDigest {
            raw_session: SecretString::from(raw_session),
        })
        .await
        .map_err(session_error)
}

async fn read_portal(
    state: &AuditorAccessState,
    session: &ResolvedAuditorSession,
    request_id: Uuid,
) -> Result<AuditorPortalReadModel, ApiError> {
    state
        .read_portal
        .handle(
            ReadAuditorPortal {
                session: session.clone(),
            },
            ExecutionMetadata::for_request(request_id),
        )
        .await
        .map_err(portal_error)
}

fn portal_error(error: crate::persistence::Error) -> ApiError {
    tracing::error!(%error, "auditor portal read model failure");
    ApiError::Internal
}

fn download_error(error: DownloadError) -> ApiError {
    match error {
        DownloadError::NotFound | DownloadError::NotReady => ApiError::NotFound,
        DownloadError::MetadataMismatch | DownloadError::Internal => {
            tracing::error!(%error, "auditor document download failed");
            ApiError::Internal
        }
        DownloadError::Repository(repository_error) => {
            tracing::error!(error = %repository_error, "auditor document download repository failure");
            ApiError::Internal
        }
    }
}

#[derive(Debug, Serialize)]
struct AuditorPortalReadModelResponse {
    workspace_name: String,
    auditor_email: String,
    framework_requirements: Vec<FrameworkRequirementResponse>,
    controls: Vec<AuditorPortalControlResponse>,
    policies: Vec<AuditorPortalPolicyResponse>,
}

impl From<AuditorPortalReadModel> for AuditorPortalReadModelResponse {
    fn from(model: AuditorPortalReadModel) -> Self {
        Self {
            workspace_name: model.workspace_name,
            auditor_email: model.auditor_email,
            framework_requirements: model
                .framework_requirements
                .into_iter()
                .map(Into::into)
                .collect(),
            controls: model.controls.into_iter().map(Into::into).collect(),
            policies: model.policies.into_iter().map(Into::into).collect(),
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
    evidence: Vec<AuditorPortalEvidenceResponse>,
    policies: Vec<AuditorPortalPolicySummaryResponse>,
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
            evidence: control.evidence.into_iter().map(Into::into).collect(),
            policies: control.policies.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Debug, Serialize)]
struct AuditorPortalPolicyResponse {
    id: Uuid,
    name: String,
    description: Option<String>,
    controls: Vec<AuditorPortalPolicyControlResponse>,
    document: Option<AuditorPortalPolicyDocumentResponse>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<AuditorPortalPolicy> for AuditorPortalPolicyResponse {
    fn from(policy: AuditorPortalPolicy) -> Self {
        Self {
            id: policy.id.into(),
            name: policy.name,
            description: policy.description,
            controls: policy.controls.into_iter().map(Into::into).collect(),
            document: policy.document.map(Into::into),
            created_at: policy.created_at,
            updated_at: policy.updated_at,
        }
    }
}

#[derive(Debug, Serialize)]
struct AuditorPortalPolicySummaryResponse {
    id: Uuid,
    name: String,
    description: Option<String>,
    document: Option<AuditorPortalPolicyDocumentStatusResponse>,
}

impl From<AuditorPortalPolicySummary> for AuditorPortalPolicySummaryResponse {
    fn from(policy: AuditorPortalPolicySummary) -> Self {
        Self {
            id: policy.id.into(),
            name: policy.name,
            description: policy.description,
            document: policy.document.map(Into::into),
        }
    }
}

#[derive(Debug, Serialize)]
struct AuditorPortalPolicyDocumentStatusResponse {
    download_eligible: bool,
}

impl From<AuditorPortalPolicyDocumentStatus> for AuditorPortalPolicyDocumentStatusResponse {
    fn from(document: AuditorPortalPolicyDocumentStatus) -> Self {
        Self {
            download_eligible: document.upload_status
                == crate::domain::DocumentUploadStatus::Uploaded,
        }
    }
}

#[derive(Debug, Serialize)]
struct AuditorPortalPolicyControlResponse {
    id: Uuid,
    code: String,
    title: String,
    description: String,
}

impl From<ControlSummary> for AuditorPortalPolicyControlResponse {
    fn from(control: ControlSummary) -> Self {
        Self {
            id: control.id.into(),
            code: control.code,
            title: control.title,
            description: control.description,
        }
    }
}

#[derive(Debug, Serialize)]
struct AuditorPortalPolicyDocumentResponse {
    id: Uuid,
    policy_id: Uuid,
    filename: String,
    created_at: DateTime<Utc>,
    download_eligible: bool,
}

impl From<AuditorPortalPolicyDocument> for AuditorPortalPolicyDocumentResponse {
    fn from(document: AuditorPortalPolicyDocument) -> Self {
        Self {
            id: document.id.into(),
            policy_id: document.policy_id.into(),
            filename: document.filename,
            created_at: document.created_at,
            download_eligible: document.download_eligible,
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

impl From<FrameworkRequirementDetail> for FrameworkRequirementResponse {
    fn from(requirement: FrameworkRequirementDetail) -> Self {
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
struct AuditorPortalEvidenceResponse {
    mapping_rationale: String,
    mapping_created_at: DateTime<Utc>,
    evidence: EvidenceResponse,
    submissions: Vec<AuditorPortalSubmissionResponse>,
}

impl From<AuditorPortalEvidence> for AuditorPortalEvidenceResponse {
    fn from(evidence: AuditorPortalEvidence) -> Self {
        Self {
            mapping_rationale: evidence.mapping_rationale,
            mapping_created_at: evidence.mapping_created_at,
            evidence: evidence.evidence.into(),
            submissions: evidence.submissions.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Debug, Serialize)]
struct EvidenceResponse {
    id: Uuid,
    title: String,
    description: String,
    collection_instructions: String,
    status: &'static str,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<EvidenceDetail> for EvidenceResponse {
    fn from(evidence: EvidenceDetail) -> Self {
        Self {
            id: Uuid::from(evidence.id),
            title: evidence.title,
            description: evidence.description,
            collection_instructions: evidence.collection_instructions,
            status: evidence.status.as_str(),
            created_at: evidence.created_at,
            updated_at: evidence.updated_at,
        }
    }
}

#[derive(Debug, Serialize)]
struct AuditorPortalSubmissionResponse {
    submission: EvidenceSubmissionResponse,
    document: AuditorPortalDocumentResponse,
}

impl From<AuditorPortalSubmission> for AuditorPortalSubmissionResponse {
    fn from(submission: AuditorPortalSubmission) -> Self {
        Self {
            submission: submission.submission.into(),
            document: submission.document.into(),
        }
    }
}

#[derive(Debug, Serialize)]
struct EvidenceSubmissionResponse {
    id: Uuid,
    evidence_id: Uuid,
    submitted_by: EvidenceSubmitterResponse,
    received_at: DateTime<Utc>,
    valid_from: DateTime<Utc>,
    valid_until: DateTime<Utc>,
}

impl From<EvidenceSubmission> for EvidenceSubmissionResponse {
    fn from(submission: EvidenceSubmission) -> Self {
        Self {
            id: Uuid::from(submission.id),
            evidence_id: Uuid::from(submission.evidence_id),
            submitted_by: EvidenceSubmitterResponse::from(submission.submitted_by),
            received_at: submission.received_at,
            valid_from: submission.valid_from,
            valid_until: submission.valid_until,
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
struct AuditorPortalDocumentResponse {
    document_id: Uuid,
    filename: String,
    download_eligible: bool,
}

impl From<AuditorPortalDocument> for AuditorPortalDocumentResponse {
    fn from(document: AuditorPortalDocument) -> Self {
        Self {
            document_id: Uuid::from(document.document_id),
            filename: document.filename,
            download_eligible: document.download_eligible,
        }
    }
}
