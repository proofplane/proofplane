use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicU32, Ordering},
        Arc,
    },
};

use axum::{
    extract::{
        multipart::{Field, MultipartError},
        DefaultBodyLimit, Multipart, Path, Request, State,
    },
    http::{Method, StatusCode},
    middleware,
    middleware::Next,
    response::Response,
    routing::{get, post},
    Extension, Json, Router,
};
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use chrono::{DateTime, Utc};
use futures_core::Stream;
use futures_util::stream;
use serde::{Deserialize, Serialize};
use sfv::{BareItem, Dictionary, ListEntry, Parser};
use uuid::Uuid;

use crate::{
    authentication::ApiTokenAuthenticator,
    authentication::ApiTokenContext,
    domain::{
        optional_text, required_text, validate_attachment_filename,
        CreateEvidenceSubmissionPayload, DomainError, EvidenceAttachment, EvidenceRequestId,
        EvidenceSubmission, EvidenceSubmissionDetail, EvidenceSubmissionId, WorkspacePermission,
    },
    object_storage::StorageError,
    routes::{
        authentication::authorize_workspace_route,
        error::{domain_errors, ApiError},
        request_context::RequestId,
    },
    services::evidence_submissions::{EvidenceSubmissionService, UploadEvidenceAttachmentPayload},
    validate,
    validation::Validation,
};

#[derive(Clone)]
pub struct EvidenceSubmissionState {
    pub service: EvidenceSubmissionService,
    pub route_auth: EvidenceSubmissionRouteAuthState,
    pub max_attachment_bytes: usize,
}

#[derive(Clone)]
pub struct EvidenceSubmissionRouteAuthState {
    pub authenticator: ApiTokenAuthenticator,
}

pub fn router(state: EvidenceSubmissionState) -> Router {
    let route_auth = state.route_auth.clone();
    let max_attachment_bytes = state.max_attachment_bytes;

    Router::new()
        .route(
            "/workspaces/{workspace_id}/evidence-requests/{evidence_request_id}/submissions",
            post(create_evidence_submission),
        )
        .route(
            "/workspaces/{workspace_id}/evidence-requests/{evidence_request_id}/submissions/latest",
            get(get_latest_evidence_submission),
        )
        .route(
            "/workspaces/{workspace_id}/evidence-submissions/{submission_id}",
            get(get_evidence_submission),
        )
        .route(
            "/workspaces/{workspace_id}/evidence-submissions/{submission_id}/attachments",
            post(upload_evidence_attachment).layer(DefaultBodyLimit::max(max_attachment_bytes)),
        )
        .route_layer(middleware::from_fn_with_state(
            route_auth,
            authorize_evidence_submission_route,
        ))
        .with_state(state)
}

async fn authorize_evidence_submission_route(
    State(state): State<EvidenceSubmissionRouteAuthState>,
    Path(path): Path<HashMap<String, String>>,
    mut request: Request,
    next: Next,
) -> Result<Response, ApiError> {
    let method = request.method().clone();
    let token = authorize_workspace_route(&state.authenticator, &path, &mut request).await?;

    let required = match method {
        Method::GET => WorkspacePermission::ReadEvidenceSubmissions,
        Method::POST => WorkspacePermission::WriteEvidenceSubmissions,
        _ => return Err(ApiError::MethodNotAllowed),
    };

    if !token.permissions.has(required) {
        return Err(ApiError::NotFound);
    }

    Ok(next.run(request).await)
}

#[derive(Debug, Deserialize)]
struct EvidenceSubmissionDTO {
    coverage_start_at: DateTime<Utc>,
    coverage_end_at: DateTime<Utc>,
    source_system: String,
    collection_method: String,
    summary: Option<String>,
    description: Option<String>,
}

impl EvidenceSubmissionDTO {
    fn into_new(
        self,
        evidence_request_id: EvidenceRequestId,
    ) -> Validation<CreateEvidenceSubmissionPayload, DomainError> {
        let coverage_start_at = self.coverage_start_at;
        let coverage_end_at = self.coverage_end_at;

        validate! {
            source_system <- required_text("source_system", self.source_system),
            collection_method <- required_text("collection_method", self.collection_method),
            summary <- optional_text("summary", self.summary, 500),
            description <- optional_text("description", self.description, 4_000),
            coverage_window <- validate_coverage_window(coverage_start_at, coverage_end_at),
            => CreateEvidenceSubmissionPayload {
                evidence_request_id,
                coverage_start_at: coverage_window.0,
                coverage_end_at: coverage_window.1,
                source_system,
                collection_method,
                summary,
                description,
            },
        }
    }
}

