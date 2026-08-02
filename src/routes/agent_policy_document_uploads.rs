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
        AgentPolicyDocumentUploadGrantError as DomainGrantError, AgentPolicyDocumentUploadGrantId,
    },
    object_storage::StorageError,
    observability::agent_policy_document_uploads::{
        record_attempt, AgentPolicyDocumentUploadAttemptResult,
    },
    routes::{
        error::ApiError, limited_body, request_body_stream_error, request_context::RequestId,
        upload_credential,
    },
    services::{
        agent_policy_document_upload_grants::AgentPolicyDocumentUploadGrantError,
        agent_policy_document_uploads::{
            AgentPolicyDocumentUploadError, AgentPolicyDocumentUploadOutcome,
            AgentPolicyDocumentUploadService,
        },
        Error as ServiceError,
    },
};

#[derive(Clone)]
pub struct AgentPolicyDocumentUploadState {
    pub service: AgentPolicyDocumentUploadService,
    pub max_document_bytes: usize,
}

pub fn router(state: AgentPolicyDocumentUploadState) -> Router {
    Router::new()
        .route("/agent-policy-document-uploads/{upload_id}", put(upload))
        .with_state(state)
}

#[derive(Serialize)]
struct AgentPolicyDocumentUploadResponse {
    policy_id: Uuid,
    document_id: Uuid,
    upload_status: &'static str,
}

async fn upload(
    State(state): State<AgentPolicyDocumentUploadState>,
    Path(raw_upload_id): Path<String>,
    Extension(request_id): Extension<RequestId>,
    headers: HeaderMap,
    body: Body,
) -> Result<Response, ApiError> {
    let upload_id = Uuid::parse_str(&raw_upload_id)
        .map(AgentPolicyDocumentUploadGrantId::from)
        .map_err(|_| unavailable_attempt())?;
    let credential = upload_credential(&headers).ok_or_else(unavailable_attempt)?;
    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| validation_attempt("content-type header is required"))?;
    let content_length = headers
        .get(header::CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        .ok_or_else(|| validation_attempt("valid content-length header is required"))?;
    let chunks = limited_body(body, state.max_document_bytes)
        .into_data_stream()
        .map(|chunk| chunk.map_err(request_body_stream_error));
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
        .await?;
    let status = match &result {
        AgentPolicyDocumentUploadOutcome::Created(_) => StatusCode::CREATED,
        AgentPolicyDocumentUploadOutcome::Replayed(_) => StatusCode::OK,
    };
    let result = result.result();
    Ok((
        status,
        Json(AgentPolicyDocumentUploadResponse {
            policy_id: result.policy_id.into(),
            document_id: result.document.id().into(),
            upload_status: result.document.upload_status.as_str(),
        }),
    )
        .into_response())
}

impl From<AgentPolicyDocumentUploadError> for ApiError {
    fn from(error: AgentPolicyDocumentUploadError) -> Self {
        match error {
            AgentPolicyDocumentUploadError::Unavailable
            | AgentPolicyDocumentUploadError::GrantCredential(
                AgentPolicyDocumentUploadGrantError::Unavailable,
            )
            | AgentPolicyDocumentUploadError::Grant(DomainGrantError::Expired)
            | AgentPolicyDocumentUploadError::Grant(DomainGrantError::AlreadyCompleted)
            | AgentPolicyDocumentUploadError::Grant(DomainGrantError::AuthorityMismatch) => {
                unavailable()
            }
            AgentPolicyDocumentUploadError::CurrentDocument
            | AgentPolicyDocumentUploadError::GrantCredential(
                AgentPolicyDocumentUploadGrantError::CurrentDocument,
            ) => current_document_conflict(),
            AgentPolicyDocumentUploadError::PayloadTooLarge
            | AgentPolicyDocumentUploadError::Service(ServiceError::Storage(
                StorageError::StreamRead {
                    payload_too_large: true,
                    ..
                },
            )) => ApiError::PayloadTooLarge,
            AgentPolicyDocumentUploadError::Grant(DomainGrantError::ContentTypeMismatch) => {
                validation_error("content-type header does not match upload grant")
            }
            AgentPolicyDocumentUploadError::Grant(
                DomainGrantError::DeclaredContentLengthMismatch,
            ) => validation_error("content-length header does not match upload grant"),
            AgentPolicyDocumentUploadError::Grant(
                DomainGrantError::ReceivedContentLengthMismatch,
            ) => validation_error("request body length does not match upload grant"),
            AgentPolicyDocumentUploadError::Grant(DomainGrantError::ChecksumMismatch) => {
                validation_error("request body checksum does not match upload grant")
            }
            AgentPolicyDocumentUploadError::Service(ServiceError::Storage(
                StorageError::StreamRead { message, .. },
            )) => ApiError::BadRequest(vec![message]),
            AgentPolicyDocumentUploadError::Grant(_)
            | AgentPolicyDocumentUploadError::GrantCredential(_)
            | AgentPolicyDocumentUploadError::Service(_)
            | AgentPolicyDocumentUploadError::Repository(_) => {
                tracing::error!(%error, "agent policy document upload dependency failure");
                ApiError::Internal
            }
        }
    }
}

fn validation_error(message: &'static str) -> ApiError {
    ApiError::BadRequest(vec![message.to_owned()])
}

fn unavailable() -> ApiError {
    ApiError::NotFound
}

fn unavailable_attempt() -> ApiError {
    record_attempt(AgentPolicyDocumentUploadAttemptResult::Unavailable);
    unavailable()
}

fn validation_attempt(message: &'static str) -> ApiError {
    record_attempt(AgentPolicyDocumentUploadAttemptResult::ValidationRejected);
    validation_error(message)
}

fn current_document_conflict() -> ApiError {
    ApiError::Conflict {
        code: "policy_document_exists",
        message: "this policy already has a current document".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upload_credential_accepts_the_machine_scheme() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            "Proofplane-Upload opaque-token".parse().unwrap(),
        );

        assert_eq!(
            crate::routes::upload_credential(&headers),
            Some("opaque-token")
        );
    }
}
