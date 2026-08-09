use rmcp::{
    handler::server::wrapper::Parameters,
    schemars::{self, JsonSchema},
    service::RequestContext,
    tool, tool_router, ErrorData, Json, RoleServer,
};
use serde::{Deserialize, Serialize};

use super::{
    common::{
        argument_errors, authorize_token_workspace, format_datetime, not_found, required_uuid,
    },
    evidence::{parse_evidence_arg, EvidenceArg},
    ProofplaneMcp,
};
use crate::{
    domain::{
        Document, EvidenceSubmission, EvidenceSubmissionId, EvidenceSubmitter, WorkspacePermission,
    },
    projections::EvidenceSubmissionDetail,
    validate,
};

#[tool_router(router = evidence_submissions_tool_router, vis = "pub(super)")]
impl ProofplaneMcp {
    #[tool(
        name = "list_evidence_submissions",
        description = "List the submissions for a piece of evidence, each one file with its coverage window, provenance, and document metadata; for guidance, call get_proofplane_guide with topic submitting-evidence."
    )]
    async fn list_evidence_submissions(
        &self,
        ctx: RequestContext<RoleServer>,
        Parameters(args): Parameters<EvidenceArg>,
    ) -> Result<Json<ListEvidenceSubmissionsResponse>, ErrorData> {
        let evidence_id = parse_evidence_arg(args)?;
        let context =
            authorize_token_workspace(&ctx, WorkspacePermission::ReadEvidenceSubmissions)?;
        let submissions = self
            .evidence_submissions
            .list_for_evidence(context.agent_connection_context(), evidence_id)
            .await?;

        Ok(Json(ListEvidenceSubmissionsResponse {
            submissions: submissions
                .into_iter()
                .map(EvidenceSubmissionResponse::from_detail)
                .collect(),
        }))
    }

    #[tool(
        name = "get_evidence_submission",
        description = "Get one evidence submission with its coverage window, provenance, and document metadata by submission ID; for guidance, call get_proofplane_guide with topic submitting-evidence."
    )]
    async fn get_evidence_submission(
        &self,
        ctx: RequestContext<RoleServer>,
        Parameters(args): Parameters<GetEvidenceSubmissionRequest>,
    ) -> Result<Json<EvidenceSubmissionResponse>, ErrorData> {
        let submission_id = parse_evidence_submission_request(args)?;
        let context =
            authorize_token_workspace(&ctx, WorkspacePermission::ReadEvidenceSubmissions)?;
        let detail = self
            .evidence_submissions
            .get(context.agent_connection_context(), submission_id)
            .await?
            .ok_or_else(not_found)?;

        Ok(Json(EvidenceSubmissionResponse::from_detail(detail)))
    }

    #[tool(
        name = "get_latest_evidence_submission",
        description = "Get the latest submission for a piece of evidence with its coverage window, provenance, and document metadata; for guidance, call get_proofplane_guide with topic submitting-evidence."
    )]
    async fn get_latest_evidence_submission(
        &self,
        ctx: RequestContext<RoleServer>,
        Parameters(args): Parameters<EvidenceArg>,
    ) -> Result<Json<EvidenceSubmissionResponse>, ErrorData> {
        let evidence_id = parse_evidence_arg(args)?;
        let context =
            authorize_token_workspace(&ctx, WorkspacePermission::ReadEvidenceSubmissions)?;
        let detail = self
            .evidence_submissions
            .latest_for_evidence(context.agent_connection_context(), evidence_id)
            .await?
            .ok_or_else(not_found)?;

        Ok(Json(EvidenceSubmissionResponse::from_detail(detail)))
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(super) struct GetEvidenceSubmissionRequest {
    pub(super) submission_id: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
struct ListEvidenceSubmissionsResponse {
    submissions: Vec<EvidenceSubmissionResponse>,
}

#[derive(Debug, Serialize, JsonSchema)]
struct EvidenceSubmissionResponse {
    submission: EvidenceSubmissionResponseDTO,
    document: EvidenceDocumentResponseDTO,
}

impl EvidenceSubmissionResponse {
    fn from_detail(detail: EvidenceSubmissionDetail) -> Self {
        Self {
            submission: detail.submission.into(),
            document: detail.document.into(),
        }
    }
}

#[derive(Debug, Serialize, JsonSchema)]
struct EvidenceSubmissionResponseDTO {
    id: String,
    evidence_id: String,
    submitted_by: EvidenceSubmitterResponseDTO,
    received_at: String,
    valid_from: String,
    valid_until: String,
}

impl From<EvidenceSubmission> for EvidenceSubmissionResponseDTO {
    fn from(submission: EvidenceSubmission) -> Self {
        Self {
            id: submission.id.to_string(),
            evidence_id: submission.evidence_id.to_string(),
            submitted_by: EvidenceSubmitterResponseDTO::from(submission.submitted_by),
            received_at: format_datetime(submission.received_at),
            valid_from: format_datetime(submission.valid_from),
            valid_until: format_datetime(submission.valid_until),
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
struct EvidenceDocumentResponseDTO {
    id: String,
    evidence_submission_id: String,
    created_by_user_id: String,
    filename: String,
    content_type: String,
    content_length: i64,
    checksum_sha256: String,
    checksum_crc32c: String,
    upload_status: &'static str,
}

impl From<Document> for EvidenceDocumentResponseDTO {
    fn from(document: Document) -> Self {
        Self {
            id: document.id().to_string(),
            evidence_submission_id: document.owner().owner_uuid().to_string(),
            created_by_user_id: document.created_by_user_id.to_string(),
            filename: document.filename,
            content_type: document.content_type,
            content_length: document.content_length,
            checksum_sha256: document.checksum_sha256,
            checksum_crc32c: document.checksum_crc32c,
            upload_status: document.upload_status.as_str(),
        }
    }
}

pub(super) fn parse_evidence_submission_request(
    args: GetEvidenceSubmissionRequest,
) -> Result<EvidenceSubmissionId, ErrorData> {
    validate! {
        submission_id <- required_uuid("submission_id", args.submission_id)
            .map(EvidenceSubmissionId::from),
        => submission_id,
    }
    .into_result()
    .map_err(argument_errors)
}
