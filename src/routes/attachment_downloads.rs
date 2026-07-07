use std::collections::HashMap;

use axum::{
    body::Body,
    extract::{rejection::QueryRejection, Path, Query, Request, State},
    http::{
        header::{CACHE_CONTROL, CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_TYPE},
        HeaderName, HeaderValue, Method, StatusCode,
    },
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
    Extension, Json, Router,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    authentication::{ApiTokenAuthenticator, ApiTokenContext},
    domain::{EvidenceAttachmentId, EvidenceSubmissionId, WorkspacePermission},
    observability::audit::{AuditActor, AuditClientType, AuditEvent, AuditObject, AuditOutcome},
    routes::{
        authentication::authorize_workspace_route, error::ApiError, request_context::RequestId,
    },
    services::attachment_downloads::{
        AttachmentDownloadService, DownloadError, IssuedDownloadGrant,
    },
};

const REFERRER_POLICY: HeaderName = HeaderName::from_static("referrer-policy");

#[derive(Clone)]
pub struct AttachmentDownloadState {
    pub service: AttachmentDownloadService,
    pub route_auth: AttachmentDownloadRouteAuthState,
}

#[derive(Clone)]
pub struct AttachmentDownloadRouteAuthState {
    pub authenticator: ApiTokenAuthenticator,
}

pub fn router(state: AttachmentDownloadState) -> Router {
    let authenticated = Router::new()
        .route(
            "/workspaces/{workspace_id}/evidence-submissions/{submission_id}/attachments/{attachment_id}/download-grants",
            post(issue_download_grant),
        )
        .route_layer(middleware::from_fn_with_state(
            state.route_auth.clone(),
            authorize_download_grant_route,
        ));

    Router::new()
        .merge(authenticated)
        .route("/attachment-downloads", get(redeem_download_grant))
        .with_state(state)
}

async fn authorize_download_grant_route(
    State(state): State<AttachmentDownloadRouteAuthState>,
    Path(path): Path<HashMap<String, String>>,
    mut request: Request,
    next: Next,
) -> Result<Response, ApiError> {
    if request.method() != Method::POST {
        return Err(ApiError::MethodNotAllowed);
    }

    let token = authorize_workspace_route(&state.authenticator, &path, &mut request).await?;
    if !token
        .permissions
        .has(WorkspacePermission::ReadEvidenceSubmissions)
    {
        return Err(ApiError::NotFound);
    }

    Ok(next.run(request).await)
}

#[derive(Debug, Deserialize)]
struct DownloadGrantPath {
    workspace_id: Uuid,
    submission_id: Uuid,
    attachment_id: Uuid,
}

#[derive(Debug, Serialize)]
struct DownloadGrantResponse {
    url: String,
    expires_at: chrono::DateTime<chrono::Utc>,
    filename: String,
    content_type: String,
    content_length: i64,
}

#[derive(Debug, Deserialize)]
struct DownloadGrantQuery {
    token: String,
}

impl From<IssuedDownloadGrant> for DownloadGrantResponse {
    fn from(grant: IssuedDownloadGrant) -> Self {
        Self {
            url: grant.url.to_string(),
            expires_at: grant.expires_at,
            filename: grant.filename,
            content_type: grant.content_type,
            content_length: grant.content_length,
        }
    }
}

async fn issue_download_grant(
    State(state): State<AttachmentDownloadState>,
    Path(path): Path<DownloadGrantPath>,
    Extension(token): Extension<ApiTokenContext>,
    Extension(request_id): Extension<RequestId>,
) -> Result<Json<DownloadGrantResponse>, ApiError> {
    let grant = state
        .service
        .issue(
            &token,
            EvidenceSubmissionId::from(path.submission_id),
            EvidenceAttachmentId::from(path.attachment_id),
        )
        .await
        .map_err(download_error)?;

    AuditEvent::new(
        "evidence_attachment_download_grant.issued",
        AuditOutcome::Success,
        AuditActor::ApiToken {
            user_id: token.user_id.into(),
            api_token_id: token.api_token_id.into(),
        },
        AuditClientType::Rest,
        "issue_attachment_download_grant",
    )
    .workspace_id(path.workspace_id)
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

    Ok(Json(grant.into()))
}

