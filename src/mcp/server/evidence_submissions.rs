use rmcp::{
    handler::server::wrapper::Parameters, schemars, schemars::JsonSchema, service::RequestContext,
    tool, tool_router, Json, RoleServer,
};
use serde::{Deserialize, Serialize};

use super::{
    common::{
        argument_errors, authorize, format_datetime, not_found, required_uuid, service_error,
    },
    evidence_requests::{parse_evidence_request_request, EvidenceRequestRequest},
    ProofplaneMcp,
};
use crate::{
    domain::{
        EvidenceAttachment, EvidenceSubmission, EvidenceSubmissionDetail, EvidenceSubmissionId,
        WorkspaceId, WorkspacePermission,
    },
    validate,
};

#[tool_router(router = evidence_submissions_tool_router, vis = "pub(super)")]
impl ProofplaneMcp {
    #[tool(
        name = "get_evidence_submission",
        description = "Get a selectively detailed evidence submission by id."
    )]
    async fn get_evidence_submission(
        &self,
        ctx: RequestContext<RoleServer>,
        Parameters(args): Parameters<GetEvidenceSubmissionRequest>,
    ) -> Result<Json<GetEvidenceSubmissionResponse>, rmcp::ErrorData> {
        let (workspace_id, submission_id) = parse_evidence_submission_request(args)?;
        let context = authorize(
            &ctx,
            workspace_id,
            WorkspacePermission::ReadEvidenceSubmissions,
        )?;
        let detail = self
            .evidence_submissions
            .get(context.token, submission_id)
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
        let (workspace_id, evidence_request_id) = parse_evidence_request_request(args)?;
        let context = authorize(
            &ctx,
            workspace_id,
            WorkspacePermission::ReadEvidenceSubmissions,
        )?;
        let detail = self
            .evidence_submissions
            .latest_for_request(context.token, evidence_request_id)
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
pub(super) struct GetEvidenceSubmissionRequest {
    pub(super) workspace_id: Option<String>,
    pub(super) submission_id: Option<String>,
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
            submitted_by: EvidenceSubmitterResponseDTO {
                api_token_id: submission.submitted_by.api_token_id.to_string(),
                user_id: submission.submitted_by.user_id.to_string(),
            },
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
    api_token_id: String,
    user_id: String,
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
) -> Result<(WorkspaceId, EvidenceSubmissionId), rmcp::ErrorData> {
    validate! {
        workspace_id <- required_uuid("workspace_id", args.workspace_id).map(WorkspaceId::from),
        submission_id <- required_uuid("submission_id", args.submission_id)
            .map(EvidenceSubmissionId::from),
        => (workspace_id, submission_id),
    }
    .into_result()
    .map_err(argument_errors)
}
