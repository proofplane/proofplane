use axum::{
    body::Body,
    extract::{Path, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::put,
    Extension, Json, Router,
};
use futures_util::StreamExt;
use serde::Serialize;
use uuid::Uuid;

use crate::{
    domain::{
        AgentEvidenceUploadGrantError as DomainGrantError, AgentEvidenceUploadGrantId,
        DocumentUploadStatus,
    },
    object_storage::StorageError,
    routes::{error::ApiError, request_context::RequestId},
    services::{
        agent_evidence_upload_grants::AgentEvidenceUploadGrantError,
        agent_evidence_uploads::{AgentEvidenceUploadError, AgentEvidenceUploadService},
        Error as ServiceError,
    },
};

const AUTHORIZATION_SCHEME: &str = "Proofplane-Upload ";

#[derive(Clone)]
pub struct AgentEvidenceUploadState {
    pub service: AgentEvidenceUploadService,
}

pub fn router(state: AgentEvidenceUploadState) -> Router {
    Router::new()
        .route("/agent-evidence-uploads/{upload_id}", put(upload))
        .with_state(state)
}

#[derive(Serialize)]
struct AgentEvidenceUploadResponse {
    submission_id: Uuid,
    document_id: Uuid,
    upload_status: &'static str,
}

async fn upload(
    State(state): State<AgentEvidenceUploadState>,
    Path(raw_upload_id): Path<String>,
    Extension(request_id): Extension<RequestId>,
    headers: HeaderMap,
    body: Body,
) -> Result<Response, ApiError> {
    let upload_id = Uuid::parse_str(&raw_upload_id)
        .map(AgentEvidenceUploadGrantId::from)
        .map_err(|_| unavailable())?;
    let credential = upload_credential(&headers).ok_or_else(unavailable)?;
    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| validation_error("content-type header is required"))?;
    let content_length = headers
        .get(header::CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        .ok_or_else(|| validation_error("valid content-length header is required"))?;
    let chunks = body.into_data_stream().map(|chunk| {
        chunk.map_err(|_| StorageError::StreamRead {
            message: "request body stream failed".to_owned(),
            payload_too_large: false,
        })
    });
    let result = state
        .service
        .upload(
            upload_id,
            credential,
            content_type,
            content_length,
            request_id.0,
            chunks,
        )
        .await
        .map_err(upload_error)?;
    let response = AgentEvidenceUploadResponse {
        submission_id: result.submission_id.into(),
        document_id: result.document.id().into(),
        upload_status: DocumentUploadStatus::PendingUpload.as_str(),
    };
    Ok((StatusCode::CREATED, Json(response)).into_response())
}

fn upload_credential(headers: &HeaderMap) -> Option<&str> {
    let value = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let credential = value.strip_prefix(AUTHORIZATION_SCHEME)?;
    if credential.is_empty() || credential.bytes().any(|byte| byte.is_ascii_whitespace()) {
        return None;
    }
    Some(credential)
}

fn upload_error(error: AgentEvidenceUploadError) -> ApiError {
    match error {
        AgentEvidenceUploadError::Unavailable
        | AgentEvidenceUploadError::GrantCredential(AgentEvidenceUploadGrantError::Unavailable)
        | AgentEvidenceUploadError::Grant(DomainGrantError::Expired)
        | AgentEvidenceUploadError::Grant(DomainGrantError::AlreadyCompleted)
        | AgentEvidenceUploadError::Grant(DomainGrantError::AuthorityMismatch) => unavailable(),
        AgentEvidenceUploadError::PayloadTooLarge
        | AgentEvidenceUploadError::Service(ServiceError::Storage(StorageError::StreamRead {
            payload_too_large: true,
            ..
        })) => ApiError::PayloadTooLarge,
        AgentEvidenceUploadError::Grant(DomainGrantError::ContentTypeMismatch) => {
            validation_error("content-type header does not match upload grant")
        }
        AgentEvidenceUploadError::Grant(DomainGrantError::DeclaredContentLengthMismatch) => {
            validation_error("content-length header does not match upload grant")
        }
        AgentEvidenceUploadError::Grant(DomainGrantError::ReceivedContentLengthMismatch) => {
            validation_error("request body length does not match upload grant")
        }
        AgentEvidenceUploadError::Grant(DomainGrantError::ChecksumMismatch) => {
            validation_error("request body checksum does not match upload grant")
        }
        AgentEvidenceUploadError::Service(ServiceError::Storage(StorageError::StreamRead {
            message,
            ..
        })) => ApiError::BadRequest(vec![message]),
        AgentEvidenceUploadError::Grant(_)
        | AgentEvidenceUploadError::GrantCredential(_)
        | AgentEvidenceUploadError::Service(_)
        | AgentEvidenceUploadError::Repository(_) => {
            tracing::error!(%error, "agent evidence upload dependency failure");
            ApiError::Internal
        }
    }
}

fn validation_error(message: &'static str) -> ApiError {
    ApiError::BadRequest(vec![message.to_owned()])
}

fn unavailable() -> ApiError {
    ApiError::NotFound
}