async fn redeem_download_grant(
    State(state): State<AttachmentDownloadState>,
    Extension(request_id): Extension<RequestId>,
    query: Result<Query<DownloadGrantQuery>, QueryRejection>,
) -> Result<Response, ApiError> {
    let Query(query) = query.map_err(|_| ApiError::NotFound)?;
    if query.token.is_empty() {
        return Err(ApiError::NotFound);
    }

    let downloaded = state
        .service
        .redeem(&query.token)
        .await
        .map_err(download_error)?;
    AuditEvent::new(
        "evidence_attachment_download_grant.redeemed",
        AuditOutcome::Success,
        download_audit_actor(
            downloaded.audit.issued_by_user_id,
            downloaded.audit.issued_via,
        ),
        AuditClientType::Rest,
        "redeem_attachment_download_grant",
    )
    .workspace_id(downloaded.audit.workspace_id.into())
    .request_id(request_id.0)
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
    let disposition = content_disposition(&downloaded.attachment.filename);
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

fn content_disposition(filename: &str) -> String {
    format!("attachment; filename=\"{filename}\"")
}

fn download_audit_actor(
    user_id: crate::domain::UserId,
    issued_via: crate::services::attachment_downloads::DownloadGrantIssuer,
) -> AuditActor {
    match issued_via {
        crate::services::attachment_downloads::DownloadGrantIssuer::ApiToken(api_token_id) => {
            AuditActor::ApiToken {
                user_id: user_id.into(),
                api_token_id: api_token_id.into(),
            }
        }
        crate::services::attachment_downloads::DownloadGrantIssuer::AgentConnection(
            agent_connection_id,
        ) => AuditActor::AgentConnection {
            user_id: user_id.into(),
            agent_connection_id: agent_connection_id.into(),
        },
    }
}

fn download_error(error: DownloadError) -> ApiError {
    match error {
        DownloadError::NotFound => ApiError::NotFound,
        DownloadError::NotReady => ApiError::Conflict {
            code: "attachment_not_ready",
            message: "attachment is not ready for download".to_owned(),
        },
        DownloadError::MetadataMismatch | DownloadError::Internal => {
            tracing::error!(%error, "attachment download failed");
            ApiError::Internal
        }
        DownloadError::Repository(repository_error) => {
            tracing::error!(error = %repository_error, "attachment download repository failure");
            ApiError::Internal
        }
    }
}

#[cfg(test)]
mod tests {
    use axum::extract::Query;
    use axum::http::Uri;

    use super::{content_disposition, DownloadGrantQuery};

    #[test]
    fn query_extractor_accepts_one_token() {
        let query = download_query("/attachment-downloads?token=abc.def").unwrap();
        assert_eq!(
            query.token, "abc.def",
            "valid single token should deserialize"
        );
    }

    #[test]
    fn query_extractor_rejects_missing_or_duplicate_token() {
        assert!(download_query("/attachment-downloads").is_err());
        assert!(download_query("/attachment-downloads?other=value").is_err());
        assert!(download_query("/attachment-downloads?token=a&token=b").is_err());
    }

    #[test]
    fn query_extractor_allows_empty_token_for_handler_guard() {
        let query = download_query("/attachment-downloads?token=").unwrap();
        assert_eq!(query.token, "");
    }

    #[test]
    fn content_disposition_uses_validated_filename() {
        assert_eq!(
            content_disposition("Quarterly evidence (final).pdf"),
            "attachment; filename=\"Quarterly evidence (final).pdf\""
        );
    }

    fn download_query(
        uri: &str,
    ) -> Result<DownloadGrantQuery, axum::extract::rejection::QueryRejection> {
        let uri: Uri = uri.parse().expect("URI parses");
        Query::<DownloadGrantQuery>::try_from_uri(&uri).map(|Query(query)| query)
    }
}
