use rmcp::{
    handler::server::wrapper::Parameters, schemars, schemars::JsonSchema, service::RequestContext,
    tool, tool_router, Json, RoleServer,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{
    common::{
        argument_errors, authorize, domain_errors, format_datetime, not_found, parse_uuid_arg,
        required_uuid, service_error,
    },
    evidence_requests::{parse_evidence_request_request, EvidenceRequestRequest, WorkspaceRequest},
    ProofplaneMcp,
};
use crate::domain::{
    required_text, Control, ControlId, CreateEvidenceRequestControlMappingPayload,
    EvidenceRequestControlMapping, EvidenceRequestId, FrameworkRequirement, WorkspaceId,
    WorkspacePermission,
};
use crate::{
    observability::audit::{AuditActor, AuditClientType, AuditEvent, AuditObject, AuditOutcome},
    validate,
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

    #[tool(
        name = "map_evidence_request_to_control",
        description = "Create a mapping between an evidence request and a control."
    )]
    async fn map_evidence_request_to_control(
        &self,
        ctx: RequestContext<RoleServer>,
        Parameters(args): Parameters<MapEvidenceRequestToControlRequest>,
    ) -> Result<Json<EvidenceRequestControlMappingResponseDTO>, rmcp::ErrorData> {
        let (workspace_id, payload) = parse_map_evidence_request_to_control_request(args)?;
        let context = authorize(&ctx, workspace_id, WorkspacePermission::WriteControls)?;
        let mapping = self
            .controls
            .create_evidence_request_control_mapping(context.token, payload)
            .await
            .map_err(service_error)?
            .ok_or_else(not_found)?;

        AuditEvent::new(
            "evidence_request_control_mapping.created",
            AuditOutcome::Success,
            AuditActor::ApiToken {
                user_id: context.token.user_id.into(),
                api_token_id: context.token.api_token_id.into(),
            },
            AuditClientType::Mcp,
            "map_evidence_request_to_control",
        )
        .workspace_id(workspace_id.into())
        .request_id(context.request_id.0)
        .metadata(
            "evidence_request_id",
            Uuid::from(mapping.evidence_request_id),
        )
        .metadata("control_id", Uuid::from(mapping.control.id))
        .object(AuditObject::new(
            "evidence_request_control_mapping",
            Uuid::from(mapping.control.id),
        ))
        .emit();

        Ok(Json(mapping.into()))
    }

    #[tool(
        name = "remove_evidence_request_control_mapping",
        description = "Remove a mapping between an evidence request and a control."
    )]
    async fn remove_evidence_request_control_mapping(
        &self,
        ctx: RequestContext<RoleServer>,
        Parameters(args): Parameters<RemoveEvidenceRequestControlMappingRequest>,
    ) -> Result<Json<RemoveEvidenceRequestControlMappingResponse>, rmcp::ErrorData> {
        let (workspace_id, evidence_request_id, control_id) =
            parse_remove_evidence_request_control_mapping_request(args)?;
        let context = authorize(&ctx, workspace_id, WorkspacePermission::WriteControls)?;
        let deleted = self
            .controls
            .delete_evidence_request_control_mapping(context.token, evidence_request_id, control_id)
            .await
            .map_err(service_error)?;

        if !deleted {
            return Err(not_found());
        }

        AuditEvent::new(
            "evidence_request_control_mapping.deleted",
            AuditOutcome::Success,
            AuditActor::ApiToken {
                user_id: context.token.user_id.into(),
                api_token_id: context.token.api_token_id.into(),
            },
            AuditClientType::Mcp,
            "remove_evidence_request_control_mapping",
        )
        .workspace_id(workspace_id.into())
        .request_id(context.request_id.0)
        .metadata("evidence_request_id", Uuid::from(evidence_request_id))
        .metadata("control_id", Uuid::from(control_id))
        .object(AuditObject::new(
            "evidence_request_control_mapping",
            Uuid::from(control_id),
        ))
        .emit();

        Ok(Json(RemoveEvidenceRequestControlMappingResponse {
            removed: true,
            evidence_request_id: evidence_request_id.to_string(),
            control_id: control_id.to_string(),
        }))
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
struct MapEvidenceRequestToControlRequest {
    workspace_id: Option<String>,
    evidence_request_id: Option<String>,
    control_id: Option<String>,
    rationale: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct RemoveEvidenceRequestControlMappingRequest {
    workspace_id: Option<String>,
    evidence_request_id: Option<String>,
    control_id: Option<String>,
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

#[derive(Debug, Serialize, JsonSchema)]
struct RemoveEvidenceRequestControlMappingResponse {
    removed: bool,
    evidence_request_id: String,
    control_id: String,
}

fn parse_map_evidence_request_to_control_request(
    args: MapEvidenceRequestToControlRequest,
) -> Result<(WorkspaceId, CreateEvidenceRequestControlMappingPayload), rmcp::ErrorData> {
    let (workspace_id, evidence_request_id, control_id) = validate! {
        workspace_id <- required_uuid("workspace_id", args.workspace_id).map(WorkspaceId::from),
        evidence_request_id <- required_uuid("evidence_request_id", args.evidence_request_id)
            .map(EvidenceRequestId::from),
        control_id <- required_uuid("control_id", args.control_id).map(ControlId::from),
        => (workspace_id, evidence_request_id, control_id),
    }
    .into_result()
    .map_err(argument_errors)?;

    let rationale = required_text("rationale", args.rationale.unwrap_or_default())
        .into_result()
        .map_err(domain_errors)?;

    Ok((
        workspace_id,
        CreateEvidenceRequestControlMappingPayload {
            evidence_request_id,
            control_id,
            rationale,
        },
    ))
}

fn parse_remove_evidence_request_control_mapping_request(
    args: RemoveEvidenceRequestControlMappingRequest,
) -> Result<(WorkspaceId, EvidenceRequestId, ControlId), rmcp::ErrorData> {
    validate! {
        workspace_id <- required_uuid("workspace_id", args.workspace_id).map(WorkspaceId::from),
        evidence_request_id <- required_uuid("evidence_request_id", args.evidence_request_id)
            .map(EvidenceRequestId::from),
        control_id <- required_uuid("control_id", args.control_id).map(ControlId::from),
        => (workspace_id, evidence_request_id, control_id),
    }
    .into_result()
    .map_err(argument_errors)
}
