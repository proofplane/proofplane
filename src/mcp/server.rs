use chrono::{DateTime, SecondsFormat, Utc};
use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{ErrorCode, Implementation, ServerCapabilities, ServerInfo},
    schemars,
    schemars::JsonSchema,
    service::RequestContext,
    tool, tool_handler, tool_router, Json, RoleServer, ServerHandler,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use super::context::McpRequestContext;
use crate::{
    domain::{
        Control, EvidenceAttachment, EvidenceRequest, EvidenceRequestControlMapping,
        EvidenceRequestId, EvidenceSubmission, EvidenceSubmissionDetail, EvidenceSubmissionId,
        WorkspaceId, WorkspacePermission,
    },
    services::{
        attachment_downloads::{AttachmentDownloadService, DownloadError, IssuedDownloadGrant},
        controls::ControlService,
        evidence_requests::EvidenceRequestService,
        evidence_submissions::EvidenceSubmissionService,
        Error as ServiceError,
    },
    VERSION,
};

#[derive(Clone)]
pub struct ProofplaneMcp {
    evidence_requests: EvidenceRequestService,
    evidence_submissions: EvidenceSubmissionService,
    attachment_downloads: AttachmentDownloadService,
    controls: ControlService,
    tool_router: ToolRouter<Self>,
}

impl ProofplaneMcp {
    pub fn new(
        evidence_requests: EvidenceRequestService,
        evidence_submissions: EvidenceSubmissionService,
        attachment_downloads: AttachmentDownloadService,
        controls: ControlService,
    ) -> Self {
        Self {
            evidence_requests,
            evidence_submissions,
            attachment_downloads,
            controls,
            tool_router: Self::tool_router(),
        }
    }
}

