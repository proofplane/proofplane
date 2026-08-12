use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use deadpool_postgres::PoolError;
use serde::Serialize;

use crate::{
    application::commands::{
        create_owned_workspace::CreateOwnedWorkspaceError,
        remove_workspace_member::RemoveWorkspaceMemberError,
    },
    domain::DomainError,
    object_storage::StorageError,
    persistence::{ConflictKind, Error as RepositoryError},
    services::Error as ServiceError,
};

#[derive(Debug, Serialize)]
struct ErrorResponse {
    error: ErrorResponseDTO,
}

#[derive(Debug, Serialize)]
struct ErrorResponseDTO {
    code: &'static str,
    message: String,
    details: Vec<String>,
}

#[derive(Debug)]
pub enum ApiError {
    BadRequest(Vec<String>),
    Internal,
    MethodNotAllowed,
    NotFound,
    PayloadTooLarge,
    Conflict { code: &'static str, message: String },
    Forbidden { code: &'static str, message: String },
    Gone { code: &'static str, message: String },
    ServiceUnavailable { code: &'static str, message: String },
    ReadinessTimeout,
    Unauthorized,
    Pool(PoolError),
    Postgres(tokio_postgres::Error),
}

impl ApiError {
    fn status(&self) -> StatusCode {
        match self {
            Self::BadRequest(_) => StatusCode::BAD_REQUEST,
            Self::Internal => StatusCode::INTERNAL_SERVER_ERROR,
            Self::MethodNotAllowed => StatusCode::METHOD_NOT_ALLOWED,
            Self::NotFound => StatusCode::NOT_FOUND,
            Self::PayloadTooLarge => StatusCode::PAYLOAD_TOO_LARGE,
            Self::Conflict { .. } => StatusCode::CONFLICT,
            Self::Forbidden { .. } => StatusCode::FORBIDDEN,
            Self::Gone { .. } => StatusCode::GONE,
            Self::ServiceUnavailable { .. } => StatusCode::SERVICE_UNAVAILABLE,
            Self::Unauthorized => StatusCode::UNAUTHORIZED,
            Self::ReadinessTimeout | Self::Pool(_) | Self::Postgres(_) => {
                StatusCode::SERVICE_UNAVAILABLE
            }
        }
    }

    fn code(&self) -> &'static str {
        match self {
            Self::BadRequest(_) => "bad_request",
            Self::Internal => "internal_error",
            Self::MethodNotAllowed => "method_not_allowed",
            Self::NotFound => "not_found",
            Self::PayloadTooLarge => "payload_too_large",
            Self::Conflict { code, .. }
            | Self::Forbidden { code, .. }
            | Self::Gone { code, .. } => code,
            Self::ServiceUnavailable { code, .. } => code,
            Self::Unauthorized => "unauthorized",
            Self::ReadinessTimeout | Self::Pool(_) | Self::Postgres(_) => "not_ready",
        }
    }

    fn message(&self) -> String {
        match self {
            Self::BadRequest(_) => "request validation failed".to_owned(),
            Self::Internal => "internal server error".to_owned(),
            Self::MethodNotAllowed => "method not allowed".to_owned(),
            Self::NotFound => "route not found".to_owned(),
            Self::PayloadTooLarge => "request payload is too large".to_owned(),
            Self::Conflict { message, .. }
            | Self::Forbidden { message, .. }
            | Self::Gone { message, .. } => message.clone(),
            Self::ServiceUnavailable { message, .. } => message.clone(),
            Self::Unauthorized => "authentication required".to_owned(),
            Self::ReadinessTimeout => "readiness check timed out".to_owned(),
            Self::Pool(_) | Self::Postgres(_) => "Postgres readiness check failed".to_owned(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = self.status();
        let code = self.code();
        let message = self.message();
        match &self {
            Self::Internal => tracing::error!("internal API error"),
            Self::BadRequest(_) => {}
            Self::Pool(error) => tracing::warn!(%error, "Postgres pool readiness check failed"),
            Self::Postgres(error) => tracing::warn!(%error, "Postgres readiness query failed"),
            Self::MethodNotAllowed
            | Self::NotFound
            | Self::PayloadTooLarge
            | Self::Conflict { .. }
            | Self::Forbidden { .. }
            | Self::Gone { .. }
            | Self::ServiceUnavailable { .. }
            | Self::ReadinessTimeout
            | Self::Unauthorized => {}
        }
        let details = match self {
            Self::BadRequest(details) => details,
            _ => Vec::new(),
        };
        let body = ErrorResponse {
            error: ErrorResponseDTO {
                code,
                message,
                details,
            },
        };

        (status, Json(body)).into_response()
    }
}

pub async fn not_found() -> ApiError {
    ApiError::NotFound
}

impl From<ServiceError> for ApiError {
    fn from(error: ServiceError) -> Self {
        match error {
            ServiceError::Repository(error) => repository_error(error),
            ServiceError::Storage(error) => storage_error(error),
        }
    }
}

impl From<RepositoryError> for ApiError {
    fn from(error: RepositoryError) -> Self {
        repository_error(error)
    }
}

impl From<RemoveWorkspaceMemberError> for ApiError {
    fn from(error: RemoveWorkspaceMemberError) -> Self {
        match error {
            RemoveWorkspaceMemberError::Unavailable => ApiError::NotFound,
            RemoveWorkspaceMemberError::NotFound => ApiError::NotFound,
            RemoveWorkspaceMemberError::LastOwner => ApiError::Conflict {
                code: "last_owner",
                message: "the workspace must retain at least one owner".to_owned(),
            },
            RemoveWorkspaceMemberError::SelfRemoval => ApiError::Conflict {
                code: "self_removal",
                message: "workspace members may not remove themselves".to_owned(),
            },
            RemoveWorkspaceMemberError::Repository(error) => repository_error(error),
        }
    }
}

impl From<CreateOwnedWorkspaceError> for ApiError {
    fn from(error: CreateOwnedWorkspaceError) -> Self {
        match error {
            CreateOwnedWorkspaceError::SlugTaken => conflict(ConflictKind::WorkspaceSlugTaken),
            CreateOwnedWorkspaceError::UserAlreadyHasWorkspace => {
                conflict(ConflictKind::WorkspaceMembershipExists)
            }
            CreateOwnedWorkspaceError::Repository(error) => repository_error(error),
        }
    }
}

pub fn domain_errors(errors: Vec<DomainError>) -> ApiError {
    ApiError::BadRequest(errors.into_iter().map(|error| error.to_string()).collect())
}

fn storage_error(error: StorageError) -> ApiError {
    match error {
        StorageError::StreamRead {
            payload_too_large: true,
            ..
        } => ApiError::PayloadTooLarge,
        StorageError::StreamRead { message, .. } => ApiError::BadRequest(vec![message]),
        other => {
            tracing::error!(%other, "object storage error");
            ApiError::Internal
        }
    }
}

fn repository_error(error: RepositoryError) -> ApiError {
    if let RepositoryError::Conflict(kind) = error {
        return conflict(kind);
    }

    tracing::error!(%error, "repository error");
    ApiError::Internal
}

fn conflict(kind: ConflictKind) -> ApiError {
    ApiError::Conflict {
        code: kind.code(),
        message: kind.message().to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use axum::{http::StatusCode, response::IntoResponse};

    use super::ApiError;
    use crate::application::commands::create_owned_workspace::CreateOwnedWorkspaceError;

    #[test]
    fn error_response_uses_stable_shape() {
        let response = ApiError::NotFound.into_response();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[test]
    fn slug_conflict_maps_to_409_with_specific_code_and_message() {
        let error = ApiError::from(CreateOwnedWorkspaceError::SlugTaken);

        assert_eq!(error.status(), StatusCode::CONFLICT);
        assert_eq!(error.code(), "slug_taken");
        assert_eq!(error.message(), "a workspace with this slug already exists");
    }
}
