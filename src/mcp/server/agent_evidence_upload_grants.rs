use rmcp::{
    handler::server::wrapper::Parameters,
    schemars::{self, JsonSchema},
    service::RequestContext,
    tool, tool_router, ErrorData, Json, RoleServer,
};
use secrecy::ExposeSecret;
use serde::{Deserialize, Serialize};

use super::{
    common::{
        argument_errors, authorize_token_workspace, domain_errors, format_datetime, not_found,
        required_arg, required_non_negative_u64, required_timestamp, required_uuid,
    },
    ProofplaneMcp,
};
use crate::{
    domain::{AgentEvidenceUploadDeclaration, CoverageWindow, EvidenceId, WorkspacePermission},
    services::{
        agent_evidence_upload_grants::{
            AgentEvidenceUploadGrantError, IssuedAgentEvidenceUploadGrant,
        },
        Error as ServiceError,
    },
    validate,
    validation::Validation,
};

const AUTHORIZATION_SCHEME: &str = "Proofplane-Upload";

#[tool_router(router = agent_evidence_upload_grants_tool_router, vis = "pub(super)")]
impl ProofplaneMcp {
    #[tool(
        name = "prepare_evidence_submission_upload",
        description = "Prepare a short-lived bearer-secret HTTP PUT descriptor for a trusted runtime to transfer one evidence file without sending file bytes through MCP; for guidance, call get_proofplane_guide with topic submitting-evidence."
    )]
    async fn prepare_evidence_submission_upload(
        &self,
        ctx: RequestContext<RoleServer>,
        Parameters(args): Parameters<PrepareEvidenceSubmissionUploadRequest>,
    ) -> Result<Json<PrepareEvidenceSubmissionUploadResponse>, ErrorData> {
        let (evidence_id, coverage, declaration) =
            parse_prepare_upload_request(args, self.max_document_bytes)?;
        let context =
            authorize_token_workspace(&ctx, WorkspacePermission::WriteEvidenceSubmissions)?;
        let issued = self
            .agent_evidence_upload_grants
            .issue(
                &context.agent_connection_context(),
                evidence_id,
                coverage,
                declaration,
            )
            .await?;
        let url = self
            .public_api_base_url
            .join(&format!("agent-evidence-uploads/{}", issued.grant.id()))
            .map_err(|error| {
                tracing::error!(%error, "MCP agent evidence upload URL construction failed");
                ErrorData::internal_error("internal error", None)
            })?;

        Ok(Json(PrepareEvidenceSubmissionUploadResponse::new(
            issued,
            url,
            self.max_document_bytes,
        )))
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct PrepareEvidenceSubmissionUploadRequest {
    #[schemars(required)]
    evidence_id: Option<String>,
    #[schemars(required)]
    valid_from: Option<String>,
    #[schemars(required)]
    valid_until: Option<String>,
    #[schemars(required)]
    filename: Option<String>,
    #[schemars(required)]
    content_type: Option<String>,
    #[schemars(required)]
    content_length: Option<i64>,
    checksum_sha256: Option<String>,
}

#[derive(Serialize, JsonSchema)]
struct PrepareEvidenceSubmissionUploadResponse {
    upload_id: String,
    submission_id: String,
    upload: EvidenceSubmissionUploadDescriptor,
}

#[derive(Serialize, JsonSchema)]
struct EvidenceSubmissionUploadDescriptor {
    method: &'static str,
    url: String,
    authorization: String,
    content_type: String,
    expires_at: String,
    max_bytes: u64,
}

impl PrepareEvidenceSubmissionUploadResponse {
    fn new(issued: IssuedAgentEvidenceUploadGrant, url: url::Url, max_bytes: u64) -> Self {
        Self {
            upload_id: issued.grant.id().to_string(),
            submission_id: issued.grant.submission_id().to_string(),
            upload: EvidenceSubmissionUploadDescriptor {
                method: "PUT",
                url: url.to_string(),
                authorization: format!(
                    "{AUTHORIZATION_SCHEME} {}",
                    issued.credential.expose_secret()
                ),
                content_type: issued.grant.declaration().content_type().to_owned(),
                expires_at: format_datetime(issued.grant.expires_at()),
                max_bytes,
            },
        }
    }
}

fn parse_prepare_upload_request(
    args: PrepareEvidenceSubmissionUploadRequest,
    max_bytes: u64,
) -> Result<(EvidenceId, CoverageWindow, AgentEvidenceUploadDeclaration), ErrorData> {
    let (evidence_id, valid_from, valid_until, filename, content_type, content_length) =
        validate! {
            evidence_id <- required_uuid("evidence_id", args.evidence_id).map(EvidenceId::from),
            valid_from <- required_timestamp("valid_from", args.valid_from),
            valid_until <- required_timestamp("valid_until", args.valid_until),
            filename <- required_arg("filename", args.filename),
            content_type <- required_arg("content_type", args.content_type),
            content_length <- required_non_negative_u64("content_length", args.content_length),
            => (evidence_id, valid_from, valid_until, filename, content_type, content_length),
        }
        .into_result()
        .map_err(argument_errors)?;

    validate! {
        coverage <- result_validation(CoverageWindow::new(valid_from, valid_until)),
        declaration <- AgentEvidenceUploadDeclaration::new(
            filename,
            content_type,
            content_length,
            args.checksum_sha256,
            max_bytes,
        ),
        => (evidence_id, coverage, declaration),
    }
    .into_result()
    .map_err(domain_errors)
}

fn result_validation<T>(
    result: Result<T, crate::domain::DomainError>,
) -> Validation<T, crate::domain::DomainError> {
    match result {
        Ok(value) => Validation::valid(value),
        Err(error) => Validation::invalid(error),
    }
}

impl From<AgentEvidenceUploadGrantError> for ErrorData {
    fn from(error: AgentEvidenceUploadGrantError) -> Self {
        match error {
            AgentEvidenceUploadGrantError::Unavailable => not_found(),
            AgentEvidenceUploadGrantError::Internal => {
                tracing::error!(%error, "MCP agent evidence upload grant failure");
                ErrorData::internal_error("internal error", None)
            }
            AgentEvidenceUploadGrantError::Repository(repository_error) => {
                ServiceError::from(repository_error).into()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use rmcp::ErrorData;
    use uuid::Uuid;

    use super::{parse_prepare_upload_request, PrepareEvidenceSubmissionUploadRequest};

    fn request() -> PrepareEvidenceSubmissionUploadRequest {
        PrepareEvidenceSubmissionUploadRequest {
            evidence_id: Some(Uuid::new_v4().to_string()),
            valid_from: Some("2026-01-01T00:00:00Z".to_owned()),
            valid_until: Some("2026-03-31T23:59:59Z".to_owned()),
            filename: Some("access-review.pdf".to_owned()),
            content_type: Some("application/pdf".to_owned()),
            content_length: Some(0),
            checksum_sha256: None,
        }
    }

    fn field_issues(error: &ErrorData) -> Vec<(&str, &str)> {
        error.data.as_ref().expect("error data")["problem"]["field_issues"]
            .as_array()
            .expect("field issues")
            .iter()
            .map(|issue| {
                (
                    issue["field"].as_str().expect("field"),
                    issue["message"].as_str().expect("message"),
                )
            })
            .collect()
    }

    #[test]
    fn preparation_request_accepts_zero_length_and_optional_checksum() {
        let (_, coverage, declaration) =
            parse_prepare_upload_request(request(), 1024).expect("request is valid");

        assert!(coverage.valid_until > coverage.valid_from);
        assert_eq!(declaration.expected_content_length(), 0);
        assert!(declaration.expected_sha256().is_none());
    }

    #[test]
    fn preparation_request_reports_all_missing_required_fields() {
        let error = parse_prepare_upload_request(
            PrepareEvidenceSubmissionUploadRequest {
                evidence_id: None,
                valid_from: None,
                valid_until: None,
                filename: None,
                content_type: None,
                content_length: None,
                checksum_sha256: None,
            },
            1024,
        )
        .expect_err("missing fields fail");

        assert_eq!(
            field_issues(&error),
            [
                ("evidence_id", "is required"),
                ("valid_from", "is required"),
                ("valid_until", "is required"),
                ("filename", "is required"),
                ("content_type", "is required"),
                ("content_length", "is required"),
            ]
        );
    }

    #[test]
    fn preparation_request_accumulates_domain_validation_failures() {
        let mut invalid = request();
        invalid.valid_from = Some("2026-04-01T00:00:00Z".to_owned());
        invalid.filename = Some("../secret.pdf".to_owned());
        invalid.content_type = Some("not a media type".to_owned());
        invalid.content_length = Some(5);
        invalid.checksum_sha256 = Some("A".repeat(64));

        let error =
            parse_prepare_upload_request(invalid, 4).expect_err("invalid domain declaration fails");
        assert_eq!(
            field_issues(&error)
                .into_iter()
                .map(|(field, _)| field)
                .collect::<Vec<_>>(),
            [
                "valid_until",
                "filename",
                "content_type",
                "content_length",
                "checksum_sha256",
            ]
        );
    }
}
