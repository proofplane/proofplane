use uuid::Uuid;

use rmcp::{
    handler::server::wrapper::Parameters, schemars, schemars::JsonSchema, service::RequestContext,
    tool, tool_router, Json, RoleServer,
};
use serde::{Deserialize, Serialize};

use super::{
    common::{
        argument_errors, authorize_token_workspace, domain_errors, format_datetime, not_found,
        required_timestamp, required_uuid,
    },
    ProofplaneMcp,
};
use crate::{
    domain::{CoverageWindow, EvidenceId, WorkspacePermission},
    observability::audit::{AuditClientType, AuditEvent, AuditObject, AuditOutcome},
    services::{
        document_upload_grants::{IssuedUploadGrant, UploadGrantError},
        Error as ServiceError,
    },
    validate,
};

#[tool_router(router = document_grants_tool_router, vis = "pub(super)")]
impl ProofplaneMcp {
    #[tool(
        name = "manage_evidence_submissions",
        description = "Create a short-lived browser URL for a human to upload files as evidence submissions for a coverage window; each file becomes one submission; for guidance, call get_proofplane_guide with topic submitting-evidence."
    )]
    async fn manage_evidence_submissions(
        &self,
        ctx: RequestContext<RoleServer>,
        Parameters(args): Parameters<ManageEvidenceSubmissionsRequest>,
    ) -> Result<Json<ManageEvidenceSubmissionsResponse>, rmcp::ErrorData> {
        let (evidence_id, coverage) = parse_manage_submissions_request(args)?;
        let context =
            authorize_token_workspace(&ctx, WorkspacePermission::WriteEvidenceSubmissions)?;
        let workspace_id = context.connection.workspace_id;
        let grant = self
            .document_upload_grants
            .issue(&context.agent_connection_context(), evidence_id, coverage)
            .await?;

        AuditEvent::new(
            "evidence_document_upload_grant.issued",
            AuditOutcome::Success,
            context.audit_actor(),
            AuditClientType::Mcp,
            "manage_evidence_submissions",
        )
        .workspace_id(workspace_id.into())
        .request_id(context.request_id.0)
        .metadata("evidence_id", Uuid::from(grant.audit.evidence_id))
        .object(AuditObject::new("evidence", grant.audit.evidence_id.into()))
        .emit();

        Ok(Json(grant.into()))
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ManageEvidenceSubmissionsRequest {
    evidence_id: Option<String>,
    valid_from: Option<String>,
    valid_until: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
struct ManageEvidenceSubmissionsResponse {
    url: String,
    expires_at: String,
    evidence_id: String,
    valid_from: String,
    valid_until: String,
    url_secret_type: &'static str,
    intended_use: &'static str,
}

impl From<IssuedUploadGrant> for ManageEvidenceSubmissionsResponse {
    fn from(grant: IssuedUploadGrant) -> Self {
        Self {
            url: grant.url.to_string(),
            expires_at: format_datetime(grant.expires_at),
            evidence_id: grant.evidence_id.to_string(),
            valid_from: format_datetime(grant.coverage.valid_from),
            valid_until: format_datetime(grant.coverage.valid_until),
            url_secret_type: "bearer_secret",
            intended_use: "human_browser_evidence_upload",
        }
    }
}

fn parse_manage_submissions_request(
    args: ManageEvidenceSubmissionsRequest,
) -> Result<(EvidenceId, CoverageWindow), rmcp::ErrorData> {
    let (evidence_id, valid_from, valid_until) = validate! {
        evidence_id <- required_uuid("evidence_id", args.evidence_id).map(EvidenceId::from),
        valid_from <- required_timestamp("valid_from", args.valid_from),
        valid_until <- required_timestamp("valid_until", args.valid_until),
        => (evidence_id, valid_from, valid_until),
    }
    .into_result()
    .map_err(argument_errors)?;

    let coverage =
        CoverageWindow::new(valid_from, valid_until).map_err(|error| domain_errors(vec![error]))?;

    Ok((evidence_id, coverage))
}

impl From<UploadGrantError> for rmcp::ErrorData {
    fn from(error: UploadGrantError) -> Self {
        match error {
            UploadGrantError::Unavailable => not_found(),
            UploadGrantError::Internal => {
                tracing::error!(%error, "MCP document upload grant failure");
                rmcp::ErrorData::internal_error("internal error", None)
            }
            UploadGrantError::Repository(repository_error) => {
                ServiceError::from(repository_error).into()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_manage_submissions_request, ManageEvidenceSubmissionsRequest};
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
    fn manage_submissions_request_requires_evidence_uuid() {
        let missing = parse_manage_submissions_request(ManageEvidenceSubmissionsRequest {
            evidence_id: None,
            valid_from: Some("2026-01-01T00:00:00Z".to_owned()),
            valid_until: Some("2026-03-31T00:00:00Z".to_owned()),
        })
        .expect_err("missing evidence");
        assert_eq!(missing.code, ErrorCode::INVALID_PARAMS);
        assert_eq!(
            field_issues(&missing),
            [("evidence_id".to_owned(), "is required".to_owned())]
        );

        let invalid = parse_manage_submissions_request(ManageEvidenceSubmissionsRequest {
            evidence_id: Some("nope".to_owned()),
            valid_from: Some("2026-01-01T00:00:00Z".to_owned()),
            valid_until: Some("2026-03-31T00:00:00Z".to_owned()),
        })
        .expect_err("invalid evidence");
        assert_eq!(
            field_issues(&invalid),
            [("evidence_id".to_owned(), "must be a UUID".to_owned())]
        );
    }
}