#[derive(Debug, Deserialize)]
struct EvidenceRequestSubmissionsPath {
    evidence_request_id: Uuid,
}

#[derive(Debug, Deserialize)]
struct EvidenceSubmissionPath {
    submission_id: Uuid,
}

#[derive(Debug, Deserialize)]
struct EvidenceSubmissionAttachmentPath {
    submission_id: Uuid,
}

struct AttachmentUploadRequest {
    filename: String,
    content_type: String,
}

impl AttachmentUploadRequest {
    fn validate(self) -> Validation<Self, DomainError> {
        validate! {
            filename <- validate_attachment_filename(self.filename),
            => Self {
                filename,
                content_type: self.content_type,
            },
        }
    }
}

#[derive(Debug, Serialize)]
struct EvidenceSubmissionResponseDTO {
    id: Uuid,
    evidence_request_id: Uuid,
    submitted_by: EvidenceSubmitterResponse,
    received_at: DateTime<Utc>,
    coverage_start_at: DateTime<Utc>,
    coverage_end_at: DateTime<Utc>,
    source_system: String,
    collection_method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
}

#[derive(Debug, Serialize)]
struct EvidenceSubmitterResponse {
    api_token_id: Uuid,
    user_id: Uuid,
}

impl From<EvidenceSubmission> for EvidenceSubmissionResponseDTO {
    fn from(submission: EvidenceSubmission) -> Self {
        Self {
            id: Uuid::from(submission.id),
            evidence_request_id: Uuid::from(submission.evidence_request_id),
            submitted_by: EvidenceSubmitterResponse {
                api_token_id: Uuid::from(submission.submitted_by.api_token_id),
                user_id: Uuid::from(submission.submitted_by.user_id),
            },
            received_at: submission.received_at,
            coverage_start_at: submission.coverage_start_at,
            coverage_end_at: submission.coverage_end_at,
            source_system: submission.source_system,
            collection_method: submission.collection_method,
            summary: submission.summary,
            description: submission.description,
        }
    }
}

#[derive(Debug, Serialize)]
struct EvidenceSubmissionSummaryResponseDTO {
    id: Uuid,
    evidence_request_id: Uuid,
    submitted_by: EvidenceSubmitterResponse,
    coverage_start_at: DateTime<Utc>,
    coverage_end_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    summary: Option<String>,
}

type CreateEvidenceSubmissionResponse = EvidenceSubmissionSummaryResponseDTO;

impl From<EvidenceSubmission> for EvidenceSubmissionSummaryResponseDTO {
    fn from(submission: EvidenceSubmission) -> Self {
        Self {
            id: Uuid::from(submission.id),
            evidence_request_id: Uuid::from(submission.evidence_request_id),
            submitted_by: EvidenceSubmitterResponse {
                api_token_id: Uuid::from(submission.submitted_by.api_token_id),
                user_id: Uuid::from(submission.submitted_by.user_id),
            },
            coverage_start_at: submission.coverage_start_at,
            coverage_end_at: submission.coverage_end_at,
            summary: submission.summary,
        }
    }
}

#[derive(Debug, Serialize)]
struct EvidenceAttachmentResponseDTO {
    id: Uuid,
    evidence_submission_id: Uuid,
    filename: String,
    content_type: String,
    content_length: i64,
    checksum_sha256: String,
    checksum_crc32c: String,
    upload_status: &'static str,
}

impl From<EvidenceAttachment> for EvidenceAttachmentResponseDTO {
    fn from(attachment: EvidenceAttachment) -> Self {
        Self {
            id: Uuid::from(attachment.id),
            evidence_submission_id: Uuid::from(attachment.evidence_submission_id),
            filename: attachment.filename,
            content_type: attachment.content_type,
            content_length: attachment.content_length,
            checksum_sha256: attachment.checksum_sha256,
            checksum_crc32c: attachment.checksum_crc32c,
            upload_status: attachment.upload_status.as_str(),
        }
    }
}

#[derive(Debug, Serialize)]
struct EvidenceSubmissionSummaryResponse {
    submission: EvidenceSubmissionSummaryResponseDTO,
    attachments: Vec<EvidenceAttachmentResponseDTO>,
}

