use axum::{
    body::Body,
    extract::{rejection::QueryRejection, Query, State},
    http::{
        header::{CACHE_CONTROL, CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_TYPE},
        HeaderName, HeaderValue, StatusCode,
    },
    response::{IntoResponse, Response},
    routing::get,
    Extension, Router,
};
use serde::Deserialize;
use uuid::Uuid;

use crate::{
    observability::audit::{AuditActor, AuditClientType, AuditEvent, AuditObject, AuditOutcome},
    routes::{error::ApiError, request_context::RequestId},
    services::document_downloads::{DocumentDownloadService, DownloadError},
};

const REFERRER_POLICY: HeaderName = HeaderName::from_static("referrer-policy");

#[derive(Clone)]
pub struct DocumentDownloadState {
    pub service: DocumentDownloadService,
}

pub fn router(state: DocumentDownloadState) -> Router {
    Router::new()
        .route("/document-downloads", get(redeem_download_grant))
        .with_state(state)
}

#[derive(Debug, Deserialize)]
struct DownloadGrantQuery {
    token: String,
}

async fn redeem_download_grant(
    State(state): State<DocumentDownloadState>,
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
        "evidence_document_download_grant.redeemed",
        AuditOutcome::Success,
        download_audit_actor(
            downloaded.audit.issued_by_user_id,
            downloaded.audit.issued_via,
        ),
        AuditClientType::Rest,
        "redeem_document_download_grant",
    )
    .workspace_id(downloaded.audit.workspace_id.into())
    .request_id(request_id.0)
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
    let disposition = content_disposition(&downloaded.document.filename);
    let mut response = Body::from_stream(downloaded.object.chunks).into_response();
    *response.status_mut() = StatusCode::OK;
    let headers = response.headers_mut();
    headers.insert(
        CONTENT_TYPE,
        HeaderValue::from_str(&downloaded.document.content_type).map_err(|_| ApiError::Internal)?,
    );
    headers.insert(
        CONTENT_LENGTH,
        HeaderValue::from_str(&downloaded.document.content_length.to_string())
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

pub(crate) fn content_disposition(filename: &str) -> String {
    format!("document; filename=\"{filename}\"")
}

fn download_audit_actor(
    user_id: crate::domain::UserId,
    issued_via: crate::services::document_downloads::DownloadGrantIssuer,
) -> AuditActor {
    AuditActor::AgentConnection {
        user_id: user_id.into(),
        agent_connection_id: issued_via.agent_connection_id().into(),
    }
}

fn download_error(error: DownloadError) -> ApiError {
    match error {
        DownloadError::NotFound => ApiError::NotFound,
        DownloadError::NotReady => ApiError::Conflict {
            code: "document_not_ready",
            message: "document is not ready for download".to_owned(),
        },
        DownloadError::MetadataMismatch | DownloadError::Internal => {
            tracing::error!(%error, "document download failed");
            ApiError::Internal
        }
        DownloadError::Repository(repository_error) => {
            tracing::error!(error = %repository_error, "document download repository failure");
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
        let query = download_query("/document-downloads?token=abc.def").unwrap();
        assert_eq!(
            query.token, "abc.def",
            "valid single token should deserialize"
        );
    }

    #[test]
    fn query_extractor_rejects_missing_or_duplicate_token() {
        assert!(download_query("/document-downloads").is_err());
        assert!(download_query("/document-downloads?other=value").is_err());
        assert!(download_query("/document-downloads?token=a&token=b").is_err());
    }

    #[test]
    fn query_extractor_allows_empty_token_for_handler_guard() {
        let query = download_query("/document-downloads?token=").unwrap();
        assert_eq!(query.token, "");
    }

    #[test]
    fn content_disposition_uses_validated_filename() {
        assert_eq!(
            content_disposition("Quarterly evidence (final).pdf"),
            "document; filename=\"Quarterly evidence (final).pdf\""
        );
    }

    fn download_query(
        uri: &str,
    ) -> Result<DownloadGrantQuery, axum::extract::rejection::QueryRejection> {
        let uri: Uri = uri.parse().expect("URI parses");
        Query::<DownloadGrantQuery>::try_from_uri(&uri).map(|Query(query)| query)
    }
}
