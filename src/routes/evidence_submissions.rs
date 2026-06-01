use std::collections::HashMap;

use axum::{
    extract::{multipart::MultipartError, DefaultBodyLimit, Multipart, Path, Request, State},
    http::{Method, StatusCode},
    middleware,
    middleware::Next,
    response::Response,
    routing::{get, post},
    Extension, Json, Router,
};
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
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
    routes::{
        authentication::{authorize_workspace_route, ActorContext},
        error::{domain_errors, ApiError},
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
    multipart: Multipart,
) -> Result<(StatusCode, Json<EvidenceAttachmentUploadResponse>), ApiError> {
    let payload =
        attachment_upload_from_multipart(EvidenceSubmissionId::from(path.submission_id), multipart)
            .await?;
    let attachment = state
        .service
        .upload_attachment(
            actor,
            EvidenceSubmissionId::from(path.submission_id),
            payload,
        )
        .await?
        .ok_or(ApiError::NotFound)?;

    Ok((StatusCode::ACCEPTED, Json(attachment.into())))
}

async fn attachment_upload_from_multipart(
    evidence_submission_id: EvidenceSubmissionId,
    mut multipart: Multipart,
) -> Result<UploadEvidenceAttachmentPayload, ApiError> {
    let mut file: Option<ReceivedFilePart> = None;
    let mut checksum_crc32c: Option<String> = None;

    while let Some(field) = multipart.next_field().await.map_err(multipart_error)? {
        match field.name() {
            Some("file") => {
                if file.is_some() {
                    return Err(ApiError::BadRequest(vec![
                        "file must be provided exactly once".to_owned(),
                    ]));
                }
                let filename = field.file_name().map(str::to_owned).ok_or_else(|| {
                    ApiError::BadRequest(vec!["file filename is required".to_owned()])
                })?;
                let content_type = field
                    .content_type()
                    .map(str::to_owned)
                    .unwrap_or_else(|| "application/octet-stream".to_owned());
                let bytes = field.bytes().await.map_err(multipart_error)?;

                file = Some(ReceivedFilePart {
                    filename,
                    content_type,
                    bytes: bytes.to_vec(),
                });
            }
            Some("checksum_crc32c") => {
                if checksum_crc32c.is_some() {
                    return Err(ApiError::BadRequest(vec![
                        "checksum_crc32c must be provided exactly once".to_owned(),
                    ]));
                }
                checksum_crc32c = Some(field.text().await.map_err(multipart_error)?);
            }
            _ => {}
        }
    }

    let file = file.ok_or_else(|| ApiError::BadRequest(vec!["file is required".to_owned()]))?;
    let checksum_crc32c = checksum_crc32c
        .ok_or_else(|| ApiError::BadRequest(vec!["checksum_crc32c is required".to_owned()]))?;

    validate_attachment_upload(evidence_submission_id, file, checksum_crc32c)
        .map_err(|message| ApiError::BadRequest(vec![message]))
}

fn multipart_error(error: MultipartError) -> ApiError {
    if error.status() == StatusCode::PAYLOAD_TOO_LARGE {
        ApiError::PayloadTooLarge
    } else {
        ApiError::BadRequest(vec![format!(
            "invalid multipart body: {}",
            error.body_text()
        )])
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ReceivedFilePart {
    filename: String,
    content_type: String,
    bytes: Vec<u8>,
}

fn validate_attachment_upload(
    evidence_submission_id: EvidenceSubmissionId,
    file: ReceivedFilePart,
    checksum_crc32c: String,
) -> Result<UploadEvidenceAttachmentPayload, String> {
    if file.filename.trim().is_empty() {
        return Err("file filename is required".to_owned());
    }

    let expected_crc32c = decode_crc32c_base64(&checksum_crc32c)?;
    let actual_crc32c = crc32c::crc32c(&file.bytes);
    if expected_crc32c != actual_crc32c {
        return Err("checksum_crc32c does not match file content".to_owned());
    }

    let content_length =
        i64::try_from(file.bytes.len()).map_err(|_| "file is too large".to_owned())?;

    Ok(UploadEvidenceAttachmentPayload {
        evidence_submission_id,
        filename: file.filename,
        content_type: file.content_type,
        content_length,
        checksum_sha256: hex::encode(Sha256::digest(&file.bytes)),
        checksum_crc32c: encode_crc32c_base64(actual_crc32c),
        bytes: file.bytes,
    })
}

fn decode_crc32c_base64(value: &str) -> Result<u32, String> {
    let bytes = BASE64_STANDARD
        .decode(value.trim())
        .map_err(|_| "checksum_crc32c must be base64-encoded CRC32C bytes".to_owned())?;
    let bytes: [u8; 4] = bytes
        .try_into()
        .map_err(|_| "checksum_crc32c must encode exactly 4 bytes".to_owned())?;

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
        encode_crc32c_base64, validate_attachment_upload, EvidenceSubmissionDTO, ReceivedFilePart,
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
        let bytes = b"evidence bytes".to_vec();
        let checksum = encode_crc32c_base64(crc32c::crc32c(&bytes));
        let payload = validate_attachment_upload(
            submission_id(),
            ReceivedFilePart {
                filename: "artifact.json".to_owned(),
                content_type: "application/json".to_owned(),
                bytes,
            },
            checksum.clone(),
        )
        .expect("attachment upload validates");

        assert_eq!(payload.evidence_submission_id, submission_id());
        assert_eq!(payload.filename, "artifact.json");
        assert_eq!(payload.content_type, "application/json");
        assert_eq!(payload.content_length, 14);
        assert_eq!(payload.checksum_crc32c, checksum);
    }

    #[test]
    fn attachment_upload_validation_rejects_invalid_base64_crc32c() {
        let error =
            validate_attachment_upload(submission_id(), valid_file_part(), "not base64".to_owned())
                .expect_err("invalid CRC32C is rejected");

        assert_eq!(error, "checksum_crc32c must be base64-encoded CRC32C bytes");
    }

    #[test]
    fn attachment_upload_validation_rejects_checksum_mismatch() {
        let error = validate_attachment_upload(
            submission_id(),
            valid_file_part(),
            encode_crc32c_base64(crc32c::crc32c(b"different bytes")),
        )
        .expect_err("mismatched CRC32C is rejected");

        assert_eq!(error, "checksum_crc32c does not match file content");
    }

    #[test]
    fn attachment_upload_validation_rejects_blank_filename() {
        let bytes = b"evidence bytes".to_vec();
        let error = validate_attachment_upload(
            submission_id(),
            ReceivedFilePart {
                filename: " \t".to_owned(),
                content_type: "application/octet-stream".to_owned(),
                bytes: bytes.clone(),
            },
            encode_crc32c_base64(crc32c::crc32c(&bytes)),
        )
        .expect_err("blank filename is rejected");

        assert_eq!(error, "file filename is required");
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

    fn submission_id() -> crate::domain::EvidenceSubmissionId {
        crate::domain::EvidenceSubmissionId::from(
            Uuid::parse_str("550e8400-e29b-41d4-a716-446655440002").unwrap(),
        )
    }

    fn valid_file_part() -> ReceivedFilePart {
        ReceivedFilePart {
            filename: "artifact.txt".to_owned(),
            content_type: "text/plain".to_owned(),
            bytes: b"evidence bytes".to_vec(),
        }
    }
}