impl From<EvidenceSubmissionDetail> for EvidenceSubmissionSummaryResponse {
    fn from(detail: EvidenceSubmissionDetail) -> Self {
        Self {
            submission: detail.submission.into(),
            attachments: detail.attachments.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Debug, Serialize)]
struct EvidenceSubmissionResponse {
    submission: EvidenceSubmissionResponseDTO,
    attachments: Vec<EvidenceAttachmentResponseDTO>,
}

impl From<EvidenceSubmissionDetail> for EvidenceSubmissionResponse {
    fn from(detail: EvidenceSubmissionDetail) -> Self {
        Self {
            submission: detail.submission.into(),
            attachments: detail.attachments.into_iter().map(Into::into).collect(),
        }
    }
}

async fn create_evidence_submission(
    State(state): State<EvidenceSubmissionState>,
    Path(path): Path<EvidenceRequestSubmissionsPath>,
    Extension(token): Extension<ApiTokenContext>,
    Json(body): Json<EvidenceSubmissionDTO>,
) -> Result<Json<CreateEvidenceSubmissionResponse>, ApiError> {
    let evidence_request_id = EvidenceRequestId::from(path.evidence_request_id);
    let payload = body
        .into_new(evidence_request_id)
        .into_result()
        .map_err(domain_errors)?;
    let submission = state
        .service
        .create(token, evidence_request_id, payload)
        .await?
        .ok_or(ApiError::NotFound)?;

    Ok(Json(submission.into()))
}

async fn get_evidence_submission(
    State(state): State<EvidenceSubmissionState>,
    Path(path): Path<EvidenceSubmissionPath>,
    Extension(token): Extension<ApiTokenContext>,
) -> Result<Json<EvidenceSubmissionResponse>, ApiError> {
    let detail = state
        .service
        .get(token, EvidenceSubmissionId::from(path.submission_id))
        .await?
        .ok_or(ApiError::NotFound)?;

    Ok(Json(detail.into()))
}

async fn get_latest_evidence_submission(
    State(state): State<EvidenceSubmissionState>,
    Path(path): Path<EvidenceRequestSubmissionsPath>,
    Extension(token): Extension<ApiTokenContext>,
) -> Result<Json<EvidenceSubmissionSummaryResponse>, ApiError> {
    let detail = state
        .service
        .latest_for_request(token, EvidenceRequestId::from(path.evidence_request_id))
        .await?
        .ok_or(ApiError::NotFound)?;

    Ok(Json(detail.into()))
}

#[derive(Debug, Serialize)]
struct EvidenceAttachmentUploadResponse {
    attachment: EvidenceAttachmentResponseDTO,
}

impl From<EvidenceAttachment> for EvidenceAttachmentUploadResponse {
    fn from(value: EvidenceAttachment) -> Self {
        Self {
            attachment: value.into(),
        }
    }
}

async fn upload_evidence_attachment(
    State(state): State<EvidenceSubmissionState>,
    Path(path): Path<EvidenceSubmissionAttachmentPath>,
    Extension(token): Extension<ApiTokenContext>,
    Extension(request_id): Extension<RequestId>,
    multipart: Multipart,
) -> Result<(StatusCode, Json<EvidenceAttachmentUploadResponse>), ApiError> {
    let submission_id = EvidenceSubmissionId::from(path.submission_id);
    if !state
        .service
        .evidence_submission_exists(&token, submission_id)
        .await?
    {
        return Err(ApiError::NotFound);
    }

    // Uploading the attachment happens before the database record for the attachment is stored
    // because if the database record fails to be created, the orphaned attachment can be
    // automatically cleaned up with object storage retention settings since it's in a quarantine
    // bucket and that will probably have a cleanup policy.
    let payload =
        attachment_upload_from_multipart(&state.service, &token, submission_id, multipart).await?;
    let attachment = state
        .service
        .create_attachment(&token, request_id.0, submission_id, payload)
        .await?;

    Ok((StatusCode::ACCEPTED, Json(attachment.into())))
}

async fn attachment_upload_from_multipart(
    service: &EvidenceSubmissionService,
    token: &ApiTokenContext,
    evidence_submission_id: EvidenceSubmissionId,
    mut multipart: Multipart,
) -> Result<UploadEvidenceAttachmentPayload, ApiError> {
    let field = multipart
        .next_field()
        .await
        .map_err(multipart_error)?
        .ok_or(ApiError::BadRequest(vec![
            "multipart upload requires at least one field".to_owned(),
        ]))?;

    let field_name = field.name().ok_or(ApiError::BadRequest(vec![
        "multipart upload field must be named".to_owned(),
    ]))?;

    if field_name != "file" {
        return Err(ApiError::BadRequest(vec![
            "multipart upload field for file must have correct name".to_owned(),
        ]));
    }

    let filename = field
        .file_name()
        .map(str::to_owned)
        .ok_or_else(|| ApiError::BadRequest(vec!["file filename is required".to_owned()]))?;
    let content_type = field
        .content_type()
        .map(str::to_owned)
        .unwrap_or_else(|| "application/octet-stream".to_owned());
    let request = AttachmentUploadRequest {
        filename,
        content_type,
    }
    .validate()
    .into_result()
    .map_err(domain_errors)?;
    let expected_crc32c = field_content_digest_crc32c(&field)
        .map_err(|message| ApiError::BadRequest(vec![message]))?;

    let crc32c = Arc::new(AtomicU32::new(0));
    let chunks = file_chunks(field, Arc::clone(&crc32c));
    let mut uploaded_file = service
        .upload_attachment(
            token,
            evidence_submission_id,
            request.filename,
            request.content_type,
            chunks,
        )
        .await?;

    let actual_crc32c = crc32c.load(Ordering::Relaxed);

    if let Err(message) = validate_attachment_upload(expected_crc32c, actual_crc32c) {
        maybe_delete_uploaded_file(service, uploaded_file.object_key).await;
        return Err(ApiError::BadRequest(vec![message]));
    }

    uploaded_file.checksum_crc32c = encode_crc32c_base64(actual_crc32c);
    Ok(uploaded_file)
}

fn multipart_error(error: MultipartError) -> ApiError {
    if error.status() == StatusCode::PAYLOAD_TOO_LARGE {
        return ApiError::PayloadTooLarge;
    }

    ApiError::BadRequest(vec![format!(
        "invalid multipart body: {}",
        error.body_text()
    )])
}

fn file_chunks(
    field: Field<'_>,
    crc32c: Arc<AtomicU32>,
) -> impl Stream<Item = Result<bytes::Bytes, StorageError>> + Send + '_ {
    stream::try_unfold(field, move |mut field| {
        let crc32c = Arc::clone(&crc32c);
        async move {
            match field.chunk().await.map_err(multipart_stream_error)? {
                Some(chunk) => {
                    let current = crc32c.load(Ordering::Relaxed);
                    crc32c.store(crc32c::crc32c_append(current, &chunk), Ordering::Relaxed);
                    Ok(Some((chunk, field)))
                }
                None => Ok(None),
            }
        }
    })
}

