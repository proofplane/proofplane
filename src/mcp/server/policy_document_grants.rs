use rmcp::{
    handler::server::wrapper::Parameters, schemars, schemars::JsonSchema, service::RequestContext,
    tool, tool_router, Json, RoleServer,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{
    common::{
        argument_errors, authorize_token_workspace, format_datetime, not_found, required_uuid,
        service_error,
    },
    ProofplaneMcp,
};
use crate::{
    domain::{PolicyId, WorkspacePermission},
    observability::audit::{AuditClientType, AuditEvent, AuditObject, AuditOutcome},
    services::policy_document_upload_grants::{IssuedPolicyUploadGrant, PolicyUploadGrantError},
    validate,
};

#[tool_router(router = policy_document_grants_tool_router, vis = "pub(super)")]
impl ProofplaneMcp {
    #[tool(
        name = "manage_policy_document",
        description = "Create a short-lived bearer-secret browser URL for a human to manage an active policy’s document; file bytes never pass through MCP; for guidance, call get_proofplane_guide with topic policies."
    )]
    async fn manage_policy_document(
        &self,
        ctx: RequestContext<RoleServer>,
        Parameters(args): Parameters<ManagePolicyDocumentRequest>,
    ) -> Result<Json<ManagePolicyDocumentResponse>, rmcp::ErrorData> {
        let policy_id = parse_policy_document_grant_request(args)?;
        let context = authorize_token_workspace(&ctx, WorkspacePermission::WriteControls)?;
        let grant = self
            .policy_document_upload_grants
            .issue(&context.agent_connection_context(), policy_id)
            .await
            .map_err(policy_upload_grant_error)?;

        AuditEvent::new(
            "policy_document_upload_grant.issued",
            AuditOutcome::Success,
            context.audit_actor(),
            AuditClientType::Mcp,
            "manage_policy_document",
        )
        .workspace_id(context.connection.workspace_id.into())
        .request_id(context.request_id.0)
        .metadata("policy_id", Uuid::from(grant.audit.policy_id))
        .object(AuditObject::new("policy", grant.audit.policy_id.into()))
        .emit();

        Ok(Json(grant.into()))
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ManagePolicyDocumentRequest {
    policy_id: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
struct ManagePolicyDocumentResponse {
    url: String,
    expires_at: String,
    policy_id: String,
    url_secret_type: &'static str,
    intended_use: &'static str,
}

impl From<IssuedPolicyUploadGrant> for ManagePolicyDocumentResponse {
    fn from(grant: IssuedPolicyUploadGrant) -> Self {
        Self {
            url: grant.url.to_string(),
            expires_at: format_datetime(grant.expires_at),
            policy_id: grant.policy_id.to_string(),
            url_secret_type: "bearer_secret",
            intended_use: "human_browser_document_management",
        }
    }
}

fn parse_policy_document_grant_request(
    args: ManagePolicyDocumentRequest,
) -> Result<PolicyId, rmcp::ErrorData> {
    validate! {
        policy_id <- required_uuid("policy_id", args.policy_id).map(PolicyId::from),
        => policy_id,
    }
    .into_result()
    .map_err(argument_errors)
}

fn policy_upload_grant_error(error: PolicyUploadGrantError) -> rmcp::ErrorData {
    match error {
        PolicyUploadGrantError::Unavailable => not_found(),
        PolicyUploadGrantError::Internal => {
            tracing::error!(%error, "MCP policy document upload grant failure");
            rmcp::ErrorData::internal_error("internal error", None)
        }
        PolicyUploadGrantError::Repository(repository_error) => {
            service_error(repository_error.into())
        }
    }
}