#[tool_router(router = tool_router)]
impl ProofplaneMcp {
    #[tool(
        name = "list_evidence_requests",
        description = "List evidence requests in a workspace."
    )]
    async fn list_evidence_requests(
        &self,
        ctx: RequestContext<RoleServer>,
        Parameters(args): Parameters<WorkspaceArgs>,
    ) -> Result<Json<ListEvidenceRequestsResult>, rmcp::ErrorData> {
        let workspace_id =
            parse_uuid_arg("workspace_id", args.workspace_id).map(WorkspaceId::from)?;
        let context = authorize(
            &ctx,
            workspace_id,
            WorkspacePermission::ReadEvidenceRequests,
        )?;
        let requests = self
            .evidence_requests
            .list_by_workspace(context.token)
            .await
            .map_err(service_error)?;

        Ok(Json(ListEvidenceRequestsResult {
            evidence_requests: requests.into_iter().map(Into::into).collect(),
        }))
    }

    #[tool(
        name = "get_evidence_request",
        description = "Get an evidence request by id."
    )]
    async fn get_evidence_request(
        &self,
        ctx: RequestContext<RoleServer>,
        Parameters(args): Parameters<EvidenceRequestArgs>,
    ) -> Result<Json<GetEvidenceRequestResult>, rmcp::ErrorData> {
        let (workspace_id, evidence_request_id) = parse_evidence_request_args(args)?;
        let context = authorize(
            &ctx,
            workspace_id,
            WorkspacePermission::ReadEvidenceRequests,
        )?;
        let request = self
            .evidence_requests
            .get(context.token, evidence_request_id)
            .await
            .map_err(service_error)?
            .ok_or_else(not_found)?;

        Ok(Json(GetEvidenceRequestResult {
            evidence_request: request.into(),
        }))
    }

    #[tool(
        name = "list_due_evidence_requests",
        description = "List evidence requests due at or before a point in time."
    )]
    async fn list_due_evidence_requests(
        &self,
        ctx: RequestContext<RoleServer>,
        Parameters(args): Parameters<ListDueEvidenceRequestsArgs>,
    ) -> Result<Json<ListEvidenceRequestsResult>, rmcp::ErrorData> {
        let (workspace_id, now) = parse_due_args(args)?;
        let context = authorize(
            &ctx,
            workspace_id,
            WorkspacePermission::ReadEvidenceRequests,
        )?;
        let requests = self
            .evidence_requests
            .list_due(context.token, now.unwrap_or_else(Utc::now))
            .await
            .map_err(service_error)?;

        Ok(Json(ListEvidenceRequestsResult {
            evidence_requests: requests.into_iter().map(Into::into).collect(),
        }))
    }

    #[tool(
        name = "get_evidence_submission",
        description = "Get a selectively detailed evidence submission by id."
    )]
    async fn get_evidence_submission(
        &self,
        ctx: RequestContext<RoleServer>,
        Parameters(args): Parameters<EvidenceSubmissionArgs>,
    ) -> Result<Json<GetEvidenceSubmissionResult>, rmcp::ErrorData> {
        let (workspace_id, submission_id) = parse_submission_args(args)?;
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

        Ok(Json(GetEvidenceSubmissionResult::from_detail(
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
        Parameters(args): Parameters<EvidenceRequestArgs>,
    ) -> Result<Json<GetEvidenceSubmissionResult>, rmcp::ErrorData> {
        let (workspace_id, evidence_request_id) = parse_evidence_request_args(args)?;
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

        Ok(Json(GetEvidenceSubmissionResult::from_detail(
            detail,
            SubmissionDetailMode::Latest,
        )))
    }

    #[tool(
        name = "create_attachment_download_grant",
        description = "Create a short-lived human-use download URL for a finalized attachment."
    )]
    async fn create_attachment_download_grant(
        &self,
        ctx: RequestContext<RoleServer>,
        Parameters(args): Parameters<AttachmentDownloadGrantArgs>,
    ) -> Result<Json<CreateAttachmentDownloadGrantResult>, rmcp::ErrorData> {
        let (workspace_id, submission_id, attachment_id) = parse_download_grant_args(args)?;
        let context = authorize(
            &ctx,
            workspace_id,
            WorkspacePermission::ReadEvidenceSubmissions,
        )?;
        let grant = self
            .attachment_downloads
            .issue(&context.token, submission_id, attachment_id)
            .await
            .map_err(download_error)?;

        Ok(Json(grant.into()))
    }

    #[tool(name = "list_controls", description = "List controls in a workspace.")]
    async fn list_controls(
        &self,
        ctx: RequestContext<RoleServer>,
        Parameters(args): Parameters<WorkspaceArgs>,
    ) -> Result<Json<ListControlsResult>, rmcp::ErrorData> {
        let workspace_id =
            parse_uuid_arg("workspace_id", args.workspace_id).map(WorkspaceId::from)?;
        let context = authorize(&ctx, workspace_id, WorkspacePermission::ReadControls)?;
        let controls = self
            .controls
            .list_controls(context.token)
            .await
            .map_err(service_error)?;

        Ok(Json(ListControlsResult {
            controls: controls.into_iter().map(Into::into).collect(),
        }))
    }

    #[tool(
        name = "list_evidence_request_control_mappings",
        description = "List control mappings for an evidence request."
    )]
    async fn list_evidence_request_control_mappings(
        &self,
        ctx: RequestContext<RoleServer>,
        Parameters(args): Parameters<EvidenceRequestArgs>,
    ) -> Result<Json<ListEvidenceRequestControlMappingsResult>, rmcp::ErrorData> {
        let (workspace_id, evidence_request_id) = parse_evidence_request_args(args)?;
        let context = authorize(&ctx, workspace_id, WorkspacePermission::ReadControls)?;
        let mappings = self
            .controls
            .list_evidence_request_control_mappings(context.token, evidence_request_id)
            .await
            .map_err(service_error)?
            .ok_or_else(not_found)?;

        Ok(Json(ListEvidenceRequestControlMappingsResult {
            mappings: mappings.into_iter().map(Into::into).collect(),
        }))
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for ProofplaneMcp {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("proofplane", VERSION))
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
struct WorkspaceArgs {
    workspace_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct EvidenceRequestArgs {
    workspace_id: Option<String>,
    evidence_request_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ListDueEvidenceRequestsArgs {
    workspace_id: Option<String>,
    now: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct EvidenceSubmissionArgs {
    workspace_id: Option<String>,
    submission_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct AttachmentDownloadGrantArgs {
    workspace_id: Option<String>,
    submission_id: Option<String>,
    attachment_id: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
struct ListEvidenceRequestsResult {
    evidence_requests: Vec<EvidenceRequestDto>,
}

#[derive(Debug, Serialize, JsonSchema)]
struct GetEvidenceRequestResult {
    evidence_request: EvidenceRequestDto,
}

#[derive(Debug, Serialize, JsonSchema)]
struct EvidenceRequestDto {
    id: String,
    workspace_id: String,
    title: String,
    description: String,
    collection_instructions: String,
    cadence: &'static str,
    due_at: String,
    schedule_anchor_at: String,
    freshness_window_days: Option<i32>,
    status: &'static str,
    created_at: String,
    updated_at: String,
}

impl From<EvidenceRequest> for EvidenceRequestDto {
    fn from(request: EvidenceRequest) -> Self {
        Self {
            id: request.id.to_string(),
            workspace_id: request.workspace_id.to_string(),
            title: request.title,
            description: request.description,
            collection_instructions: request.collection_instructions,
            cadence: request.cadence.as_str(),
            due_at: format_datetime(request.due_at),
            schedule_anchor_at: format_datetime(request.schedule_anchor_at),
            freshness_window_days: request.freshness_window_days,
            status: request.status.as_str(),
            created_at: format_datetime(request.created_at),
            updated_at: format_datetime(request.updated_at),
        }
    }
}

#[derive(Debug, Serialize, JsonSchema)]
struct GetEvidenceSubmissionResult {
    submission: EvidenceSubmissionDto,
    attachments: Vec<EvidenceAttachmentDto>,
}

enum SubmissionDetailMode {
    Direct,
    Latest,
}

impl GetEvidenceSubmissionResult {
    fn from_detail(detail: EvidenceSubmissionDetail, mode: SubmissionDetailMode) -> Self {
        Self {
            submission: EvidenceSubmissionDto::from_submission(detail.submission, mode),
            attachments: detail.attachments.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Debug, Serialize, JsonSchema)]
struct EvidenceSubmissionDto {
    id: String,
    evidence_request_id: String,
    submitted_by: EvidenceSubmitterDto,
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

impl EvidenceSubmissionDto {
    fn from_submission(submission: EvidenceSubmission, mode: SubmissionDetailMode) -> Self {
        let direct = matches!(mode, SubmissionDetailMode::Direct);
        Self {
            id: submission.id.to_string(),
            evidence_request_id: submission.evidence_request_id.to_string(),
            submitted_by: EvidenceSubmitterDto {
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
struct EvidenceSubmitterDto {
    api_token_id: String,
    user_id: String,
}

#[derive(Debug, Serialize, JsonSchema)]
struct EvidenceAttachmentDto {
    id: String,
    evidence_submission_id: String,
    filename: String,
    content_type: String,
    content_length: i64,
    checksum_sha256: String,
    checksum_crc32c: String,
    upload_status: &'static str,
}

impl From<EvidenceAttachment> for EvidenceAttachmentDto {
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

#[derive(Debug, Serialize, JsonSchema)]
struct CreateAttachmentDownloadGrantResult {
    url: String,
    expires_at: String,
    filename: String,
    content_type: String,
    content_length: i64,
    url_secret_type: &'static str,
    intended_use: &'static str,
}

impl From<IssuedDownloadGrant> for CreateAttachmentDownloadGrantResult {
    fn from(grant: IssuedDownloadGrant) -> Self {
        Self {
            url: grant.url.to_string(),
            expires_at: format_datetime(grant.expires_at),
            filename: grant.filename,
            content_type: grant.content_type,
            content_length: grant.content_length,
            url_secret_type: "bearer_secret",
            intended_use: "human_presentation",
        }
    }
}

#[derive(Debug, Serialize, JsonSchema)]
struct ListControlsResult {
    controls: Vec<ControlDto>,
}

#[derive(Debug, Serialize, JsonSchema)]
struct ControlDto {
    id: String,
    workspace_id: String,
    code: String,
    title: String,
    description: String,
    framework_requirements: Vec<FrameworkRequirementDto>,
    created_at: String,
    updated_at: String,
}

impl From<Control> for ControlDto {
    fn from(control: Control) -> Self {
        Self {
            id: control.id.to_string(),
            workspace_id: control.workspace_id.to_string(),
            code: control.code,
            title: control.title,
            description: control.description,
            framework_requirements: control
                .framework_requirements
                .into_iter()
                .map(Into::into)
                .collect(),
            created_at: format_datetime(control.created_at),
            updated_at: format_datetime(control.updated_at),
        }
    }
}

#[derive(Debug, Serialize, JsonSchema)]
struct FrameworkRequirementDto {
    id: String,
    framework_id: String,
    code: String,
    title: String,
    description: String,
}

impl From<crate::domain::FrameworkRequirement> for FrameworkRequirementDto {
    fn from(requirement: crate::domain::FrameworkRequirement) -> Self {
        Self {
            id: requirement.id.to_string(),
            framework_id: requirement.framework_id.to_string(),
            code: requirement.code,
            title: requirement.title,
            description: requirement.description,
        }
    }
}

#[derive(Debug, Serialize, JsonSchema)]
struct ListEvidenceRequestControlMappingsResult {
    mappings: Vec<EvidenceRequestControlMappingDto>,
}

#[derive(Debug, Serialize, JsonSchema)]
struct EvidenceRequestControlMappingDto {
    evidence_request_id: String,
    control: ControlSummaryDto,
    rationale: String,
    created_at: String,
}

impl From<EvidenceRequestControlMapping> for EvidenceRequestControlMappingDto {
    fn from(mapping: EvidenceRequestControlMapping) -> Self {
        Self {
            evidence_request_id: mapping.evidence_request_id.to_string(),
            control: ControlSummaryDto {
                id: mapping.control.id.to_string(),
                code: mapping.control.code,
                title: mapping.control.title,
                description: mapping.control.description,
            },
            rationale: mapping.rationale,
            created_at: format_datetime(mapping.created_at),
        }
    }
}

#[derive(Debug, Serialize, JsonSchema)]
struct ControlSummaryDto {
    id: String,
    code: String,
    title: String,
    description: String,
}

#[derive(Debug, Serialize)]
struct FieldIssue {
    field: &'static str,
    message: &'static str,
}

fn authorize(
    ctx: &RequestContext<RoleServer>,
    workspace_id: WorkspaceId,
    permission: WorkspacePermission,
) -> Result<McpRequestContext, rmcp::ErrorData> {
    let parts = ctx
        .extensions
        .get::<http::request::Parts>()
        .ok_or_else(|| rmcp::ErrorData::internal_error("request context unavailable", None))?;

    McpRequestContext::authorize(&parts.extensions, &parts.headers, workspace_id, permission)
}

fn parse_evidence_request_args(
    args: EvidenceRequestArgs,
) -> Result<(WorkspaceId, EvidenceRequestId), rmcp::ErrorData> {
    let mut issues = Vec::new();
    let workspace_id =
        parse_uuid_issue("workspace_id", args.workspace_id, &mut issues).map(WorkspaceId::from);
    let evidence_request_id =
        parse_uuid_issue("evidence_request_id", args.evidence_request_id, &mut issues)
            .map(EvidenceRequestId::from);
    finish_args(issues)?;
    Ok((
        workspace_id.expect("validated"),
        evidence_request_id.expect("validated"),
    ))
}

fn parse_due_args(
    args: ListDueEvidenceRequestsArgs,
) -> Result<(WorkspaceId, Option<DateTime<Utc>>), rmcp::ErrorData> {
    let mut issues = Vec::new();
    let workspace_id =
        parse_uuid_issue("workspace_id", args.workspace_id, &mut issues).map(WorkspaceId::from);
    let now = match args.now {
        Some(value) => match DateTime::parse_from_rfc3339(&value) {
            Ok(parsed) => Some(parsed.with_timezone(&Utc)),
            Err(_) => {
                issues.push(FieldIssue {
                    field: "now",
                    message: "must be an RFC 3339 timestamp",
                });
                None
            }
        },
        None => None,
    };
    finish_args(issues)?;
    Ok((workspace_id.expect("validated"), now))
}

fn parse_submission_args(
    args: EvidenceSubmissionArgs,
) -> Result<(WorkspaceId, EvidenceSubmissionId), rmcp::ErrorData> {
    let mut issues = Vec::new();
    let workspace_id =
        parse_uuid_issue("workspace_id", args.workspace_id, &mut issues).map(WorkspaceId::from);
    let submission_id = parse_uuid_issue("submission_id", args.submission_id, &mut issues)
        .map(EvidenceSubmissionId::from);
    finish_args(issues)?;
    Ok((
        workspace_id.expect("validated"),
        submission_id.expect("validated"),
    ))
}

fn parse_download_grant_args(
    args: AttachmentDownloadGrantArgs,
) -> Result<
    (
        WorkspaceId,
        EvidenceSubmissionId,
        crate::domain::EvidenceAttachmentId,
    ),
    rmcp::ErrorData,
> {
    let mut issues = Vec::new();
    let workspace_id =
        parse_uuid_issue("workspace_id", args.workspace_id, &mut issues).map(WorkspaceId::from);
    let submission_id = parse_uuid_issue("submission_id", args.submission_id, &mut issues)
        .map(EvidenceSubmissionId::from);
    let attachment_id = parse_uuid_issue("attachment_id", args.attachment_id, &mut issues)
        .map(crate::domain::EvidenceAttachmentId::from);
    finish_args(issues)?;
    Ok((
        workspace_id.expect("validated"),
        submission_id.expect("validated"),
        attachment_id.expect("validated"),
    ))
}

fn parse_uuid_arg(field: &'static str, value: Option<String>) -> Result<Uuid, rmcp::ErrorData> {
    let mut issues = Vec::new();
    let id = parse_uuid_issue(field, value, &mut issues);
    finish_args(issues)?;
    Ok(id.expect("validated"))
}

fn parse_uuid_issue(
    field: &'static str,
    value: Option<String>,
    issues: &mut Vec<FieldIssue>,
) -> Option<Uuid> {
    match value {
        Some(value) => match Uuid::parse_str(&value) {
            Ok(id) => Some(id),
            Err(_) => {
                issues.push(FieldIssue {
                    field,
                    message: "must be a UUID",
                });
                None
            }
        },
        None => {
            issues.push(FieldIssue {
                field,
                message: "is required",
            });
            None
        }
    }
}

fn finish_args(issues: Vec<FieldIssue>) -> Result<(), rmcp::ErrorData> {
    if issues.is_empty() {
        return Ok(());
    }

    Err(rmcp::ErrorData::invalid_params(
        "tool argument validation failed",
        Some(json!({
            "problem": {
                "code": "validation_failed",
                "message": "tool argument validation failed",
                "field_issues": issues,
            }
        })),
    ))
}

fn not_found() -> rmcp::ErrorData {
    rmcp::ErrorData::resource_not_found(
        "resource not found",
        Some(json!({
            "problem": {
                "code": "not_found",
                "message": "resource not found",
            }
        })),
    )
}

fn conflict(code: &'static str, message: &'static str) -> rmcp::ErrorData {
    rmcp::ErrorData::new(
        ErrorCode(-32000),
        message,
        Some(json!({
            "problem": {
                "code": code,
                "message": message,
            }
        })),
    )
}

fn service_error(error: ServiceError) -> rmcp::ErrorData {
    tracing::error!(%error, "MCP service failure");
    rmcp::ErrorData::internal_error(
        "dependency failure",
        Some(json!({
            "problem": {
                "code": "dependency_failed",
                "message": "a dependency failed while handling the tool call",
            }
        })),
    )
}

fn download_error(error: DownloadError) -> rmcp::ErrorData {
    match error {
        DownloadError::NotFound => not_found(),
        DownloadError::NotReady => conflict(
            "attachment_not_ready",
            "attachment is not ready for download",
        ),
        DownloadError::MetadataMismatch | DownloadError::Internal => {
            tracing::error!(%error, "MCP attachment download failure");
            rmcp::ErrorData::internal_error(
                "internal error",
                Some(json!({
                    "problem": {
                        "code": "internal_error",
                        "message": "internal error",
                    }
                })),
            )
        }
        DownloadError::Repository(repository_error) => {
            tracing::error!(error = %repository_error, "MCP attachment download repository failure");
            rmcp::ErrorData::internal_error(
                "dependency failure",
                Some(json!({
                    "problem": {
                        "code": "dependency_failed",
                        "message": "a dependency failed while handling the tool call",
                    }
                })),
            )
        }
    }
}

fn format_datetime(value: DateTime<Utc>) -> String {
    value.to_rfc3339_opts(SecondsFormat::Millis, true)
}
