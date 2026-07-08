use rmcp::{
    handler::server::wrapper::Parameters, schemars, schemars::JsonSchema, service::RequestContext,
    tool, tool_router, Json, RoleServer,
};
use serde::{Deserialize, Serialize};

use super::{
    common::{
        argument_errors, authorize_token_workspace, domain_errors, format_datetime, not_found,
        required_timestamp, required_uuid, service_error,
    },
    evidence_requests::{parse_evidence_request_request, EvidenceRequestRequest},
    ProofplaneMcp,
};
use crate::{
    domain::{
        optional_text, required_text, CreateEvidenceSubmissionPayload, DomainError,
        EvidenceAttachment, EvidenceRequestId, EvidenceSubmission, EvidenceSubmissionDetail,
        EvidenceSubmissionId, EvidenceSubmitter, WorkspaceId, WorkspacePermission,
    },
    observability::audit::{AuditClientType, AuditEvent, AuditObject, AuditOutcome},
    validate,
    validation::Validation,
};
use chrono::{DateTime, Utc};
use uuid::Uuid;

#[tool_router(router = evidence_submissions_tool_router, vis = "pub(super)")]
impl ProofplaneMcp {
    #[tool(
        name = "create_evidence_submission",
        description = "Create an evidence submission record and return REST-only attachment upload instructions."
    )]
    async fn create_evidence_submission(
        &self,
        ctx: RequestContext<RoleServer>,
        Parameters(args): Parameters<CreateEvidenceSubmissionRequest>,
    ) -> Result<Json<CreateEvidenceSubmissionResponse>, rmcp::ErrorData> {
        let (evidence_request_id, payload) = parse_create_submission_request(args)?;
        let context =
            authorize_token_workspace(&ctx, WorkspacePermission::WriteEvidenceSubmissions)?;
        let workspace_id = context.connection.workspace_id;
        let submission = self
            .evidence_submissions
            .create(
                context.agent_connection_context(),
                evidence_request_id,
                payload,
            )
            .await
            .map_err(service_error)?
        .ok_or_else(not_found)?;

        AuditEvent::new(
            "evidence_submission.created",
            AuditOutcome::Success,
            context.audit_actor(),
            AuditClientType::Mcp,
            "create_evidence_submission",
        )
        .workspace_id(workspace_id.into())
        .request_id(context.request_id.0)
        .metadata("evidence_request_id", Uuid::from(evidence_request_id))
        .metadata("evidence_submission_id", Uuid::from(submission.id))
        .object(AuditObject::new(
            "evidence_submission",
            submission.id.into(),
        ))
        .emit();

        Ok(Json(CreateEvidenceSubmissionResponse::new(
            workspace_id,
            submission,
        )))
    }

    #[tool(
        name = "get_evidence_submission",
        description = "Get a selectively detailed evidence submission by id."
    )]
    async fn get_evidence_submission(
        &self,
        ctx: RequestContext<RoleServer>,
        Parameters(args): Parameters<GetEvidenceSubmissionRequest>,
    ) -> Result<Json<GetEvidenceSubmissionResponse>, rmcp::ErrorData> {
        let submission_id = parse_evidence_submission_request(args)?;
        let context =
            authorize_token_workspace(&ctx, WorkspacePermission::ReadEvidenceSubmissions)?;
        let detail = self
            .evidence_submissions
            .get(context.agent_connection_context(), submission_id)
            .await
            .map_err(service_error)?
            .ok_or_else(not_found)?;

        Ok(Json(GetEvidenceSubmissionResponse::from_detail(
            detail,
            SubmissionDetailMode::Direct,
        )))
    }

    #[tool(
        name = "get_latest_evidence_submission",
        description = "Get the latest selectively summarized submission for an evidence request."
    )]
    async fn get_latest_evidence_submission(
        &self,
        ctx: RequestContext<RoleServer>,
        Parameters(args): Parameters<EvidenceRequestRequest>,
    ) -> Result<Json<GetEvidenceSubmissionResponse>, rmcp::ErrorData> {
        let evidence_request_id = parse_evidence_request_request(args)?;
        let context =
            authorize_token_workspace(&ctx, WorkspacePermission::ReadEvidenceSubmissions)?;
        let detail = self
            .evidence_submissions
            .latest_for_request(context.agent_connection_context(), evidence_request_id)
            .await
            .map_err(service_error)?
            .ok_or_else(not_found)?;

        Ok(Json(GetEvidenceSubmissionResponse::from_detail(
            detail,
            SubmissionDetailMode::Latest,
        )))
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
struct CreateEvidenceSubmissionRequest {
    evidence_request_id: Option<String>,
    coverage_start_at: Option<String>,
    coverage_end_at: Option<String>,
    source_system: Option<String>,
    collection_method: Option<String>,
    summary: Option<String>,
    description: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(super) struct GetEvidenceSubmissionRequest {
    pub(super) submission_id: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
struct CreateEvidenceSubmissionResponse {
    submission_id: String,
    evidence_request_id: String,
}

impl CreateEvidenceSubmissionResponse {
    fn new(_workspace_id: WorkspaceId, submission: EvidenceSubmission) -> Self {
        Self {
            submission_id: submission.id.to_string(),
            evidence_request_id: submission.evidence_request_id.to_string(),
        }
    }
}

#[derive(Debug, Serialize, JsonSchema)]
struct GetEvidenceSubmissionResponse {
    submission: EvidenceSubmissionResponseDTO,
    attachments: Vec<EvidenceAttachmentResponseDTO>,
}

enum SubmissionDetailMode {
    Direct,
    Latest,
}

impl GetEvidenceSubmissionResponse {
    fn from_detail(detail: EvidenceSubmissionDetail, mode: SubmissionDetailMode) -> Self {
        Self {
            submission: EvidenceSubmissionResponseDTO::from_submission(detail.submission, mode),
            attachments: detail.attachments.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Debug, Serialize, JsonSchema)]
struct EvidenceSubmissionResponseDTO {
    id: String,
    evidence_request_id: String,
    submitted_by: EvidenceSubmitterResponseDTO,
    #[serde(skip_serializing_if = "Option::is_none")]
    received_at: Option<String>,
    coverage_start_at: String,
    coverage_end_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_system: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    collection_method: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
}

impl EvidenceSubmissionResponseDTO {
    fn from_submission(submission: EvidenceSubmission, mode: SubmissionDetailMode) -> Self {
        let direct = matches!(mode, SubmissionDetailMode::Direct);
        Self {
            id: submission.id.to_string(),
            evidence_request_id: submission.evidence_request_id.to_string(),
            submitted_by: EvidenceSubmitterResponseDTO::from(submission.submitted_by),
            received_at: direct.then_some(format_datetime(submission.received_at)),
            coverage_start_at: format_datetime(submission.coverage_start_at),
            coverage_end_at: format_datetime(submission.coverage_end_at),
            source_system: direct.then_some(submission.source_system),
            collection_method: direct.then_some(submission.collection_method),
            summary: submission.summary,
            description: direct.then_some(submission.description).flatten(),
        }
    }
}

#[derive(Debug, Serialize, JsonSchema)]
struct EvidenceSubmitterResponseDTO {
    agent_connection_id: Option<String>,
    user_id: String,
}

impl From<EvidenceSubmitter> for EvidenceSubmitterResponseDTO {
    fn from(submitter: EvidenceSubmitter) -> Self {
        Self {
            agent_connection_id: submitter.agent_connection_id().map(|id| id.to_string()),
            user_id: submitter.user_id().to_string(),
        }
    }
}

#[derive(Debug, Serialize, JsonSchema)]
struct EvidenceAttachmentResponseDTO {
    id: String,
    evidence_submission_id: String,
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
            id: attachment.id.to_string(),
            evidence_submission_id: attachment.evidence_submission_id.to_string(),
            filename: attachment.filename,
            content_type: attachment.content_type,
            content_length: attachment.content_length,
            checksum_sha256: attachment.checksum_sha256,
            checksum_crc32c: attachment.checksum_crc32c,
            upload_status: attachment.upload_status.as_str(),
        }
    }
}

pub(super) fn parse_evidence_submission_request(
    args: GetEvidenceSubmissionRequest,
) -> Result<EvidenceSubmissionId, rmcp::ErrorData> {
    validate! {
        submission_id <- required_uuid("submission_id", args.submission_id)
            .map(EvidenceSubmissionId::from),
        => submission_id,
    }
    .into_result()
    .map_err(argument_errors)
}

fn parse_create_submission_request(
    args: CreateEvidenceSubmissionRequest,
) -> Result<(EvidenceRequestId, CreateEvidenceSubmissionPayload), rmcp::ErrorData> {
    let (evidence_request_id, coverage_start_at, coverage_end_at) = validate! {
        evidence_request_id <- required_uuid("evidence_request_id", args.evidence_request_id)
            .map(EvidenceRequestId::from),
        coverage_start_at <- required_timestamp("coverage_start_at", args.coverage_start_at),
        coverage_end_at <- required_timestamp("coverage_end_at", args.coverage_end_at),
        => (evidence_request_id, coverage_start_at, coverage_end_at),
    }
    .into_result()
    .map_err(argument_errors)?;

    let payload = validate! {
        source_system <- required_text("source_system", args.source_system.unwrap_or_default()),
        collection_method <- required_text(
            "collection_method",
            args.collection_method.unwrap_or_default()
        ),
        summary <- optional_text("summary", args.summary, 500),
        description <- optional_text("description", args.description, 4_000),
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
    .into_result()
    .map_err(domain_errors)?;

    Ok((evidence_request_id, payload))
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
