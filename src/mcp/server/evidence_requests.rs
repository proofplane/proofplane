use chrono::{DateTime, Utc};
use rmcp::{
    handler::server::wrapper::Parameters, schemars, schemars::JsonSchema, service::RequestContext,
    tool, tool_router, Json, RoleServer,
};
use serde::{Deserialize, Serialize};

use super::{
    common::{
        argument_errors, authorize_token_workspace, format_datetime, not_found, optional_timestamp,
        required_uuid, service_error,
    },
    ProofplaneMcp,
};
use crate::{
    domain::{EvidenceRequest, EvidenceRequestId, WorkspacePermission},
    validate,
};

#[tool_router(router = evidence_requests_tool_router, vis = "pub(super)")]
impl ProofplaneMcp {
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

#[cfg(test)]
mod tests {
    use super::{parse_due_evidence_requests_request, ListDueEvidenceRequestsRequest};

    #[test]
    fn due_request_accepts_missing_optional_now() {
        let now = parse_due_evidence_requests_request(ListDueEvidenceRequestsRequest { now: None })
            .expect("valid args");

        assert_eq!(now, None);
    }
}
