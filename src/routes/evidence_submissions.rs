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
use serde_json::{Map, Value};
use sfv::{BareItem, Dictionary, ListEntry, Parser};
use tracing::error;
use uuid::Uuid;

use crate::{
    authentication::ApiKeyAuthenticator,
    authorization::workspaces::WorkspaceAuthorizer,
    domain::{
        required_text, CreateEvidenceSubmissionPayload, DomainError, EvidenceAttachment,
        EvidenceAttachmentScan, EvidenceAttachmentWithScan, EvidenceRequestId, EvidenceSubmission,
        EvidenceSubmissionDetail, EvidenceSubmissionId,
    },
    object_storage::StorageError,
    routes::{
        authentication::{authorize_workspace_route, ActorContext},
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
    pub authenticator: ApiKeyAuthenticator,
    pub authorizer: WorkspaceAuthorizer,
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
    let authorizer = state.authorizer.clone();
    let actor = authorize_workspace_route(&state.authenticator, &path, &mut request).await?;
    let workspace_id = actor.workspace_id;

    let allowed = match method {
        Method::GET => authorizer
            .can_read_evidence_submissions(&actor)
            .await
            .map_err(|e| {
                error!(
                    method = %method,
                    actor = %actor.id,
                    workspace = %workspace_id,
                    error = %e,
                    "unable to check read permissions for evidence submissions"
                );
                ApiError::Internal
            }),
        Method::POST => authorizer
            .can_write_evidence_submissions(&actor)
            .await
            .map_err(|e| {
                error!(
                    method = %method,
                    actor = %actor.id,
                    workspace = %workspace_id,
                    error = %e,
                    "unable to check write permissions for evidence submissions"
                );
                ApiError::Internal
            }),
        _ => Err(ApiError::MethodNotAllowed),
    }?;

    if !allowed {
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
    provenance: Option<Value>,
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
            provenance <- validate_provenance(self.provenance),
            coverage_window <- validate_coverage_window(coverage_start_at, coverage_end_at),
            => CreateEvidenceSubmissionPayload {
                evidence_request_id,
                coverage_start_at: coverage_window.0,
                coverage_end_at: coverage_window.1,
                source_system,
                collection_method,
                provenance,
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

#[derive(Debug, Serialize)]
struct EvidenceSubmissionResponse {
    id: Uuid,
    evidence_request_id: Uuid,
    submitted_by: Uuid,
    received_at: DateTime<Utc>,
    coverage_start_at: DateTime<Utc>,
    coverage_end_at: DateTime<Utc>,
    source_system: String,
    collection_method: String,
    provenance: Value,
}

impl From<EvidenceSubmission> for EvidenceSubmissionResponse {
    fn from(submission: EvidenceSubmission) -> Self {
        Self {
            id: Uuid::from(submission.id),
            evidence_request_id: Uuid::from(submission.evidence_request_id),
            submitted_by: Uuid::from(submission.submitted_by),
            received_at: submission.received_at,
            coverage_start_at: submission.coverage_start_at,
            coverage_end_at: submission.coverage_end_at,
            source_system: submission.source_system,
            collection_method: submission.collection_method,
            provenance: submission.provenance,
        }
    }
}

#[derive(Debug, Serialize)]
struct EvidenceAttachmentResponse {
    id: Uuid,
    evidence_submission_id: Uuid,
    filename: String,
    content_type: String,
    content_length: i64,
    object_key: String,
    checksum_sha256: String,
    checksum_crc32c: String,
}

impl From<EvidenceAttachment> for EvidenceAttachmentResponse {
    fn from(attachment: EvidenceAttachment) -> Self {
        Self {
            id: Uuid::from(attachment.id),
            evidence_submission_id: Uuid::from(attachment.evidence_submission_id),
            filename: attachment.filename,
            content_type: attachment.content_type,
            content_length: attachment.content_length,
            object_key: attachment.object_key,
            checksum_sha256: attachment.checksum_sha256,
            checksum_crc32c: attachment.checksum_crc32c,
        }
    }
}

#[derive(Debug, Serialize)]
struct EvidenceAttachmentScanResponse {
    evidence_attachment_id: Uuid,
    scan_status: &'static str,
    scanner_name: Option<String>,
    scanner_version: Option<String>,
    scanned_at: Option<DateTime<Utc>>,
    scan_failure_reason: Option<String>,
    updated_at: DateTime<Utc>,
}

impl From<EvidenceAttachmentScan> for EvidenceAttachmentScanResponse {
    fn from(scan: EvidenceAttachmentScan) -> Self {
        Self {
            evidence_attachment_id: Uuid::from(scan.evidence_attachment_id),
            scan_status: scan.scan_status.as_str(),
            scanner_name: scan.scanner_name,
            scanner_version: scan.scanner_version,
            scanned_at: scan.scanned_at,
            scan_failure_reason: scan.scan_failure_reason,
            updated_at: scan.updated_at,
        }
    }
}

#[derive(Debug, Serialize)]
struct EvidenceAttachmentWithScanResponse {
    attachment: EvidenceAttachmentResponse,
    scan: EvidenceAttachmentScanResponse,
}

impl From<EvidenceAttachmentWithScan> for EvidenceAttachmentWithScanResponse {
    fn from(value: EvidenceAttachmentWithScan) -> Self {
        Self {
            attachment: value.attachment.into(),
            scan: value.scan.into(),
        }
    }
}

#[derive(Debug, Serialize)]
struct EvidenceSubmissionDetailResponse {
    submission: EvidenceSubmissionResponse,
    attachments: Vec<EvidenceAttachmentWithScanResponse>,
}

impl From<EvidenceSubmissionDetail> for EvidenceSubmissionDetailResponse {
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
    Extension(actor): Extension<ActorContext>,
    Json(body): Json<EvidenceSubmissionDTO>,
) -> Result<Json<EvidenceSubmissionResponse>, ApiError> {
    let evidence_request_id = EvidenceRequestId::from(path.evidence_request_id);
    let payload = body
        .into_new(evidence_request_id)
        .into_result()
        .map_err(domain_errors)?;
    let submission = state
        .service
        .create(actor, evidence_request_id, payload)
        .await?
        .ok_or(ApiError::NotFound)?;

    Ok(Json(submission.into()))
}

async fn get_evidence_submission(
    State(state): State<EvidenceSubmissionState>,
    Path(path): Path<EvidenceSubmissionPath>,
    Extension(actor): Extension<ActorContext>,
) -> Result<Json<EvidenceSubmissionDetailResponse>, ApiError> {
    let detail = state
        .service
        .get(actor, EvidenceSubmissionId::from(path.submission_id))
        .await?
        .ok_or(ApiError::NotFound)?;

    Ok(Json(detail.into()))
}

#[derive(Debug, Serialize)]
struct EvidenceAttachmentUploadResponse {
    attachment: EvidenceAttachmentResponse,
    scan: EvidenceAttachmentScanResponse,
}

impl From<EvidenceAttachmentWithScan> for EvidenceAttachmentUploadResponse {
    fn from(value: EvidenceAttachmentWithScan) -> Self {
        Self {
            attachment: value.attachment.into(),
            scan: value.scan.into(),
        }
    }
}

async fn upload_evidence_attachment(
    State(state): State<EvidenceSubmissionState>,
    Path(path): Path<EvidenceSubmissionAttachmentPath>,
    Extension(actor): Extension<ActorContext>,
    Extension(request_id): Extension<RequestId>,
    multipart: Multipart,
) -> Result<(StatusCode, Json<EvidenceAttachmentUploadResponse>), ApiError> {
    let submission_id = EvidenceSubmissionId::from(path.submission_id);
    if !state
        .service
        .evidence_submission_exists(actor.clone(), submission_id)
        .await?
    {
        return Err(ApiError::NotFound);
    }

    // Uploading the attachment happens before the database record for the attachment is stored
    // because if the database record fails to be created, the orphaned attachment can be
    // automatically cleaned up with object storage retention settings since it's in a quarantine
    // bucket and that will probably have a cleanup policy.
    let payload =
        attachment_upload_from_multipart(&state.service, actor.clone(), submission_id, multipart)
            .await?;
    let attachment = state
        .service
        .create_attachment(actor, request_id.0, submission_id, payload)
        .await?;

    Ok((StatusCode::ACCEPTED, Json(attachment.into())))
}

async fn attachment_upload_from_multipart(
    service: &EvidenceSubmissionService,
    actor: ActorContext,
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
    if filename.trim().is_empty() {
        return Err(ApiError::BadRequest(vec![
            "file filename is required".to_owned()
        ]));
    }
    let expected_crc32c = field_content_digest_crc32c(&field)
        .map_err(|message| ApiError::BadRequest(vec![message]))?;

    let crc32c = Arc::new(AtomicU32::new(0));
    let chunks = file_chunks(field, Arc::clone(&crc32c));
    let mut uploaded_file = service
        .upload_attachment(
            actor.clone(),
            evidence_submission_id,
            filename,
            content_type,
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

fn validate_provenance(value: Option<Value>) -> Validation<Value, DomainError> {
    match value {
        None => Validation::valid(Value::Object(Map::new())),
        Some(value @ Value::Object(_)) => Validation::valid(value),
        Some(_) => Validation::invalid(DomainError::InvalidProvenanceObject),
    }
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
    use serde_json::json;
    use uuid::Uuid;

    use super::{
        encode_crc32c_base64, parse_content_digest_crc32c, validate_attachment_upload,
        EvidenceSubmissionDTO,
    };
    use crate::domain::{DomainError, EvidenceRequestId};

    #[test]
    fn submission_dto_maps_to_create_payload() {
        let payload = valid_dto(None)
            .into_new(request_id())
            .into_result()
            .unwrap();

        assert_eq!(payload.evidence_request_id, request_id());
        assert_eq!(payload.source_system, "okta");
        assert_eq!(payload.collection_method, "api_export");
        assert_eq!(payload.provenance, json!({}));
    }

    #[test]
    fn submission_dto_accumulates_validation_errors() {
        let errors = EvidenceSubmissionDTO {
            coverage_start_at: instant("2026-04-01T00:00:00Z"),
            coverage_end_at: instant("2026-03-31T23:59:59Z"),
            source_system: " ".to_owned(),
            collection_method: "\t".to_owned(),
            provenance: Some(json!("external-run-123")),
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
                DomainError::InvalidProvenanceObject,
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
    fn valid_dto(provenance: Option<serde_json::Value>) -> EvidenceSubmissionDTO {
        EvidenceSubmissionDTO {
            coverage_start_at: instant("2026-01-01T00:00:00Z"),
            coverage_end_at: instant("2026-03-31T23:59:59Z"),
            source_system: "okta".to_owned(),
            collection_method: "api_export".to_owned(),
            provenance,
        }
    }

    fn instant(value: &str) -> DateTime<Utc> {
        value.parse().expect("timestamp parses")
    }

    fn request_id() -> EvidenceRequestId {
        EvidenceRequestId::from(Uuid::parse_str("550e8400-e29b-41d4-a716-446655440001").unwrap())
    }
}