fn multipart_stream_error(error: MultipartError) -> StorageError {
    StorageError::StreamRead {
        payload_too_large: error.status() == StatusCode::PAYLOAD_TOO_LARGE,
        message: if error.status() == StatusCode::PAYLOAD_TOO_LARGE {
            "request payload is too large".to_owned()
        } else {
            format!("invalid multipart body: {}", error.body_text())
        },
    }
}

fn validate_attachment_upload(expected_crc32c: u32, actual_crc32c: u32) -> Result<(), String> {
    if expected_crc32c != actual_crc32c {
        return Err("checksum_crc32c does not match file content".to_owned());
    }

    Ok(())
}

async fn maybe_delete_uploaded_file(service: &EvidenceSubmissionService, key: String) {
    let _ = service.delete_uploaded_attachment_object(&key).await;
}

fn field_content_digest_crc32c(field: &Field<'_>) -> Result<u32, String> {
    let value = field
        .headers()
        .get("content-digest")
        .ok_or_else(|| "Content-Digest is required".to_owned())?
        .to_str()
        .map_err(|_| "Content-Digest must be valid ASCII".to_owned())?;

    parse_content_digest_crc32c(value)
}

fn parse_content_digest_crc32c(value: &str) -> Result<u32, String> {
    let dictionary: Dictionary = Parser::new(value)
        .parse()
        .map_err(|_| "Content-Digest must be a valid structured field dictionary".to_owned())?;
    let entry = dictionary
        .get("crc32c")
        .ok_or_else(|| "Content-Digest crc32c is required".to_owned())?;
    let ListEntry::Item(item) = entry else {
        return Err("Content-Digest crc32c must be a byte sequence".to_owned());
    };
    let BareItem::ByteSequence(bytes) = &item.bare_item else {
        return Err("Content-Digest crc32c must be a byte sequence".to_owned());
    };
    let bytes: [u8; 4] = bytes
        .as_slice()
        .try_into()
        .map_err(|_| "Content-Digest crc32c must encode exactly 4 bytes".to_owned())?;

    Ok(u32::from_be_bytes(bytes))
}

fn encode_crc32c_base64(value: u32) -> String {
    BASE64_STANDARD.encode(value.to_be_bytes())
}

fn validate_coverage_window(
    start: DateTime<Utc>,
    end: DateTime<Utc>,
) -> Validation<(DateTime<Utc>, DateTime<Utc>), DomainError> {
    if end < start {
        return Validation::invalid(DomainError::InvalidCoverageWindow);
    }

    Validation::valid((start, end))
}

