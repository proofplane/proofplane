use axum::{
    extract::Request,
    http::{header::HeaderName, HeaderValue},
    middleware::Next,
    response::Response,
};
use tracing::Span;
use uuid::Uuid;

use super::error::ApiError;

pub const REQUEST_ID_HEADER: HeaderName = HeaderName::from_static("x-request-id");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RequestId(pub Uuid);

pub async fn attach_request_id(mut request: Request, next: Next) -> Result<Response, ApiError> {
    let request_id = match request.headers().get(&REQUEST_ID_HEADER) {
        Some(value) => value
            .to_str()
            .ok()
            .and_then(|value| Uuid::parse_str(value).ok())
            .ok_or_else(invalid_request_id)?,
        None => Uuid::new_v4(),
    };

    request.extensions_mut().insert(RequestId(request_id));
    Span::current().record("request_id", request_id.to_string());
    let mut response = next.run(request).await;
    response.headers_mut().insert(
        REQUEST_ID_HEADER,
        HeaderValue::from_str(&request_id.to_string()).map_err(|_| ApiError::Internal)?,
    );

    Ok(response)
}

fn invalid_request_id() -> ApiError {
    ApiError::BadRequest(vec!["x-request-id must be a UUID".to_owned()])
}
