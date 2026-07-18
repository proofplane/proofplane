use rmcp::{
    handler::server::wrapper::Parameters, model::ErrorCode, schemars, schemars::JsonSchema,
    service::RequestContext, tool, tool_router, Json, RoleServer,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use super::{
    common::{
        argument_errors, authorize_token_workspace, domain_errors, format_datetime, invalid_field,
        not_found, required_uuid, service_error, McpArgumentError,
    },
    ProofplaneMcp,
};
use crate::{
    domain::{ControlId, CreatePolicyPayload, PolicyId, UpdatePolicyPayload, WorkspacePermission},
    observability::audit::{AuditClientType, AuditEvent, AuditObject, AuditOutcome},
    projections::policy_projection::{PolicyCatalogEntry, PolicyDetail, PolicyDocumentDetail},
    repository::ArchivePolicyResult,
    services::{policies::PolicyMutationError, Error as ServiceError},
    validate,
    validation::Validation,
};

#[tool_router(router = policies_tool_router, vis = "pub(super)")]
impl ProofplaneMcp {
    #[tool(
        name = "list_policies",
        description = "List active policies with their mapped-control counts and current document status."
    )]
    async fn list_policies(
        &self,
        ctx: RequestContext<RoleServer>,
    ) -> Result<Json<ListPoliciesResponse>, rmcp::ErrorData> {
        let context = authorize_token_workspace(&ctx, WorkspacePermission::ReadControls)?;
        let policies = self
            .policies
            .list(context.agent_connection_context())
            .await
            .map_err(service_error)?;

        emit_policy_audit(&context, "policy.listed", "list_policies", None);

        Ok(Json(ListPoliciesResponse {
            policies: policies.into_iter().map(Into::into).collect(),
        }))
    }

    #[tool(
        name = "get_policy",
        description = "Get one active policy with its mapped controls and safe current document metadata by policy ID."
    )]
    async fn get_policy(
        &self,
        ctx: RequestContext<RoleServer>,
        Parameters(args): Parameters<PolicyRequest>,
    ) -> Result<Json<PolicyDetailResponse>, rmcp::ErrorData> {
        let policy_id = parse_policy_request(args)?;
        let context = authorize_token_workspace(&ctx, WorkspacePermission::ReadControls)?;
        let policy = self
            .policies
            .get(context.agent_connection_context(), policy_id)
            .await
            .map_err(service_error)?
            .ok_or_else(not_found)?;

        emit_policy_audit(
            &context,
            "policy.read",
            "get_policy",
            Some(policy.policy.id),
        );

        Ok(Json(policy.into()))
    }

    #[tool(
        name = "create_policy",
        description = "Create a policy with optional control mappings and return its complete active metadata."
    )]
    async fn create_policy(
        &self,
        ctx: RequestContext<RoleServer>,
        Parameters(args): Parameters<CreatePolicyRequest>,
    ) -> Result<Json<PolicyDetailResponse>, rmcp::ErrorData> {
        let payload = parse_create_policy_request(args)?;
        let context = authorize_token_workspace(&ctx, WorkspacePermission::WriteControls)?;
        let policy = self
            .policies
            .create(context.agent_connection_context(), payload)
            .await
            .map_err(policy_mutation_error)?;

        emit_policy_audit(
            &context,
            "policy.created",
            "create_policy",
            Some(policy.policy.id),
        );

        Ok(Json(policy.into()))
    }

    #[tool(
        name = "update_policy",
        description = "Update an active policy’s name and optional description without changing mappings or document state."
    )]
    async fn update_policy(
        &self,
        ctx: RequestContext<RoleServer>,
        Parameters(args): Parameters<UpdatePolicyRequest>,
    ) -> Result<Json<PolicyDetailResponse>, rmcp::ErrorData> {
        let (policy_id, payload) = parse_update_policy_request(args)?;
        let context = authorize_token_workspace(&ctx, WorkspacePermission::WriteControls)?;
        let policy = self
            .policies
            .update(context.agent_connection_context(), policy_id, payload)
            .await
            .map_err(policy_mutation_error)?
            .ok_or_else(not_found)?;

        emit_policy_audit(
            &context,
            "policy.updated",
            "update_policy",
            Some(policy.policy.id),
        );

        Ok(Json(policy.into()))
    }

    #[tool(
        name = "archive_policy",
        description = "Archive an active policy when its current document is not being processed."
    )]
    async fn archive_policy(
        &self,
        ctx: RequestContext<RoleServer>,
        Parameters(args): Parameters<PolicyRequest>,
    ) -> Result<Json<ArchivePolicyResponse>, rmcp::ErrorData> {
        let policy_id = parse_policy_request(args)?;
        let context = authorize_token_workspace(&ctx, WorkspacePermission::WriteControls)?;
        let result = self
            .policies
            .archive(context.agent_connection_context(), policy_id)
            .await
            .map_err(policy_mutation_error)?;

        let ArchivePolicyResult::Archived {
            policy_id,
            archived_at,
        } = result
        else {
            return Err(match result {
                ArchivePolicyResult::NotFound => not_found(),
                ArchivePolicyResult::DocumentInProgress => policy_document_in_progress(),
                ArchivePolicyResult::Archived { .. } => rmcp::ErrorData::internal_error(
                    "dependency failure",
                    Some(json!({
                        "problem": {
                            "code": "dependency_failed",
                            "message": "a dependency failed while handling the tool call",
                        }
                    })),
                ),
            });
        };

        emit_policy_audit(
            &context,
            "policy.archived",
            "archive_policy",
            Some(policy_id),
        );

        Ok(Json(ArchivePolicyResponse {
            policy_id: policy_id.to_string(),
            archived_at: format_datetime(archived_at),
        }))
    }

    #[tool(
        name = "attach_policy_to_control",
        description = "Attach an active policy to a control without changing the control or its other mappings."
    )]
    async fn attach_policy_to_control(
        &self,
        ctx: RequestContext<RoleServer>,
        Parameters(args): Parameters<PolicyControlRequest>,
    ) -> Result<Json<PolicyControlResponse>, rmcp::ErrorData> {
        let (policy_id, control_id) = parse_policy_control_request(args)?;
        let context = authorize_token_workspace(&ctx, WorkspacePermission::WriteControls)?;
        let mapping = self
            .policies
            .attach_to_control(context.agent_connection_context(), policy_id, control_id)
            .await
            .map_err(policy_mutation_error)?
            .ok_or_else(not_found)?;

        emit_policy_control_audit(
            &context,
            "policy_control_mapping.created",
            "attach_policy_to_control",
            policy_id,
            mapping.control.id,
        );

        Ok(Json(PolicyControlResponse {
            policy_id: policy_id.to_string(),
            control_id: mapping.control.id.to_string(),
        }))
    }

    #[tool(
        name = "detach_policy_from_control",
        description = "Detach an active policy from a control without changing the control or its other mappings."
    )]
    async fn detach_policy_from_control(
        &self,
        ctx: RequestContext<RoleServer>,
        Parameters(args): Parameters<PolicyControlRequest>,
    ) -> Result<Json<PolicyControlResponse>, rmcp::ErrorData> {
        let (policy_id, control_id) = parse_policy_control_request(args)?;
        let context = authorize_token_workspace(&ctx, WorkspacePermission::WriteControls)?;
        let detached = self
            .policies
            .detach_from_control(context.agent_connection_context(), policy_id, control_id)
            .await
            .map_err(policy_mutation_error)?;
        if !detached {
            return Err(not_found());
        }

        emit_policy_control_audit(
            &context,
            "policy_control_mapping.deleted",
            "detach_policy_from_control",
            policy_id,
            control_id,
        );

        Ok(Json(PolicyControlResponse {
            policy_id: policy_id.to_string(),
            control_id: control_id.to_string(),
        }))
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
struct PolicyRequest {
    policy_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct CreatePolicyRequest {
    name: Option<String>,
    description: Option<String>,
    control_ids: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct UpdatePolicyRequest {
    policy_id: Option<String>,
    name: Option<String>,
    description: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct PolicyControlRequest {
    policy_id: Option<String>,
    control_id: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
struct ListPoliciesResponse {
    policies: Vec<PolicySummaryResponse>,
}

#[derive(Debug, Serialize, JsonSchema)]
struct PolicySummaryResponse {
    id: String,
    name: String,
    description: Option<String>,
    mapped_control_count: i64,
    document: Option<PolicyDocumentStatusResponse>,
}

impl From<PolicyCatalogEntry> for PolicySummaryResponse {
    fn from(policy: PolicyCatalogEntry) -> Self {
        Self {
            id: policy.id.to_string(),
            name: policy.name,
            description: policy.description,
            mapped_control_count: policy.mapped_control_count,
            document: policy
                .document
                .map(|document| PolicyDocumentStatusResponse {
                    upload_status: document.upload_status.as_str(),
                }),
        }
    }
}

#[derive(Debug, Serialize, JsonSchema)]
struct PolicyDocumentStatusResponse {
    upload_status: &'static str,
}

#[derive(Debug, Serialize, JsonSchema)]
struct PolicyDetailResponse {
    id: String,
    name: String,
    description: Option<String>,
    controls: Vec<PolicyControlSummaryResponse>,
    document: Option<PolicyDocumentResponse>,
    created_at: String,
    updated_at: String,
}

impl From<PolicyDetail> for PolicyDetailResponse {
    fn from(detail: PolicyDetail) -> Self {
        let policy = detail.policy;
        Self {
            id: policy.id.to_string(),
            name: policy.name,
            description: policy.description,
            controls: policy
                .control_mappings
                .into_iter()
                .map(|mapping| PolicyControlSummaryResponse {
                    id: mapping.control.id.to_string(),
                    code: mapping.control.code,
                    title: mapping.control.title,
                    description: mapping.control.description,
                })
                .collect(),
            document: detail.document.map(Into::into),
            created_at: format_datetime(policy.created_at),
            updated_at: format_datetime(policy.updated_at),
        }
    }
}

#[derive(Debug, Serialize, JsonSchema)]
struct PolicyControlSummaryResponse {
    id: String,
    code: String,
    title: String,
    description: String,
}

#[derive(Debug, Serialize, JsonSchema)]
struct PolicyDocumentResponse {
    id: String,
    filename: String,
    content_type: String,
    content_length: i64,
    checksum_sha256: String,
    checksum_crc32c: String,
    upload_status: &'static str,
    created_at: String,
}

impl From<PolicyDocumentDetail> for PolicyDocumentResponse {
    fn from(document: PolicyDocumentDetail) -> Self {
        Self {
            id: document.id.to_string(),
            filename: document.filename,
            content_type: document.content_type,
            content_length: document.content_length,
            checksum_sha256: document.checksum_sha256,
            checksum_crc32c: document.checksum_crc32c,
            upload_status: document.upload_status.as_str(),
            created_at: format_datetime(document.created_at),
        }
    }
}

#[derive(Debug, Serialize, JsonSchema)]
struct ArchivePolicyResponse {
    policy_id: String,
    archived_at: String,
}

#[derive(Debug, Serialize, JsonSchema)]
struct PolicyControlResponse {
    policy_id: String,
    control_id: String,
}

fn parse_policy_request(args: PolicyRequest) -> Result<PolicyId, rmcp::ErrorData> {
    validate! {
        policy_id <- required_uuid("policy_id", args.policy_id).map(PolicyId::from),
        => policy_id,
    }
    .into_result()
    .map_err(argument_errors)
}

fn parse_create_policy_request(
    args: CreatePolicyRequest,
) -> Result<CreatePolicyPayload, rmcp::ErrorData> {
    let control_ids = optional_control_ids(args.control_ids)
        .into_result()
        .map_err(argument_errors)?;

    Ok(CreatePolicyPayload {
        name: args.name.unwrap_or_default(),
        description: args.description,
        control_ids,
    })
}

fn parse_update_policy_request(
    args: UpdatePolicyRequest,
) -> Result<(PolicyId, UpdatePolicyPayload), rmcp::ErrorData> {
    let policy_id = validate! {
        policy_id <- required_uuid("policy_id", args.policy_id).map(PolicyId::from),
        => policy_id,
    }
    .into_result()
    .map_err(argument_errors)?;

    Ok((
        policy_id,
        UpdatePolicyPayload {
            name: args.name.unwrap_or_default(),
            description: args.description,
        },
    ))
}

fn parse_policy_control_request(
    args: PolicyControlRequest,
) -> Result<(PolicyId, ControlId), rmcp::ErrorData> {
    validate! {
        policy_id <- required_uuid("policy_id", args.policy_id).map(PolicyId::from),
        control_id <- required_uuid("control_id", args.control_id).map(ControlId::from),
        => (policy_id, control_id),
    }
    .into_result()
    .map_err(argument_errors)
}

fn optional_control_ids(
    values: Option<Vec<String>>,
) -> Validation<Vec<ControlId>, McpArgumentError> {
    let Some(values) = values else {
        return Validation::valid(Vec::new());
    };
    let mut ids = Vec::with_capacity(values.len());
    let mut errors = Vec::new();
    for value in values {
        match Uuid::parse_str(&value) {
            Ok(id) => ids.push(ControlId::from(id)),
            Err(_) => errors.push(McpArgumentError::InvalidUuid {
                field: "control_ids",
            }),
        }
    }

    if errors.is_empty() {
        Validation::valid(ids)
    } else {
        Validation::invalid_many(errors)
    }
}

fn policy_mutation_error(error: PolicyMutationError) -> rmcp::ErrorData {
    match error {
        PolicyMutationError::Validation(errors) => domain_errors(errors),
        PolicyMutationError::NameTaken => conflict(
            "policy_name_taken",
            "an active policy with this name already exists in the workspace",
        ),
        PolicyMutationError::MappingExists => conflict(
            "policy_control_mapping_exists",
            "this control is already mapped to the policy",
        ),
        PolicyMutationError::InvalidControlReferences => {
            invalid_field("control_ids", "control_ids contains unknown ids")
        }
        PolicyMutationError::Repository(error) => service_error(ServiceError::Repository(error)),
    }
}

fn conflict(code: &'static str, message: &'static str) -> rmcp::ErrorData {
    rmcp::ErrorData::new(
        ErrorCode(-32000),
        message,
        Some(json!({
            "problem": {
                "code": code,
                "message": message,
            }
        })),
    )
}

fn policy_document_in_progress() -> rmcp::ErrorData {
    conflict(
        "policy_document_in_progress",
        "policy cannot be archived while its document is being processed",
    )
}

fn emit_policy_audit(
    context: &crate::mcp::context::McpRequestContext,
    event: &'static str,
    operation: &'static str,
    policy_id: Option<PolicyId>,
) {
    let mut audit = AuditEvent::new(
        event,
        AuditOutcome::Success,
        context.audit_actor(),
        AuditClientType::Mcp,
        operation,
    )
    .workspace_id(context.connection.workspace_id.into())
    .request_id(context.request_id.0);
    if let Some(policy_id) = policy_id {
        audit = audit
            .metadata("policy_id", Uuid::from(policy_id))
            .object(AuditObject::new("policy", Uuid::from(policy_id)));
    }
    audit.emit();
}

fn emit_policy_control_audit(
    context: &crate::mcp::context::McpRequestContext,
    event: &'static str,
    operation: &'static str,
    policy_id: PolicyId,
    control_id: ControlId,
) {
    AuditEvent::new(
        event,
        AuditOutcome::Success,
        context.audit_actor(),
        AuditClientType::Mcp,
        operation,
    )
    .workspace_id(context.connection.workspace_id.into())
    .request_id(context.request_id.0)
    .metadata("policy_id", Uuid::from(policy_id))
    .metadata("control_id", Uuid::from(control_id))
    .object(AuditObject::new(
        "policy_control_mapping",
        Uuid::from(control_id),
    ))
    .emit();
}

#[cfg(test)]
mod tests {
    use super::{
        parse_create_policy_request, parse_policy_control_request, CreatePolicyRequest,
        PolicyControlRequest,
    };
    use uuid::Uuid;

    #[test]
    fn create_policy_request_defaults_omitted_control_ids_and_preserves_optional_description() {
        let payload = parse_create_policy_request(CreatePolicyRequest {
            name: Some("Policy".to_owned()),
            description: Some("Description".to_owned()),
            control_ids: None,
        })
        .expect("request parses");

        assert!(payload.control_ids.is_empty());
        assert_eq!(payload.description.as_deref(), Some("Description"));
    }

    #[test]
    fn create_policy_request_rejects_malformed_control_ids() {
        let error = parse_create_policy_request(CreatePolicyRequest {
            name: Some("Policy".to_owned()),
            description: None,
            control_ids: Some(vec!["not-a-uuid".to_owned()]),
        })
        .expect_err("malformed control id fails");

        assert_eq!(
            error.data.expect("problem data")["problem"]["field_issues"][0]["field"],
            "control_ids"
        );
    }

    #[test]
    fn policy_control_request_reports_both_malformed_identifiers() {
        let error = parse_policy_control_request(PolicyControlRequest {
            policy_id: Some("bad-policy".to_owned()),
            control_id: Some("bad-control".to_owned()),
        })
        .expect_err("malformed ids fail");
        let data = error.data.expect("problem data");
        let fields = data["problem"]["field_issues"]
            .as_array()
            .expect("field issues")
            .iter()
            .map(|issue| issue["field"].as_str().expect("field"))
            .collect::<Vec<_>>();

        assert_eq!(fields, ["policy_id", "control_id"]);
    }

    #[test]
    fn create_policy_request_parses_control_ids() {
        let control_id = Uuid::new_v4();
        let payload = parse_create_policy_request(CreatePolicyRequest {
            name: Some("Policy".to_owned()),
            description: None,
            control_ids: Some(vec![control_id.to_string()]),
        })
        .expect("request parses");

        assert_eq!(Uuid::from(payload.control_ids[0]), control_id);
    }
}
