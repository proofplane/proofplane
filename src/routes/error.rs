use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use deadpool_postgres::PoolError;
use serde::Serialize;

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
    NotFound,
    ReadinessTimeout,
    Pool(PoolError),
    Postgres(tokio_postgres::Error),
}

impl ApiError {
    fn status(&self) -> StatusCode {
        match self {
            Self::NotFound => StatusCode::NOT_FOUND,
            Self::ReadinessTimeout | Self::Pool(_) | Self::Postgres(_) => {
                StatusCode::SERVICE_UNAVAILABLE
            }
        }
    }

    fn code(&self) -> &'static str {
        match self {
            Self::NotFound => "not_found",
            Self::ReadinessTimeout | Self::Pool(_) | Self::Postgres(_) => "not_ready",
        }
    }

    fn message(&self) -> &'static str {
        match self {
            Self::NotFound => "route not found",
            Self::ReadinessTimeout => "readiness check timed out",
            Self::Pool(_) | Self::Postgres(_) => "Postgres readiness check failed",
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = self.status();
        match &self {
            Self::Pool(error) => tracing::warn!(%error, "Postgres pool readiness check failed"),
            Self::Postgres(error) => tracing::warn!(%error, "Postgres readiness query failed"),
            Self::NotFound | Self::ReadinessTimeout => {}
        }
        let body = ErrorResponse {
            error: ErrorBody {
                code: self.code(),
                message: self.message(),
                details: Vec::new(),
            },
        };

        (status, Json(body)).into_response()
    }
}

pub async fn not_found() -> ApiError {
    ApiError::NotFound
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
