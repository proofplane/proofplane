pub mod agent_connections;
pub mod agent_evidence_uploads;
pub mod agent_policy_document_uploads;
pub mod auditor_access;
pub mod authentication;
pub mod document_downloads;
pub mod document_upload_sessions;
pub mod error;
pub mod health;
pub mod me;
pub mod metrics;
pub mod oauth;
pub mod policy_document_upload_sessions;
pub mod protected_resource_metadata;
pub mod request_context;
pub mod version;
pub mod workspaces;

pub(crate) fn upload_credential(headers: &axum::http::HeaderMap) -> Option<&str> {
    const AUTHORIZATION_SCHEME: &str = "Proofplane-Upload ";

    let value = headers
        .get(axum::http::header::AUTHORIZATION)?
        .to_str()
        .ok()?;
    let credential = value.strip_prefix(AUTHORIZATION_SCHEME)?;
    if credential.is_empty() || credential.bytes().any(|byte| byte.is_ascii_whitespace()) {
        return None;
    }
    Some(credential)
}

pub(crate) fn limited_body(body: axum::body::Body, max_bytes: usize) -> axum::body::Body {
    axum::body::Body::new(http_body_util::Limited::new(body, max_bytes))
}

pub(crate) fn request_body_stream_error(error: axum::Error) -> crate::object_storage::StorageError {
    let payload_too_large = error.into_inner().is::<http_body_util::LengthLimitError>();
    crate::object_storage::StorageError::StreamRead {
        message: if payload_too_large {
            "request payload is too large"
        } else {
            "request body stream failed"
        }
        .to_owned(),
        payload_too_large,
    }
}