#[cfg(test)]
mod tests {
    use chrono::{DateTime, Utc};
    use uuid::Uuid;

    use super::{
        encode_crc32c_base64, parse_content_digest_crc32c, validate_attachment_upload,
        EvidenceSubmissionDTO,
    };
    use crate::domain::{DomainError, EvidenceRequestId};

    #[test]
    fn submission_dto_maps_to_create_payload() {
        let payload = valid_dto().into_new(request_id()).into_result().unwrap();

        assert_eq!(payload.evidence_request_id, request_id());
        assert_eq!(payload.source_system, "okta");
        assert_eq!(payload.collection_method, "api_export");
        assert_eq!(payload.summary.as_deref(), Some("Quarterly review"));
        assert_eq!(payload.description.as_deref(), Some("Review details"));
    }

    #[test]
    fn submission_dto_accumulates_validation_errors() {
        let errors = EvidenceSubmissionDTO {
            coverage_start_at: instant("2026-04-01T00:00:00Z"),
            coverage_end_at: instant("2026-03-31T23:59:59Z"),
            source_system: " ".to_owned(),
            collection_method: "\t".to_owned(),
            summary: Some(" ".to_owned()),
            description: Some("x".repeat(4_001)),
        }
        .into_new(request_id())
        .into_result()
        .unwrap_err();

        assert_eq!(
            errors,
            vec![
                DomainError::EmptyRequiredText {
                    field: "source_system"
                },
                DomainError::EmptyRequiredText {
                    field: "collection_method"
                },
                DomainError::BlankOptionalText { field: "summary" },
                DomainError::OptionalTextTooLong {
                    field: "description",
                    maximum: 4_000,
                },
                DomainError::InvalidCoverageWindow,
            ]
        );
    }

    #[test]
    fn attachment_upload_validation_accepts_matching_crc32c() {
        validate_attachment_upload(
            crc32c::crc32c(b"evidence bytes"),
            crc32c::crc32c(b"evidence bytes"),
        )
        .expect("attachment upload validates");
    }

    #[test]
    fn content_digest_parser_accepts_crc32c_byte_sequence() {
        let digest = format!("sha-256=:abcd:, crc32c=:{}:", encode_crc32c_base64(123));

        assert_eq!(
            parse_content_digest_crc32c(&digest).expect("Content-Digest parses"),
            123
        );
    }

    #[test]
    fn content_digest_parser_rejects_malformed_dictionary() {
        let error = parse_content_digest_crc32c("crc32c=:not base64:")
            .expect_err("malformed Content-Digest is rejected");

        assert_eq!(
            error,
            "Content-Digest must be a valid structured field dictionary"
        );
    }

    #[test]
    fn content_digest_parser_rejects_missing_crc32c() {
        let error =
            parse_content_digest_crc32c("sha-256=:abcd:").expect_err("missing crc32c is rejected");

        assert_eq!(error, "Content-Digest crc32c is required");
    }

    #[test]
    fn content_digest_parser_rejects_non_byte_sequence_crc32c() {
        let error =
            parse_content_digest_crc32c("crc32c=123").expect_err("non-byte-sequence is rejected");

        assert_eq!(error, "Content-Digest crc32c must be a byte sequence");
    }

    #[test]
    fn content_digest_parser_rejects_wrong_crc32c_length() {
        let error =
            parse_content_digest_crc32c("crc32c=:abc:").expect_err("short CRC32C is rejected");

        assert_eq!(error, "Content-Digest crc32c must encode exactly 4 bytes");
    }

    #[test]
    fn attachment_upload_validation_rejects_checksum_mismatch() {
        let error = validate_attachment_upload(
            crc32c::crc32c(b"different bytes"),
            crc32c::crc32c(b"evidence bytes"),
        )
        .expect_err("mismatched CRC32C is rejected");

        assert_eq!(error, "checksum_crc32c does not match file content");
    }
    fn valid_dto() -> EvidenceSubmissionDTO {
        EvidenceSubmissionDTO {
            coverage_start_at: instant("2026-01-01T00:00:00Z"),
            coverage_end_at: instant("2026-03-31T23:59:59Z"),
            source_system: "okta".to_owned(),
            collection_method: "api_export".to_owned(),
            summary: Some("  Quarterly review  ".to_owned()),
            description: Some("  Review details  ".to_owned()),
        }
    }

    fn instant(value: &str) -> DateTime<Utc> {
        value.parse().expect("timestamp parses")
    }

    fn request_id() -> EvidenceRequestId {
        EvidenceRequestId::from(Uuid::parse_str("550e8400-e29b-41d4-a716-446655440001").unwrap())
    }
}
