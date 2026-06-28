use rmcp::{
    handler::server::wrapper::Parameters, model::ErrorCode, schemars, schemars::JsonSchema,
    service::RequestContext, tool, tool_router, Json, RoleServer,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use super::{
    common::{
        argument_errors, authorize_token_workspace, domain_errors, format_datetime, not_found,
        parse_uuid_arg, required_uuid, service_error,
    },
    evidence_requests::{parse_evidence_request_request, EvidenceRequestRequest},
    ProofplaneMcp,
};
use crate::domain::{
    required_text, Control, ControlId, CreateControlPayload,
    CreateEvidenceRequestControlMappingPayload, EvidenceRequestControlMapping, EvidenceRequestId,
    Framework, FrameworkId, FrameworkRequirement, FrameworkRequirementId, UpdateControlPayload,
    WorkspacePermission,
};
use crate::{
    observability::audit::{AuditActor, AuditClientType, AuditEvent, AuditObject, AuditOutcome},
    services::controls::ControlMutationError,
    validate,
    validation::Validation,
};

#[tool_router(router = controls_tool_router, vis = "pub(super)")]
impl ProofplaneMcp {
    #[tool(
        name = "list_frameworks",
        description = "List global compliance frameworks."
    )]
    async fn list_frameworks(
        &self,
        ctx: RequestContext<RoleServer>,
    ) -> Result<Json<ListFrameworksResponse>, rmcp::ErrorData> {
        authorize_token_workspace(&ctx, WorkspacePermission::ReadControls)?;
        let frameworks = self
            .controls
            .list_frameworks()
            .await
            .map_err(service_error)?;

        Ok(Json(ListFrameworksResponse {
            frameworks: frameworks.into_iter().map(Into::into).collect(),
        }))
    }

    #[tool(
        name = "list_framework_requirements",
        description = "List global requirements for a compliance framework."
    )]
    async fn list_framework_requirements(
        &self,
        ctx: RequestContext<RoleServer>,
        Parameters(args): Parameters<FrameworkRequirementsRequest>,
    ) -> Result<Json<ListFrameworkRequirementsResponse>, rmcp::ErrorData> {
        authorize_token_workspace(&ctx, WorkspacePermission::ReadControls)?;
        let framework_id =
            parse_uuid_arg("framework_id", args.framework_id).map(FrameworkId::from)?;
        let requirements = self
            .controls
            .list_framework_requirements(framework_id)
            .await
            .map_err(service_error)?;
        if requirements.is_empty() {
            return Err(not_found());
        }

        Ok(Json(ListFrameworkRequirementsResponse {
            requirements: requirements.into_iter().map(Into::into).collect(),
        }))
    }

    #[tool(name = "list_controls", description = "List controls in a workspace.")]
    async fn list_controls(
        &self,
        ctx: RequestContext<RoleServer>,
    ) -> Result<Json<ListControlsResponse>, rmcp::ErrorData> {
        let context = authorize_token_workspace(&ctx, WorkspacePermission::ReadControls)?;
        let controls = self
            .controls
            .list_controls(context.token)
            .await
            .map_err(service_error)?;

        Ok(Json(ListControlsResponse {
            controls: controls.into_iter().map(Into::into).collect(),
        }))
    }

    #[tool(name = "get_control", description = "Get a control in a workspace.")]
    async fn get_control(
        &self,
        ctx: RequestContext<RoleServer>,
        Parameters(args): Parameters<GetControlRequest>,
    ) -> Result<Json<ControlResponseDTO>, rmcp::ErrorData> {
        let control_id = parse_get_control_request(args)?;
        let context = authorize_token_workspace(&ctx, WorkspacePermission::ReadControls)?;
        let control = self
            .controls
            .get_control(context.token, control_id)
            .await
            .map_err(service_error)?
            .ok_or_else(not_found)?;

        Ok(Json(control.into()))
    }

    #[tool(
        name = "create_control",
        description = "Create a control in a workspace."
    )]
    async fn create_control(
        &self,
        ctx: RequestContext<RoleServer>,
        Parameters(args): Parameters<CreateControlRequest>,
    ) -> Result<Json<ControlResponseDTO>, rmcp::ErrorData> {
        let payload = parse_create_control_request(args)?;
        let context = authorize_token_workspace(&ctx, WorkspacePermission::WriteControls)?;
        let workspace_id = context.token.workspace_id;
        let control = self
            .controls
            .create_control(context.token, payload)
            .await
            .map_err(control_mutation_error)?;

        AuditEvent::new(
            "control.created",
            AuditOutcome::Success,
            AuditActor::ApiToken {
                user_id: context.token.user_id.into(),
                api_token_id: context.token.api_token_id.into(),
            },
            AuditClientType::Mcp,
            "create_control",
        )
        .workspace_id(workspace_id.into())
        .request_id(context.request_id.0)
        .metadata("control_id", Uuid::from(control.id))
        .object(AuditObject::new("control", Uuid::from(control.id)))
        .emit();

        Ok(Json(control.into()))
    }

    #[tool(
        name = "replace_control",
        description = "Replace a control in a workspace."
    )]
    async fn replace_control(
        &self,
        ctx: RequestContext<RoleServer>,
        Parameters(args): Parameters<ReplaceControlRequest>,
    ) -> Result<Json<ControlResponseDTO>, rmcp::ErrorData> {
        let (control_id, payload) = parse_replace_control_request(args)?;
        let context = authorize_token_workspace(&ctx, WorkspacePermission::WriteControls)?;
        let workspace_id = context.token.workspace_id;
        let control = self
            .controls
            .replace_control(context.token, control_id, payload)
            .await
            .map_err(control_mutation_error)?
            .ok_or_else(not_found)?;

        AuditEvent::new(
            "control.updated",
            AuditOutcome::Success,
            AuditActor::ApiToken {
                user_id: context.token.user_id.into(),
                api_token_id: context.token.api_token_id.into(),
            },
            AuditClientType::Mcp,
            "replace_control",
        )
        .workspace_id(workspace_id.into())
        .request_id(context.request_id.0)
        .metadata("control_id", Uuid::from(control.id))
        .object(AuditObject::new("control", Uuid::from(control.id)))
        .emit();

        Ok(Json(control.into()))
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
        let evidence_request_id = parse_evidence_request_request(args)?;
        let context = authorize_token_workspace(&ctx, WorkspacePermission::ReadControls)?;
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
        let payload = parse_map_evidence_request_to_control_request(args)?;
        let context = authorize_token_workspace(&ctx, WorkspacePermission::WriteControls)?;
        let workspace_id = context.token.workspace_id;
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
        let (evidence_request_id, control_id) =
            parse_remove_evidence_request_control_mapping_request(args)?;
        let context = authorize_token_workspace(&ctx, WorkspacePermission::WriteControls)?;
        let workspace_id = context.token.workspace_id;
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
struct FrameworkRequirementsRequest {
    framework_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct GetControlRequest {
    control_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ControlDTO {
    code: Option<String>,
    title: Option<String>,
    description: Option<String>,
    framework_requirement_ids: Option<Vec<String>>,
}

type CreateControlRequest = ControlDTO;

#[derive(Debug, Deserialize, JsonSchema)]
struct ReplaceControlRequest {
    control_id: Option<String>,
    code: Option<String>,
    title: Option<String>,
    description: Option<String>,
    framework_requirement_ids: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct MapEvidenceRequestToControlRequest {
    evidence_request_id: Option<String>,
    control_id: Option<String>,
    rationale: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct RemoveEvidenceRequestControlMappingRequest {
    evidence_request_id: Option<String>,
    control_id: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
struct ListControlsResponse {
    controls: Vec<ControlResponseDTO>,
}

#[derive(Debug, Serialize, JsonSchema)]
struct ListFrameworksResponse {
    frameworks: Vec<FrameworkResponseDTO>,
}

#[derive(Debug, Serialize, JsonSchema)]
struct FrameworkResponseDTO {
    id: String,
    code: String,
    name: String,
    description: String,
}

impl From<Framework> for FrameworkResponseDTO {
    fn from(framework: Framework) -> Self {
        Self {
            id: framework.id.to_string(),
            code: framework.code,
            name: framework.name,
            description: framework.description,
        }
    }
}

#[derive(Debug, Serialize, JsonSchema)]
struct ListFrameworkRequirementsResponse {
    requirements: Vec<FrameworkRequirementResponseDTO>,
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
) -> Result<CreateEvidenceRequestControlMappingPayload, rmcp::ErrorData> {
    let (evidence_request_id, control_id) = validate! {
        evidence_request_id <- required_uuid("evidence_request_id", args.evidence_request_id)
            .map(EvidenceRequestId::from),
        control_id <- required_uuid("control_id", args.control_id).map(ControlId::from),
        => (evidence_request_id, control_id),
    }
    .into_result()
    .map_err(argument_errors)?;

    let rationale = required_text("rationale", args.rationale.unwrap_or_default())
        .into_result()
        .map_err(domain_errors)?;

    Ok(CreateEvidenceRequestControlMappingPayload {
        evidence_request_id,
        control_id,
        rationale,
    })
}

fn parse_get_control_request(args: GetControlRequest) -> Result<ControlId, rmcp::ErrorData> {
    validate! {
        control_id <- required_uuid("control_id", args.control_id).map(ControlId::from),
        => control_id,
    }
    .into_result()
    .map_err(argument_errors)
}

fn parse_create_control_request(
    args: CreateControlRequest,
) -> Result<CreateControlPayload, rmcp::ErrorData> {
    let framework_requirement_ids = validate! {
        framework_requirement_ids <- required_uuid_array(
            "framework_requirement_ids",
            args.framework_requirement_ids,
        ),
        => framework_requirement_ids,
    }
    .into_result()
    .map_err(argument_errors)?;

    let (code, title, description) = validate! {
        code <- required_text("code", args.code.unwrap_or_default()),
        title <- required_text("title", args.title.unwrap_or_default()),
        description <- required_text("description", args.description.unwrap_or_default()),
        => (code, title, description),
    }
    .into_result()
    .map_err(domain_errors)?;

    Ok(CreateControlPayload {
        code,
        title,
        description,
        framework_requirement_ids,
    })
}

fn parse_replace_control_request(
    args: ReplaceControlRequest,
) -> Result<(ControlId, UpdateControlPayload), rmcp::ErrorData> {
    let (control_id, framework_requirement_ids) = validate! {
        control_id <- required_uuid("control_id", args.control_id).map(ControlId::from),
        framework_requirement_ids <- required_uuid_array(
            "framework_requirement_ids",
            args.framework_requirement_ids,
        ),
        => (control_id, framework_requirement_ids),
    }
    .into_result()
    .map_err(argument_errors)?;

    let (code, title, description) = validate! {
        code <- required_text("code", args.code.unwrap_or_default()),
        title <- required_text("title", args.title.unwrap_or_default()),
        description <- required_text("description", args.description.unwrap_or_default()),
        => (code, title, description),
    }
    .into_result()
    .map_err(domain_errors)?;

    Ok((
        control_id,
        UpdateControlPayload {
            code,
            title,
            description,
            framework_requirement_ids,
        },
    ))
}

fn required_uuid_array(
    field: &'static str,
    values: Option<Vec<String>>,
) -> Validation<Vec<FrameworkRequirementId>, super::common::McpArgumentError> {
    let Some(values) = values else {
        return Validation::invalid(super::common::McpArgumentError::Missing { field });
    };

    let mut ids = Vec::with_capacity(values.len());
    let mut errors = Vec::new();
    for value in values {
        match Uuid::parse_str(&value) {
            Ok(id) => ids.push(FrameworkRequirementId::from(id)),
            Err(_) => errors.push(super::common::McpArgumentError::InvalidUuid { field }),
        }
    }

    if errors.is_empty() {
        return Validation::valid(ids);
    }

    Validation::invalid_many(errors)
}

fn parse_remove_evidence_request_control_mapping_request(
    args: RemoveEvidenceRequestControlMappingRequest,
) -> Result<(EvidenceRequestId, ControlId), rmcp::ErrorData> {
    validate! {
        evidence_request_id <- required_uuid("evidence_request_id", args.evidence_request_id)
            .map(EvidenceRequestId::from),
        control_id <- required_uuid("control_id", args.control_id).map(ControlId::from),
        => (evidence_request_id, control_id),
    }
    .into_result()
    .map_err(argument_errors)
}

fn control_mutation_error(error: ControlMutationError) -> rmcp::ErrorData {
    match error {
        ControlMutationError::CodeTaken => rmcp::ErrorData::new(
            ErrorCode(-32000),
            "a control with this code already exists in the workspace",
            Some(json!({
                "problem": {
                    "code": "control_code_taken",
                    "message": "a control with this code already exists in the workspace",
                }
            })),
        ),
        ControlMutationError::InvalidFrameworkRequirementReferences => {
            rmcp::ErrorData::invalid_params(
                "tool argument validation failed",
                Some(json!({
                    "problem": {
                        "code": "validation_failed",
                        "message": "tool argument validation failed",
                        "field_issues": [{
                            "field": "framework_requirement_ids",
                            "message": "framework_requirement_ids contains unknown ids",
                        }],
                    }
                })),
            )
        }
        ControlMutationError::Repository(error) => {
            tracing::error!(%error, "MCP control mutation repository failure");
            rmcp::ErrorData::internal_error(
                "dependency failure",
                Some(json!({
                    "problem": {
                        "code": "dependency_failed",
                        "message": "a dependency failed while handling the tool call",
                    }
                })),
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        parse_create_control_request, parse_replace_control_request, ControlDTO,
        ReplaceControlRequest,
    };
    use uuid::Uuid;

    fn field_issue_names(error: &rmcp::ErrorData) -> Vec<String> {
        error.data.as_ref().expect("error data")["problem"]["field_issues"]
            .as_array()
            .expect("field issues")
            .iter()
            .map(|issue| issue["field"].as_str().expect("field").to_owned())
            .collect()
    }

    #[test]
    fn create_control_request_parses_required_fields_and_uuid_array() {
        let requirement_id = Uuid::new_v4();
        let payload = parse_create_control_request(ControlDTO {
            code: Some("PP-AC-01".to_owned()),
            title: Some("Access review".to_owned()),
            description: Some("Review access quarterly.".to_owned()),
            framework_requirement_ids: Some(vec![requirement_id.to_string()]),
        })
        .expect("create request parses");

        assert_eq!(payload.code, "PP-AC-01");
        assert_eq!(
            payload
                .framework_requirement_ids
                .into_iter()
                .map(Uuid::from)
                .collect::<Vec<_>>(),
            [requirement_id]
        );
    }

    #[test]
    fn create_control_request_requires_framework_requirement_ids_even_when_empty() {
        let error = parse_create_control_request(ControlDTO {
            code: Some("PP-AC-01".to_owned()),
            title: Some("Access review".to_owned()),
            description: Some("Review access quarterly.".to_owned()),
            framework_requirement_ids: None,
        })
        .expect_err("missing array fails validation");

        assert_eq!(field_issue_names(&error), ["framework_requirement_ids"]);

        let payload = parse_create_control_request(ControlDTO {
            code: Some("PP-AC-01".to_owned()),
            title: Some("Access review".to_owned()),
            description: Some("Review access quarterly.".to_owned()),
            framework_requirement_ids: Some(vec![]),
        })
        .expect("empty array is explicit and valid");
        assert!(payload.framework_requirement_ids.is_empty());
    }

    #[test]
    fn create_control_request_reports_invalid_uuid_array_field() {
        let error = parse_create_control_request(ControlDTO {
            code: Some("PP-AC-01".to_owned()),
            title: Some("Access review".to_owned()),
            description: Some("Review access quarterly.".to_owned()),
            framework_requirement_ids: Some(vec!["not-a-uuid".to_owned()]),
        })
        .expect_err("invalid requirement id fails validation");

        assert_eq!(field_issue_names(&error), ["framework_requirement_ids"]);
    }

    #[test]
    fn create_control_request_rejects_empty_required_text() {
        let error = parse_create_control_request(ControlDTO {
            code: Some("".to_owned()),
            title: Some(" ".to_owned()),
            description: Some("\t".to_owned()),
            framework_requirement_ids: Some(vec![]),
        })
        .expect_err("empty text fails validation");

        assert_eq!(field_issue_names(&error), ["code", "title", "description"]);
    }

    #[test]
    fn replace_control_request_requires_control_id_and_parses_update_payload() {
        let control_id = Uuid::new_v4();
        let requirement_id = Uuid::new_v4();
        let (parsed_control_id, payload) = parse_replace_control_request(ReplaceControlRequest {
            control_id: Some(control_id.to_string()),
            code: Some("PP-AC-02".to_owned()),
            title: Some("Updated access review".to_owned()),
            description: Some("Updated review control.".to_owned()),
            framework_requirement_ids: Some(vec![requirement_id.to_string()]),
        })
        .expect("replace request parses");

        assert_eq!(Uuid::from(parsed_control_id), control_id);
        assert_eq!(payload.code, "PP-AC-02");

        let missing_control_id = parse_replace_control_request(ReplaceControlRequest {
            control_id: None,
            code: Some("PP-AC-02".to_owned()),
            title: Some("Updated access review".to_owned()),
            description: Some("Updated review control.".to_owned()),
            framework_requirement_ids: Some(vec![]),
        })
        .expect_err("missing control id fails validation");
        assert_eq!(field_issue_names(&missing_control_id), ["control_id"]);
    }
}
