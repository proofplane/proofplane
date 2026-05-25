use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use deadpool_postgres::PoolError;
use serde::Serialize;

use crate::{
    domain::DomainError, repository::Error as RepositoryError, services::Error as ServiceError,
};

#[derive(Debug, Serialize)]
struct ErrorResponse {
    error: ErrorBody,
}

#[derive(Debug, Serialize)]
struct ErrorBody {
    code: &'static str,
    message: &'static str,
    details: Vec<String>,
}

#[derive(Debug)]
pub enum ApiError {
    BadRequest(Vec<String>),
    Internal,
    MethodNotAllowed,
    NotFound,
    Conflict,
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
            Self::Conflict => StatusCode::CONFLICT,
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
            Self::Conflict => "conflict",
            Self::Unauthorized => "unauthorized",
            Self::ReadinessTimeout | Self::Pool(_) | Self::Postgres(_) => "not_ready",
        }
    }

    fn message(&self) -> &'static str {
        match self {
            Self::BadRequest(_) => "request validation failed",
            Self::Internal => "internal server error",
            Self::MethodNotAllowed => "method not allowed",
            Self::NotFound => "route not found",
            Self::Conflict => "resource conflict",
            Self::Unauthorized => "authentication required",
            Self::ReadinessTimeout => "readiness check timed out",
            Self::Pool(_) | Self::Postgres(_) => "Postgres readiness check failed",
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = self.status();
        match &self {
            Self::Internal => tracing::error!("internal API error"),
            Self::BadRequest(_) => {}
            Self::Pool(error) => tracing::warn!(%error, "Postgres pool readiness check failed"),
            Self::Postgres(error) => tracing::warn!(%error, "Postgres readiness query failed"),
            Self::MethodNotAllowed
            | Self::NotFound
            | Self::Conflict
            | Self::ReadinessTimeout
            | Self::Unauthorized => {}
        }
        let details = match &self {
            Self::BadRequest(details) => details.clone(),
            _ => Vec::new(),
        };
        let body = ErrorResponse {
            error: ErrorBody {
                code: self.code(),
                message: self.message(),
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
        }
    }
}

pub fn domain_errors(errors: Vec<DomainError>) -> ApiError {
    ApiError::BadRequest(errors.into_iter().map(|error| error.to_string()).collect())
}

fn repository_error(error: RepositoryError) -> ApiError {
    if let RepositoryError::Conflict(_) = error {
        return ApiError::Conflict;
    }

    tracing::error!(%error, "repository error");
    ApiError::Internal
}

#[cfg(test)]
mod tests {
    use axum::{http::StatusCode, response::IntoResponse};

    use super::ApiError;

    #[test]
    fn error_response_uses_stable_shape() {
        let response = ApiError::NotFound.into_response();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
}
