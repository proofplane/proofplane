use rmcp::{
    handler::server::wrapper::Parameters,
    schemars::{self, JsonSchema},
    service::RequestContext,
    tool, tool_router, ErrorData, Json, RoleServer,
};
use secrecy::ExposeSecret;
use serde::{Deserialize, Serialize};
use url::Url;
use uuid::Uuid;

use super::{
    common::{
        argument_errors, authorize_token_workspace, domain_errors, format_datetime, invalid_field,
        not_found, optional_timestamp, required_timestamp, required_uuid,
    },
    ProofplaneMcp,
};
use crate::{
    domain::{AuditReviewPeriod, AuditorAccessGrant, AuditorAccessGrantId, WorkspacePermission},
    observability::audit::{AuditActor, AuditClientType, AuditEvent, AuditObject, AuditOutcome},
    services::{
        auditor_access_grants::{
            AuditorAccessGrantError, CreateAuditorAccessGrantRequest, IssuedAuditorAccessGrant,
        },
        Error as ServiceError,
    },
    validate,
};

#[tool_router(router = auditor_access_grants_tool_router, vis = "pub(super)")]
impl ProofplaneMcp {
    #[tool(
        name = "create_auditor_access_link",
        description = "Create a bearer-secret browser link that lets the named auditor review compliance evidence whose coverage window overlaps the audit period from period_start to period_end, and cannot see or download anything outside it, until the grant expires."
    )]
    async fn create_auditor_access_link(
        &self,
        ctx: RequestContext<RoleServer>,
        Parameters(args): Parameters<CreateAuditorAccessLinkRequest>,
    ) -> Result<Json<CreateAuditorAccessLinkResponse>, ErrorData> {
        let request = parse_create_request(args)?;
        let context = authorize_token_workspace(&ctx, WorkspacePermission::ManageAuditorAccess)?;
        let workspace_id = context.connection.workspace_id;
        let issued = self
            .auditor_access_grants
            .create(&context.connection, request)
            .await?;

        emit_auditor_grant_audit(
            "auditor_access_grant.created",
            "create_auditor_access_link",
            &context,
            &issued.grant,
        );

        Ok(Json(CreateAuditorAccessLinkResponse::from_issued(
            issued,
            &self.public_api_base_url,
            workspace_id.into(),
        )))
    }

    #[tool(
        name = "list_auditor_access_links",
        description = "List auditor access grants with email, creation, expiry, and revocation metadata without returning bearer-secret URLs."
    )]
    async fn list_auditor_access_links(
        &self,
        ctx: RequestContext<RoleServer>,
    ) -> Result<Json<ListAuditorAccessLinksResponse>, ErrorData> {
        let context = authorize_token_workspace(&ctx, WorkspacePermission::ManageAuditorAccess)?;
        let grants = self.auditor_access_grants.list(&context.connection).await?;

        Ok(Json(ListAuditorAccessLinksResponse {
            grants: grants
                .into_iter()
                .map(AuditorAccessGrantResponse::from)
                .collect(),
        }))
    }

    #[tool(
        name = "revoke_auditor_access_link",
        description = "Revoke an auditor access grant by grant ID and return its updated metadata."
    )]
    async fn revoke_auditor_access_link(
        &self,
        ctx: RequestContext<RoleServer>,
        Parameters(args): Parameters<RevokeAuditorAccessLinkRequest>,
    ) -> Result<Json<RevokeAuditorAccessLinkResponse>, ErrorData> {
        let grant_id = parse_revoke_request(args)?;
        let context = authorize_token_workspace(&ctx, WorkspacePermission::ManageAuditorAccess)?;
        let grant = self
            .auditor_access_grants
            .revoke(&context.connection, grant_id)
            .await?;

        emit_auditor_grant_audit(
            "auditor_access_grant.revoked",
            "revoke_auditor_access_link",
            &context,
            &grant,
        );

        Ok(Json(RevokeAuditorAccessLinkResponse {
            grant: grant.into(),
        }))
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
struct CreateAuditorAccessLinkRequest {
    email: Option<String>,
    expires_at: Option<String>,
    period_start: Option<String>,
    period_end: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct RevokeAuditorAccessLinkRequest {
    grant_id: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
struct CreateAuditorAccessLinkResponse {
    url: String,
    grant: AuditorAccessGrantResponse,
    url_secret_type: &'static str,
    intended_use: &'static str,
}

impl CreateAuditorAccessLinkResponse {
    fn from_issued(issued: IssuedAuditorAccessGrant, base_url: &Url, workspace_id: Uuid) -> Self {
        let mut url = base_url.clone();
        url.set_path(&format!("/auditor-access/{workspace_id}"));
        url.query_pairs_mut()
            .clear()
            .append_pair("token", issued.raw_secret.expose_secret());

        Self {
            url: url.to_string(),
            grant: issued.grant.into(),
            url_secret_type: "bearer_secret",
            intended_use: "auditor_browser_access",
        }
    }
}

#[derive(Debug, Serialize, JsonSchema)]
struct ListAuditorAccessLinksResponse {
    grants: Vec<AuditorAccessGrantResponse>,
}

#[derive(Debug, Serialize, JsonSchema)]
struct RevokeAuditorAccessLinkResponse {
    grant: AuditorAccessGrantResponse,
}

#[derive(Debug, Serialize, JsonSchema)]
struct AuditorAccessGrantResponse {
    id: String,
    auditor_email: String,
    created_at: String,
    expires_at: String,
    period_start: String,
    period_end: String,
    revoked_at: Option<String>,
}

impl From<AuditorAccessGrant> for AuditorAccessGrantResponse {
    fn from(grant: AuditorAccessGrant) -> Self {
        Self {
            id: grant.id.to_string(),
            auditor_email: grant.auditor_email,
            created_at: format_datetime(grant.created_at),
            expires_at: format_datetime(grant.expires_at),
            period_start: format_datetime(grant.period.start),
            period_end: format_datetime(grant.period.end),
            revoked_at: grant.revoked_at.map(format_datetime),
        }
    }
}

fn parse_create_request(
    args: CreateAuditorAccessLinkRequest,
) -> Result<CreateAuditorAccessGrantRequest, ErrorData> {
    let email = args.email.filter(|value| !value.trim().is_empty());
    let (auditor_email, expires_at, period_start, period_end) = validate! {
        auditor_email <- match email {
            Some(email) => crate::validation::Validation::valid(email),
            None => crate::validation::Validation::invalid(super::common::McpArgumentError::Missing {
                field: "email",
            }),
        },
        expires_at <- optional_timestamp("expires_at", args.expires_at),
        period_start <- required_timestamp("period_start", args.period_start),
        period_end <- required_timestamp("period_end", args.period_end),
        => (auditor_email, expires_at, period_start, period_end),
    }
    .into_result()
    .map_err(argument_errors)?;

    let period = AuditReviewPeriod::new(period_start, period_end)
        .map_err(|error| domain_errors(vec![error]))?;

    Ok(CreateAuditorAccessGrantRequest {
        auditor_email,
        expires_at,
        period,
    })
}

fn parse_revoke_request(
    args: RevokeAuditorAccessLinkRequest,
) -> Result<AuditorAccessGrantId, ErrorData> {
    required_uuid("grant_id", args.grant_id)
        .map(AuditorAccessGrantId::from)
        .into_result()
        .map_err(argument_errors)
}

impl From<AuditorAccessGrantError> for ErrorData {
    fn from(error: AuditorAccessGrantError) -> Self {
        match error {
            AuditorAccessGrantError::Denied | AuditorAccessGrantError::Unavailable => not_found(),
            AuditorAccessGrantError::ExpiresAtInPast => {
                invalid_field("expires_at", "expires_at must be in the future")
            }
            AuditorAccessGrantError::InvalidEmail => {
                invalid_field("email", "auditor_email is invalid")
            }
            AuditorAccessGrantError::Secret(error) => {
                tracing::error!(%error, "MCP auditor access grant secret failure");
                ErrorData::internal_error("internal error", None)
            }
            AuditorAccessGrantError::Repository(error) => ServiceError::from(error).into(),
        }
    }
}

fn emit_auditor_grant_audit(
    event_name: &'static str,
    operation: &'static str,
    context: &crate::mcp::McpRequestContext,
    grant: &AuditorAccessGrant,
) {
    AuditEvent::new(
        event_name,
        AuditOutcome::Success,
        AuditActor::AgentConnection {
            user_id: context.connection.user_id.into(),
            agent_connection_id: context.connection.connection_id.into(),
        },
        AuditClientType::Mcp,
        operation,
    )
    .workspace_id(context.connection.workspace_id.into())
    .request_id(context.request_id.0)
    .metadata("auditor_email", &grant.auditor_email)
    .metadata("expires_at", format_datetime(grant.expires_at))
    .metadata("period_start", format_datetime(grant.period.start))
    .metadata("period_end", format_datetime(grant.period.end))
    .object(AuditObject::new("auditor_access_grant", grant.id.into()))
    .emit();
}
