use rmcp::{
    handler::server::wrapper::Parameters, schemars, schemars::JsonSchema, service::RequestContext,
    tool, tool_router, Json, RoleServer,
};
use serde::Serialize;

use super::{
    common::{authorize, format_datetime, not_found, parse_uuid_arg, service_error},
    evidence_requests::{parse_evidence_request_request, EvidenceRequestRequest, WorkspaceRequest},
    ProofplaneMcp,
};
use crate::domain::{
    Control, EvidenceRequestControlMapping, FrameworkRequirement, WorkspaceId, WorkspacePermission,
};

#[tool_router(router = controls_tool_router, vis = "pub(super)")]
impl ProofplaneMcp {
    #[tool(name = "list_controls", description = "List controls in a workspace.")]
    async fn list_controls(
        &self,
        ctx: RequestContext<RoleServer>,
        Parameters(args): Parameters<WorkspaceRequest>,
    ) -> Result<Json<ListControlsResponse>, rmcp::ErrorData> {
        let workspace_id =
            parse_uuid_arg("workspace_id", args.workspace_id).map(WorkspaceId::from)?;
        let context = authorize(&ctx, workspace_id, WorkspacePermission::ReadControls)?;
        let controls = self
            .controls
            .list_controls(context.token)
            .await
            .map_err(service_error)?;

        Ok(Json(ListControlsResponse {
            controls: controls.into_iter().map(Into::into).collect(),
        }))
    }

    #[tool(
        name = "list_evidence_request_control_mappings",
        description = "List control mappings for an evidence request."
    )]
    async fn list_evidence_request_control_mappings(
        &self,
        ctx: RequestContext<RoleServer>,
        Parameters(args): Parameters<EvidenceRequestRequest>,
    ) -> Result<Json<ListEvidenceRequestControlMappingsResponse>, rmcp::ErrorData> {
        let (workspace_id, evidence_request_id) = parse_evidence_request_request(args)?;
        let context = authorize(&ctx, workspace_id, WorkspacePermission::ReadControls)?;
        let mappings = self
            .controls
            .list_evidence_request_control_mappings(context.token, evidence_request_id)
            .await
            .map_err(service_error)?
            .ok_or_else(not_found)?;

        Ok(Json(ListEvidenceRequestControlMappingsResponse {
            mappings: mappings.into_iter().map(Into::into).collect(),
        }))
    }
}

#[derive(Debug, Serialize, JsonSchema)]
struct ListControlsResponse {
    controls: Vec<ControlResponseDTO>,
}

#[derive(Debug, Serialize, JsonSchema)]
struct ControlResponseDTO {
    id: String,
    workspace_id: String,
    code: String,
    title: String,
    description: String,
    framework_requirements: Vec<FrameworkRequirementResponseDTO>,
    created_at: String,
    updated_at: String,
}

impl From<Control> for ControlResponseDTO {
    fn from(control: Control) -> Self {
        Self {
            id: control.id.to_string(),
            workspace_id: control.workspace_id.to_string(),
            code: control.code,
            title: control.title,
            description: control.description,
            framework_requirements: control
                .framework_requirements
                .into_iter()
                .map(Into::into)
                .collect(),
            created_at: format_datetime(control.created_at),
            updated_at: format_datetime(control.updated_at),
        }
    }
}

#[derive(Debug, Serialize, JsonSchema)]
struct FrameworkRequirementResponseDTO {
    id: String,
    framework_id: String,
    code: String,
    title: String,
    description: String,
}

impl From<FrameworkRequirement> for FrameworkRequirementResponseDTO {
    fn from(requirement: FrameworkRequirement) -> Self {
        Self {
            id: requirement.id.to_string(),
            framework_id: requirement.framework_id.to_string(),
            code: requirement.code,
            title: requirement.title,
            description: requirement.description,
        }
    }
}

#[derive(Debug, Serialize, JsonSchema)]
struct ListEvidenceRequestControlMappingsResponse {
    mappings: Vec<EvidenceRequestControlMappingResponseDTO>,
}

#[derive(Debug, Serialize, JsonSchema)]
struct EvidenceRequestControlMappingResponseDTO {
    evidence_request_id: String,
    control: ControlSummaryResponseDTO,
    rationale: String,
    created_at: String,
}

impl From<EvidenceRequestControlMapping> for EvidenceRequestControlMappingResponseDTO {
    fn from(mapping: EvidenceRequestControlMapping) -> Self {
        Self {
            evidence_request_id: mapping.evidence_request_id.to_string(),
            control: ControlSummaryResponseDTO {
                id: mapping.control.id.to_string(),
                code: mapping.control.code,
                title: mapping.control.title,
                description: mapping.control.description,
            },
            rationale: mapping.rationale,
            created_at: format_datetime(mapping.created_at),
        }
    }
}

#[derive(Debug, Serialize, JsonSchema)]
struct ControlSummaryResponseDTO {
    id: String,
    code: String,
    title: String,
    description: String,
}
