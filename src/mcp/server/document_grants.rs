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
    services::document_upload_grants::{IssuedUploadGrant, UploadGrantError},
    validate,
};

#[tool_router(router = document_grants_tool_router, vis = "pub(super)")]
impl ProofplaneMcp {
    #[tool(
        name = "manage_evidence_submission_document",
        description = "Create a short-lived bearer-secret browser URL for a human to upload or download an evidence submission’s documents; file bytes never pass through MCP; for guidance, call get_proofplane_guide with topic documents."
    )]
    async fn manage_evidence_submission_document(
        &self,
        ctx: RequestContext<RoleServer>,
        Parameters(args): Parameters<ManageEvidenceSubmissionDocumentRequest>,
    ) -> Result<Json<ManageEvidenceSubmissionDocumentResponse>, rmcp::ErrorData> {
        let submission_id = parse_document_grant_request(args)?;
        let context =
            authorize_token_workspace(&ctx, WorkspacePermission::WriteEvidenceSubmissions)?;
        let workspace_id = context.connection.workspace_id;
        let grant = self
            .document_upload_grants
            .issue(&context.agent_connection_context(), submission_id)
            .await
            .map_err(upload_grant_error)?;

        AuditEvent::new(
            "evidence_document_upload_grant.issued",
            AuditOutcome::Success,
            context.audit_actor(),
            AuditClientType::Mcp,
            "manage_evidence_submission_document",
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
struct ManageEvidenceSubmissionDocumentRequest {
    submission_id: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
struct ManageEvidenceSubmissionDocumentResponse {
    url: String,
    expires_at: String,
    submission_id: String,
    url_secret_type: &'static str,
    intended_use: &'static str,
}

impl From<IssuedUploadGrant> for ManageEvidenceSubmissionDocumentResponse {
    fn from(grant: IssuedUploadGrant) -> Self {
        Self {
            url: grant.url.to_string(),
            expires_at: format_datetime(grant.expires_at),
            submission_id: grant.submission_id.to_string(),
            url_secret_type: "bearer_secret",
            intended_use: "human_browser_document_management",
        }
    }
}

fn parse_document_grant_request(
    args: ManageEvidenceSubmissionDocumentRequest,
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
            tracing::error!(%error, "MCP document upload grant failure");
            rmcp::ErrorData::internal_error("internal error", None)
        }
        UploadGrantError::Repository(repository_error) => service_error(repository_error.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_document_grant_request, ManageEvidenceSubmissionDocumentRequest};
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
    fn document_grant_request_requires_submission_uuid() {
        let missing = parse_document_grant_request(ManageEvidenceSubmissionDocumentRequest {
            submission_id: None,
        })
        .expect_err("missing submission");
        assert_eq!(missing.code, ErrorCode::INVALID_PARAMS);
        assert_eq!(
            field_issues(&missing),
            [("submission_id".to_owned(), "is required".to_owned())]
        );

        let invalid = parse_document_grant_request(ManageEvidenceSubmissionDocumentRequest {
            submission_id: Some("nope".to_owned()),
        })
        .expect_err("invalid submission");
        assert_eq!(
            field_issues(&invalid),
            [("submission_id".to_owned(), "must be a UUID".to_owned())]
        );
    }
}
