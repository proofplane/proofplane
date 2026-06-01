use std::collections::HashMap;

use axum::{
    extract::{Path, Request, State},
    http::Method,
    middleware,
    middleware::Next,
    response::Response,
    routing::{get, post},
    Extension, Json, Router,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
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
    services::evidence_submissions::EvidenceSubmissionService,
    validate,
    validation::Validation,
};

#[derive(Clone)]
pub struct EvidenceSubmissionState {
    pub service: EvidenceSubmissionService,
    pub route_auth: EvidenceSubmissionRouteAuthState,
}

#[derive(Clone)]
pub struct EvidenceSubmissionRouteAuthState {
    pub authenticator: ApiKeyAuthenticator,
    pub authorizer: WorkspaceAuthorizer,
}

pub fn router(state: EvidenceSubmissionState) -> Router {
    let route_auth = state.route_auth.clone();

    Router::new()
        .route(
            "/workspaces/{workspace_id}/evidence-requests/{evidence_request_id}/submissions",
            post(create_evidence_submission),
        )
        .route(
            "/workspaces/{workspace_id}/evidence-submissions/{submission_id}",
            get(get_evidence_submission),
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

    use super::EvidenceSubmissionDTO;
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
