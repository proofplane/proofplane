use chrono::{DateTime, Utc};
use rmcp::{
    handler::server::wrapper::Parameters, schemars, schemars::JsonSchema, service::RequestContext,
    tool, tool_router, Json, RoleServer,
};
use serde::{Deserialize, Serialize};

use super::{
    common::{
        argument_errors, authorize_token_workspace, domain_errors, format_datetime, not_found,
        optional_timestamp, required_timestamp, required_uuid, service_error,
    },
    ProofplaneMcp,
};
use crate::{
    domain::{
        required_text, validate_freshness_window_days, CreateEvidenceRequestPayload, DomainError,
        EvidenceRequest, EvidenceRequestCadence, EvidenceRequestId, EvidenceRequestStatus,
        WorkspacePermission,
    },
    observability::audit::{AuditActor, AuditClientType, AuditEvent, AuditObject, AuditOutcome},
    validate,
    validation::Validation,
};
use uuid::Uuid;

#[tool_router(router = evidence_requests_tool_router, vis = "pub(super)")]
impl ProofplaneMcp {
    #[tool(
        name = "create_evidence_request",
        description = "Create an evidence request in a workspace."
    )]
    async fn create_evidence_request(
        &self,
        ctx: RequestContext<RoleServer>,
        Parameters(args): Parameters<CreateEvidenceRequest>,
    ) -> Result<Json<GetEvidenceRequestResponse>, rmcp::ErrorData> {
        let payload = parse_create_evidence_request(args)?;
        let context = authorize_token_workspace(&ctx, WorkspacePermission::WriteEvidenceRequests)?;
        let workspace_id = context.token.workspace_id;
        let request = self
            .evidence_requests
            .create(context.token, payload)
            .await
            .map_err(service_error)?;

        AuditEvent::new(
            "evidence_request.created",
            AuditOutcome::Success,
            AuditActor::ApiToken {
                user_id: context.token.user_id.into(),
                api_token_id: context.token.api_token_id.into(),
            },
            AuditClientType::Mcp,
            "create_evidence_request",
        )
        .workspace_id(workspace_id.into())
        .request_id(context.request_id.0)
        .metadata("evidence_request_id", Uuid::from(request.id))
        .object(AuditObject::new("evidence_request", Uuid::from(request.id)))
        .emit();

        Ok(Json(GetEvidenceRequestResponse {
            evidence_request: request.into(),
        }))
    }

    #[tool(
        name = "list_evidence_requests",
        description = "List evidence requests in a workspace."
    )]
    async fn list_evidence_requests(
        &self,
        ctx: RequestContext<RoleServer>,
    ) -> Result<Json<ListEvidenceRequestsResponse>, rmcp::ErrorData> {
        let context = authorize_token_workspace(&ctx, WorkspacePermission::ReadEvidenceRequests)?;
        let requests = self
            .evidence_requests
            .list_by_workspace(context.token)
            .await
            .map_err(service_error)?;

        Ok(Json(ListEvidenceRequestsResponse {
            evidence_requests: requests.into_iter().map(Into::into).collect(),
        }))
    }

    #[tool(
        name = "get_evidence_request",
        description = "Get an evidence request by id."
    )]
    async fn get_evidence_request(
        &self,
        ctx: RequestContext<RoleServer>,
        Parameters(args): Parameters<EvidenceRequestRequest>,
    ) -> Result<Json<GetEvidenceRequestResponse>, rmcp::ErrorData> {
        let evidence_request_id = parse_evidence_request_request(args)?;
        let context = authorize_token_workspace(&ctx, WorkspacePermission::ReadEvidenceRequests)?;
        let request = self
            .evidence_requests
            .get(context.token, evidence_request_id)
            .await
            .map_err(service_error)?
            .ok_or_else(not_found)?;

        Ok(Json(GetEvidenceRequestResponse {
            evidence_request: request.into(),
        }))
    }

    #[tool(
        name = "list_due_evidence_requests",
        description = "List evidence requests due at or before a point in time."
    )]
    async fn list_due_evidence_requests(
        &self,
        ctx: RequestContext<RoleServer>,
        Parameters(args): Parameters<ListDueEvidenceRequestsRequest>,
    ) -> Result<Json<ListEvidenceRequestsResponse>, rmcp::ErrorData> {
        let now = parse_due_evidence_requests_request(args)?;
        let context = authorize_token_workspace(&ctx, WorkspacePermission::ReadEvidenceRequests)?;
        let requests = self
            .evidence_requests
            .list_due(context.token, now.unwrap_or_else(Utc::now))
            .await
            .map_err(service_error)?;

        Ok(Json(ListEvidenceRequestsResponse {
            evidence_requests: requests.into_iter().map(Into::into).collect(),
        }))
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
struct CreateEvidenceRequest {
    title: Option<String>,
    description: Option<String>,
    collection_instructions: Option<String>,
    cadence: Option<String>,
    due_at: Option<String>,
    schedule_anchor_at: Option<String>,
    freshness_window_days: Option<i32>,
    status: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(super) struct EvidenceRequestRequest {
    pub(super) evidence_request_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ListDueEvidenceRequestsRequest {
    now: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
struct ListEvidenceRequestsResponse {
    evidence_requests: Vec<EvidenceRequestResponseDTO>,
}

#[derive(Debug, Serialize, JsonSchema)]
struct GetEvidenceRequestResponse {
    evidence_request: EvidenceRequestResponseDTO,
}

#[derive(Debug, Serialize, JsonSchema)]
struct EvidenceRequestResponseDTO {
    id: String,
    workspace_id: String,
    title: String,
    description: String,
    collection_instructions: String,
    cadence: &'static str,
    due_at: String,
    schedule_anchor_at: String,
    freshness_window_days: Option<i32>,
    status: &'static str,
    created_at: String,
    updated_at: String,
}

impl From<EvidenceRequest> for EvidenceRequestResponseDTO {
    fn from(request: EvidenceRequest) -> Self {
        Self {
            id: request.id.to_string(),
            workspace_id: request.workspace_id.to_string(),
            title: request.title,
            description: request.description,
            collection_instructions: request.collection_instructions,
            cadence: request.cadence.as_str(),
            due_at: format_datetime(request.due_at),
            schedule_anchor_at: format_datetime(request.schedule_anchor_at),
            freshness_window_days: request.freshness_window_days,
            status: request.status.as_str(),
            created_at: format_datetime(request.created_at),
            updated_at: format_datetime(request.updated_at),
        }
    }
}

pub(super) fn parse_evidence_request_request(
    args: EvidenceRequestRequest,
) -> Result<EvidenceRequestId, rmcp::ErrorData> {
    validate! {
        evidence_request_id <- required_uuid("evidence_request_id", args.evidence_request_id)
            .map(EvidenceRequestId::from),
        => evidence_request_id,
    }
    .into_result()
    .map_err(argument_errors)
}

fn parse_create_evidence_request(
    args: CreateEvidenceRequest,
) -> Result<CreateEvidenceRequestPayload, rmcp::ErrorData> {
    let (due_at, schedule_anchor_at) = validate! {
        due_at <- required_timestamp("due_at", args.due_at),
        schedule_anchor_at <- required_timestamp("schedule_anchor_at", args.schedule_anchor_at),
        => (due_at, schedule_anchor_at),
    }
    .into_result()
    .map_err(argument_errors)?;

    validate! {
        title <- required_text("title", args.title.unwrap_or_default()),
        description <- required_text("description", args.description.unwrap_or_default()),
        collection_instructions <- required_text(
            "collection_instructions",
            args.collection_instructions.unwrap_or_default()
        ),
        cadence <- parse_cadence(args.cadence.unwrap_or_default()),
        freshness_window_days <- validate_freshness_window_days(args.freshness_window_days),
        status <- parse_status(args.status.unwrap_or_default()),
        => CreateEvidenceRequestPayload {
            title,
            description,
            collection_instructions,
            cadence,
            due_at,
            schedule_anchor_at,
            freshness_window_days,
            status,
        },
    }
    .into_result()
    .map_err(domain_errors)
}

fn parse_due_evidence_requests_request(
    args: ListDueEvidenceRequestsRequest,
) -> Result<Option<DateTime<Utc>>, rmcp::ErrorData> {
    validate! {
        now <- optional_timestamp("now", args.now),
        => now,
    }
    .into_result()
    .map_err(argument_errors)
}

fn parse_cadence(value: String) -> Validation<EvidenceRequestCadence, DomainError> {
    value
        .parse::<EvidenceRequestCadence>()
        .map(Validation::valid)
        .unwrap_or_else(Validation::invalid)
}

fn parse_status(value: String) -> Validation<EvidenceRequestStatus, DomainError> {
    value
        .parse::<EvidenceRequestStatus>()
        .map(Validation::valid)
        .unwrap_or_else(Validation::invalid)
}

#[cfg(test)]
mod tests {
    use super::{
        parse_create_evidence_request, parse_due_evidence_requests_request, CreateEvidenceRequest,
        ListDueEvidenceRequestsRequest,
    };
    use crate::domain::{EvidenceRequestCadence, EvidenceRequestStatus};

    #[test]
    fn due_request_accepts_missing_optional_now() {
        let now = parse_due_evidence_requests_request(ListDueEvidenceRequestsRequest { now: None })
            .expect("valid args");

        assert_eq!(now, None);
    }

    #[test]
    fn create_request_parses_payload() {
        let payload = parse_create_evidence_request(CreateEvidenceRequest {
            title: Some("Quarterly access review".to_owned()),
            description: Some("Collect access review evidence.".to_owned()),
            collection_instructions: Some("Upload the access review export.".to_owned()),
            cadence: Some("quarterly".to_owned()),
            due_at: Some("2026-04-01T00:00:00Z".to_owned()),
            schedule_anchor_at: Some("2026-01-01T00:00:00Z".to_owned()),
            freshness_window_days: Some(90),
            status: Some("active".to_owned()),
        })
        .expect("valid request parses");

        assert_eq!(payload.title, "Quarterly access review");
        assert_eq!(payload.cadence, EvidenceRequestCadence::Quarterly);
        assert_eq!(payload.status, EvidenceRequestStatus::Active);
    }
}
