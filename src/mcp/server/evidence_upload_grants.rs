use uuid::Uuid;

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
    ProofplaneMcp,
};
use crate::{
    domain::{CoverageWindow, EvidenceId, WorkspacePermission},
    observability::audit::{AuditClientType, AuditEvent, AuditObject, AuditOutcome},
    services::evidence_upload_grants::{IssuedUploadGrant, UploadGrantError},
    validate,
};

#[tool_router(router = evidence_upload_grants_tool_router, vis = "pub(super)")]
impl ProofplaneMcp {
    #[tool(
        name = "manage_evidence_submissions",
        description = "Create a short-lived bearer-secret browser URL for a human to upload files covering one period of evidence; file bytes never pass through MCP; for guidance, call get_proofplane_guide with topic submitting-evidence."
    )]
    async fn manage_evidence_submissions(
        &self,
        ctx: RequestContext<RoleServer>,
        Parameters(args): Parameters<ManageEvidenceSubmissionsArgs>,
    ) -> Result<Json<ManageEvidenceSubmissionsResponse>, rmcp::ErrorData> {
        let (evidence_id, coverage) = parse_manage_evidence_submissions_args(args)?;
        let context =
            authorize_token_workspace(&ctx, WorkspacePermission::WriteEvidenceSubmissions)?;
        let workspace_id = context.connection.workspace_id;
        let grant = self
            .evidence_upload_grants
            .issue(&context.agent_connection_context(), evidence_id, coverage)
            .await
            .map_err(upload_grant_error)?;

        AuditEvent::new(
            "evidence_upload_grant.issued",
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
struct ManageEvidenceSubmissionsArgs {
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

fn parse_manage_evidence_submissions_args(
    args: ManageEvidenceSubmissionsArgs,
) -> Result<(EvidenceId, CoverageWindow), rmcp::ErrorData> {
    let (evidence_id, valid_from, valid_until) = validate! {
        evidence_id <- required_uuid("evidence_id", args.evidence_id)
            .map(EvidenceId::from),
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

fn upload_grant_error(error: UploadGrantError) -> rmcp::ErrorData {
    match error {
        UploadGrantError::Unavailable => not_found(),
        UploadGrantError::Internal => {
            tracing::error!(%error, "MCP evidence upload grant failure");
            rmcp::ErrorData::internal_error("internal error", None)
        }
        UploadGrantError::Repository(repository_error) => service_error(repository_error.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_manage_evidence_submissions_args, ManageEvidenceSubmissionsArgs};
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

    fn args() -> ManageEvidenceSubmissionsArgs {
        ManageEvidenceSubmissionsArgs {
            evidence_id: Some("00000000-0000-4000-8000-000000000001".to_owned()),
            valid_from: Some("2026-01-01T00:00:00Z".to_owned()),
            valid_until: Some("2026-03-31T00:00:00Z".to_owned()),
        }
    }

    #[test]
    fn accepts_evidence_and_coverage_window() {
        let (evidence_id, coverage) =
            parse_manage_evidence_submissions_args(args()).expect("valid args");

        assert_eq!(
            evidence_id.to_string(),
            "00000000-0000-4000-8000-000000000001"
        );
        assert!(coverage.valid_until > coverage.valid_from);
    }

    #[test]
    fn requires_evidence_and_coverage_arguments() {
        let missing = parse_manage_evidence_submissions_args(ManageEvidenceSubmissionsArgs {
            evidence_id: None,
            valid_from: None,
            valid_until: None,
        })
        .expect_err("missing args");

        assert_eq!(missing.code, ErrorCode::INVALID_PARAMS);
        assert_eq!(
            field_issues(&missing),
            [
                ("evidence_id".to_owned(), "is required".to_owned()),
                ("valid_from".to_owned(), "is required".to_owned()),
                ("valid_until".to_owned(), "is required".to_owned()),
            ]
        );
    }

    #[test]
    fn rejects_coverage_window_that_ends_before_it_starts() {
        let mut inverted = args();
        inverted.valid_from = Some("2026-03-31T00:00:00Z".to_owned());
        inverted.valid_until = Some("2026-01-01T00:00:00Z".to_owned());

        let error = parse_manage_evidence_submissions_args(inverted).expect_err("inverted window");

        assert_eq!(
            field_issues(&error),
            [(
                "valid_until".to_owned(),
                "valid_until must be greater than or equal to valid_from".to_owned()
            )]
        );
    }
}
