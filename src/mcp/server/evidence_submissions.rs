use rmcp::{
    handler::server::wrapper::Parameters, schemars, schemars::JsonSchema, service::RequestContext,
    tool, tool_router, Json, RoleServer,
};
use serde::{Deserialize, Serialize};

use super::{
    common::{
        argument_errors, authorize_token_workspace, format_datetime, not_found, required_uuid,
        service_error,
    },
    evidence::{parse_evidence_args, EvidenceArgs},
    ProofplaneMcp,
};
use crate::{
    domain::{EvidenceSubmission, EvidenceSubmissionId, EvidenceSubmitter, WorkspacePermission},
    validate,
};

#[tool_router(router = evidence_submissions_tool_router, vis = "pub(super)")]
impl ProofplaneMcp {
    #[tool(
        name = "get_evidence_submission",
        description = "Get one evidence submission with its file metadata, coverage window, provenance, and upload status by submission ID; for guidance, call get_proofplane_guide with topic submitting-evidence."
    )]
    async fn get_evidence_submission(
        &self,
        ctx: RequestContext<RoleServer>,
        Parameters(args): Parameters<GetEvidenceSubmissionArgs>,
    ) -> Result<Json<GetEvidenceSubmissionResponse>, rmcp::ErrorData> {
        let submission_id = parse_evidence_submission_args(args)?;
        let context =
            authorize_token_workspace(&ctx, WorkspacePermission::ReadEvidenceSubmissions)?;
        let submission = self
            .evidence_submissions
            .get(context.agent_connection_context(), submission_id)
            .await
            .map_err(service_error)?
            .ok_or_else(not_found)?;

        Ok(Json(GetEvidenceSubmissionResponse {
            submission: submission.into(),
        }))
    }

    #[tool(
        name = "list_evidence_submissions",
        description = "List the submissions filed for one piece of evidence, newest first, with coverage windows and upload status; for guidance, call get_proofplane_guide with topic submitting-evidence."
    )]
    async fn list_evidence_submissions(
        &self,
        ctx: RequestContext<RoleServer>,
        Parameters(args): Parameters<EvidenceArgs>,
    ) -> Result<Json<ListEvidenceSubmissionsResponse>, rmcp::ErrorData> {
        let evidence_id = parse_evidence_args(args)?;
        let context =
            authorize_token_workspace(&ctx, WorkspacePermission::ReadEvidenceSubmissions)?;
        let submissions = self
            .evidence_submissions
            .list_for_evidence(context.agent_connection_context(), evidence_id)
            .await
            .map_err(service_error)?
            .ok_or_else(not_found)?;

        Ok(Json(ListEvidenceSubmissionsResponse {
            submissions: submissions.into_iter().map(Into::into).collect(),
        }))
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(super) struct GetEvidenceSubmissionArgs {
    pub(super) submission_id: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
struct GetEvidenceSubmissionResponse {
    submission: EvidenceSubmissionResponseDTO,
}

#[derive(Debug, Serialize, JsonSchema)]
struct ListEvidenceSubmissionsResponse {
    submissions: Vec<EvidenceSubmissionResponseDTO>,
}

#[derive(Debug, Serialize, JsonSchema)]
struct EvidenceSubmissionResponseDTO {
    id: String,
    evidence_id: String,
    submitted_by: EvidenceSubmitterResponseDTO,
    received_at: String,
    valid_from: String,
    valid_until: String,
    filename: String,
    content_type: String,
    content_length: i64,
    checksum_sha256: String,
    checksum_crc32c: String,
    upload_status: &'static str,
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
            filename: submission.filename,
            content_type: submission.content_type,
            content_length: submission.content_length,
            checksum_sha256: submission.checksum_sha256,
            checksum_crc32c: submission.checksum_crc32c,
            upload_status: submission.upload_status.as_str(),
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

pub(super) fn parse_evidence_submission_args(
    args: GetEvidenceSubmissionArgs,
) -> Result<EvidenceSubmissionId, rmcp::ErrorData> {
    validate! {
        submission_id <- required_uuid("submission_id", args.submission_id)
            .map(EvidenceSubmissionId::from),
        => submission_id,
    }
    .into_result()
    .map_err(argument_errors)
}

#[cfg(test)]
mod tests {
    use rmcp::model::ErrorCode;

    use super::{parse_evidence_submission_args, GetEvidenceSubmissionArgs};

    fn field_issues(error: &rmcp::ErrorData) -> Vec<(String, String)> {
        error.data.as_ref().expect("error data")["problem"]["field_issues"]
            .as_array()
            .expect("field issues")
            .iter()
            .map(|issue| {
                (
                    issue["field"].as_str().expect("field").to_owned(),
                    issue["message"].as_str().expect("message").to_owned(),
                )
            })
            .collect()
    }

    #[test]
    fn submission_args_require_a_uuid() {
        let missing = parse_evidence_submission_args(GetEvidenceSubmissionArgs {
            submission_id: None,
        })
        .expect_err("missing submission");
        assert_eq!(missing.code, ErrorCode::INVALID_PARAMS);
        assert_eq!(
            field_issues(&missing),
            [("submission_id".to_owned(), "is required".to_owned())]
        );

        let invalid = parse_evidence_submission_args(GetEvidenceSubmissionArgs {
            submission_id: Some("nope".to_owned()),
        })
        .expect_err("invalid submission");
        assert_eq!(
            field_issues(&invalid),
            [("submission_id".to_owned(), "must be a UUID".to_owned())]
        );
    }
}
