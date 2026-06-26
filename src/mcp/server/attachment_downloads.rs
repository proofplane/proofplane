use rmcp::{
    handler::server::wrapper::Parameters, schemars, schemars::JsonSchema, service::RequestContext,
    tool, tool_router, Json, RoleServer,
};
use serde::{Deserialize, Serialize};

use super::{
    common::{argument_errors, authorize, download_error, format_datetime, required_uuid},
    ProofplaneMcp,
};
use crate::{
    domain::{EvidenceAttachmentId, EvidenceSubmissionId, WorkspaceId, WorkspacePermission},
    observability::audit::{AuditActor, AuditClientType, AuditEvent, AuditObject, AuditOutcome},
    services::attachment_downloads::IssuedDownloadGrant,
    validate,
};

#[tool_router(router = attachment_downloads_tool_router, vis = "pub(super)")]
impl ProofplaneMcp {
    #[tool(
        name = "create_attachment_download_grant",
        description = "Create a short-lived human-use download URL for a finalized attachment."
    )]
    async fn create_attachment_download_grant(
        &self,
        ctx: RequestContext<RoleServer>,
        Parameters(args): Parameters<CreateAttachmentDownloadGrantRequest>,
    ) -> Result<Json<CreateAttachmentDownloadGrantResponse>, rmcp::ErrorData> {
        let (workspace_id, submission_id, attachment_id) =
            parse_attachment_download_grant_request(args)?;
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

        AuditEvent::new(
            "evidence_attachment_download_grant.issued",
            AuditOutcome::Success,
            AuditActor::ApiToken {
                user_id: context.token.user_id.into(),
                api_token_id: context.token.api_token_id.into(),
            },
            AuditClientType::Mcp,
            "create_attachment_download_grant",
        )
        .workspace_id(workspace_id.into())
        .request_id(context.request_id.0)
        .evidence_submission_id(grant.audit.submission_id.into())
        .evidence_attachment_id(grant.audit.attachment_id.into())
        .object(AuditObject::new(
            "evidence_attachment",
            grant.audit.attachment_id.into(),
        ))
        .emit();

        Ok(Json(grant.into()))
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
struct CreateAttachmentDownloadGrantRequest {
    workspace_id: Option<String>,
    submission_id: Option<String>,
    attachment_id: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
struct CreateAttachmentDownloadGrantResponse {
    url: String,
    expires_at: String,
    filename: String,
    content_type: String,
    content_length: i64,
    url_secret_type: &'static str,
    intended_use: &'static str,
}

impl From<IssuedDownloadGrant> for CreateAttachmentDownloadGrantResponse {
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

fn parse_attachment_download_grant_request(
    args: CreateAttachmentDownloadGrantRequest,
) -> Result<(WorkspaceId, EvidenceSubmissionId, EvidenceAttachmentId), rmcp::ErrorData> {
    validate! {
        workspace_id <- required_uuid("workspace_id", args.workspace_id).map(WorkspaceId::from),
        submission_id <- required_uuid("submission_id", args.submission_id)
            .map(EvidenceSubmissionId::from),
        attachment_id <- required_uuid("attachment_id", args.attachment_id)
            .map(EvidenceAttachmentId::from),
        => (workspace_id, submission_id, attachment_id),
    }
    .into_result()
    .map_err(argument_errors)
}

#[cfg(test)]
mod tests {
    use super::{parse_attachment_download_grant_request, CreateAttachmentDownloadGrantRequest};
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
    fn download_grant_request_accumulates_multiple_invalid_uuid_fields() {
        let error = parse_attachment_download_grant_request(CreateAttachmentDownloadGrantRequest {
            workspace_id: Some("not-workspace".to_owned()),
            submission_id: Some("not-submission".to_owned()),
            attachment_id: Some("not-attachment".to_owned()),
        })
        .expect_err("invalid args");

        assert_eq!(error.code, ErrorCode::INVALID_PARAMS);
        assert_eq!(error.message, "tool argument validation failed");
        assert_eq!(
            field_issues(&error),
            [
                ("workspace_id".to_owned(), "must be a UUID".to_owned()),
                ("submission_id".to_owned(), "must be a UUID".to_owned()),
                ("attachment_id".to_owned(), "must be a UUID".to_owned()),
            ]
        );
    }

    #[test]
    fn missing_required_uuid_fields_map_to_required_message() {
        let error = parse_attachment_download_grant_request(CreateAttachmentDownloadGrantRequest {
            workspace_id: None,
            submission_id: None,
            attachment_id: None,
        })
        .expect_err("missing args");

        assert_eq!(
            field_issues(&error),
            [
                ("workspace_id".to_owned(), "is required".to_owned()),
                ("submission_id".to_owned(), "is required".to_owned()),
                ("attachment_id".to_owned(), "is required".to_owned()),
            ]
        );
    }
}
