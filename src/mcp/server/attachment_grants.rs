use uuid::Uuid;

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
    ProofplaneMcp,
};
use crate::{
    domain::{EvidenceSubmissionId, WorkspacePermission},
    observability::audit::{AuditClientType, AuditEvent, AuditObject, AuditOutcome},
    services::attachment_upload_grants::{IssuedUploadGrant, UploadGrantError},
    validate,
};

#[tool_router(router = attachment_grants_tool_router, vis = "pub(super)")]
impl ProofplaneMcp {
    #[tool(
        name = "manage_evidence_submission_attachment",
        description = "Create a short-lived human-browser URL to upload or download an evidence submission attachment."
    )]
    async fn manage_evidence_submission_attachment(
        &self,
        ctx: RequestContext<RoleServer>,
        Parameters(args): Parameters<ManageEvidenceSubmissionAttachmentRequest>,
    ) -> Result<Json<ManageEvidenceSubmissionAttachmentResponse>, rmcp::ErrorData> {
        let submission_id = parse_attachment_grant_request(args)?;
        let context =
            authorize_token_workspace(&ctx, WorkspacePermission::WriteEvidenceSubmissions)?;
        let workspace_id = context.token.workspace_id;
        let grant = if let Some(connection) = context.agent_connection_context() {
            self.attachment_upload_grants
                .issue_for_agent(&connection, submission_id)
                .await
        } else {
            self.attachment_upload_grants
                .issue(&context.token, submission_id)
                .await
        }
        .map_err(upload_grant_error)?;

        AuditEvent::new(
            "evidence_attachment_upload_grant.issued",
            AuditOutcome::Success,
            context.audit_actor(),
            AuditClientType::Mcp,
            "manage_evidence_submission_attachment",
        )
        .workspace_id(workspace_id.into())
        .request_id(context.request_id.0)
        .metadata(
            "evidence_submission_id",
            Uuid::from(grant.audit.submission_id),
        )
        .object(AuditObject::new(
            "evidence_submission",
            grant.audit.submission_id.into(),
        ))
        .emit();

        Ok(Json(grant.into()))
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ManageEvidenceSubmissionAttachmentRequest {
    submission_id: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
struct ManageEvidenceSubmissionAttachmentResponse {
    url: String,
    expires_at: String,
    submission_id: String,
    url_secret_type: &'static str,
    intended_use: &'static str,
}

impl From<IssuedUploadGrant> for ManageEvidenceSubmissionAttachmentResponse {
    fn from(grant: IssuedUploadGrant) -> Self {
        Self {
            url: grant.url.to_string(),
            expires_at: format_datetime(grant.expires_at),
            submission_id: grant.submission_id.to_string(),
            url_secret_type: "bearer_secret",
            intended_use: "human_browser_attachment_management",
        }
    }
}

fn parse_attachment_grant_request(
    args: ManageEvidenceSubmissionAttachmentRequest,
) -> Result<EvidenceSubmissionId, rmcp::ErrorData> {
    validate! {
        submission_id <- required_uuid("submission_id", args.submission_id)
            .map(EvidenceSubmissionId::from),
        => submission_id,
    }
    .into_result()
    .map_err(argument_errors)
}

fn upload_grant_error(error: UploadGrantError) -> rmcp::ErrorData {
    match error {
        UploadGrantError::Unavailable => not_found(),
        UploadGrantError::Internal => {
            tracing::error!(%error, "MCP attachment upload grant failure");
            rmcp::ErrorData::internal_error("internal error", None)
        }
        UploadGrantError::Repository(repository_error) => service_error(repository_error.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_attachment_grant_request, ManageEvidenceSubmissionAttachmentRequest};
    use rmcp::model::ErrorCode;

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
    fn attachment_grant_request_requires_submission_uuid() {
        let missing = parse_attachment_grant_request(ManageEvidenceSubmissionAttachmentRequest {
            submission_id: None,
        })
        .expect_err("missing submission");
        assert_eq!(missing.code, ErrorCode::INVALID_PARAMS);
        assert_eq!(
            field_issues(&missing),
            [("submission_id".to_owned(), "is required".to_owned())]
        );

        let invalid = parse_attachment_grant_request(ManageEvidenceSubmissionAttachmentRequest {
            submission_id: Some("nope".to_owned()),
        })
        .expect_err("invalid submission");
        assert_eq!(
            field_issues(&invalid),
            [("submission_id".to_owned(), "must be a UUID".to_owned())]
        );
    }
}
