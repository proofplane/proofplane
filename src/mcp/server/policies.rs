use rmcp::{
    handler::server::wrapper::Parameters,
    model::ErrorCode,
    schemars::{self, JsonSchema},
    service::RequestContext,
    tool, tool_router, ErrorData, Json, RoleServer,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use super::{
    common::{
        argument_errors, authorize_token_workspace, batch_rejected, domain_errors, format_datetime,
        invalid_field, not_found, required_uuid, McpArgumentError,
    },
    ProofplaneMcp,
};
use crate::{
    application::{
        commands::policies::{
            ArchivePolicy, ArchivePolicyError, AttachControlToPolicies, AttachPolicyToControls,
            ControlPolicyCommandError, CreatePolicy, DetachControlFromPolicies,
            DetachPolicyFromControls, PolicyCommandError, ReplacePolicy,
        },
        queries::policy_catalog::{GetPolicy, ListPolicies, PolicyCatalogError},
        ExecutionMetadata,
    },
    domain::{
        validate_batch, ControlId, CreateControlPolicyMappingsPayload,
        CreatePolicyControlMappingsPayload, CreatePolicyPayload,
        DeleteControlPolicyMappingsPayload, DeletePolicyControlMappingsPayload, PolicyId,
        UpdatePolicyPayload, WorkspacePermission,
    },
    observability::audit::{AuditClientType, AuditEvent, AuditObject, AuditOutcome},
    persistence::Error as RepositoryError,
    read_models::{PolicyCatalogEntry, PolicyDetail, PolicyDocumentDetail},
    services::Error as ServiceError,
    validate,
    validation::Validation,
};

#[tool_router(router = policies_tool_router, vis = "pub(super)")]
impl ProofplaneMcp {
    #[tool(
        name = "list_policies",
        description = "List active policies with their mapped-control counts and current document status; for guidance, call get_proofplane_guide with topic policies."
    )]
    async fn list_policies(
        &self,
        ctx: RequestContext<RoleServer>,
    ) -> Result<Json<ListPoliciesResponse>, ErrorData> {
        let context = authorize_token_workspace(&ctx, WorkspacePermission::ReadControls)?;
        let policies = self
            .policy_handlers
            .list
            .handle(ListPolicies {
                connection: context.agent_connection_context(),
            })
            .await
            .map_err(policy_catalog_error)?;

        emit_policy_audit(&context, "policy.listed", "list_policies", None);

        Ok(Json(ListPoliciesResponse {
            policies: policies.into_iter().map(Into::into).collect(),
        }))
    }

    #[tool(
        name = "get_policy",
        description = "Get one active policy with its mapped controls and safe current document metadata by policy ID; for guidance, call get_proofplane_guide with topic policies."
    )]
    async fn get_policy(
        &self,
        ctx: RequestContext<RoleServer>,
        Parameters(args): Parameters<PolicyRequest>,
    ) -> Result<Json<PolicyDetailResponse>, ErrorData> {
        let policy_id = parse_policy_request(args)?;
        let context = authorize_token_workspace(&ctx, WorkspacePermission::ReadControls)?;
        let policy = self
            .policy_handlers
            .get
            .handle(GetPolicy {
                connection: context.agent_connection_context(),
                policy_id,
            })
            .await
            .map_err(policy_catalog_error)?
            .ok_or_else(not_found)?;

        emit_policy_audit(&context, "policy.read", "get_policy", Some(policy.id));

        Ok(Json(policy.into()))
    }

    #[tool(
        name = "create_policy",
        description = "Create a policy with optional control mappings and return its complete active metadata; for guidance, call get_proofplane_guide with topic policies."
    )]
    async fn create_policy(
        &self,
        ctx: RequestContext<RoleServer>,
        Parameters(args): Parameters<CreatePolicyRequest>,
    ) -> Result<Json<PolicyDetailResponse>, ErrorData> {
        let payload = parse_create_policy_request(args)?;
        let context = authorize_token_workspace(&ctx, WorkspacePermission::WriteControls)?;
        let policy = self
            .policy_handlers
            .create
            .handle(
                CreatePolicy {
                    connection: context.agent_connection_context(),
                    name: payload.name,
                    description: payload.description,
                    control_ids: payload.control_ids,
                },
                ExecutionMetadata::background(),
            )
            .await
            .map_err(policy_mutation_error)?;
        let policy = policy.policy;

        emit_policy_audit(&context, "policy.created", "create_policy", Some(policy.id));

        Ok(Json(policy.into()))
    }

    #[tool(
        name = "update_policy",
        description = "Update an active policy’s name and optional description without changing mappings or document state; for guidance, call get_proofplane_guide with topic policies."
    )]
    async fn update_policy(
        &self,
        ctx: RequestContext<RoleServer>,
        Parameters(args): Parameters<UpdatePolicyRequest>,
    ) -> Result<Json<PolicyDetailResponse>, ErrorData> {
        let (policy_id, payload) = parse_update_policy_request(args)?;
        let context = authorize_token_workspace(&ctx, WorkspacePermission::WriteControls)?;
        let policy = self
            .policy_handlers
            .replace
            .handle(
                ReplacePolicy {
                    connection: context.agent_connection_context(),
                    policy_id,
                    name: payload.name,
                    description: payload.description,
                },
                ExecutionMetadata::background(),
            )
            .await
            .map_err(policy_mutation_error)?
            .policy;

        emit_policy_audit(&context, "policy.updated", "update_policy", Some(policy.id));

        Ok(Json(policy.into()))
    }

    #[tool(
        name = "archive_policy",
        description = "Archive an active policy when its current document is not being processed; for guidance, call get_proofplane_guide with topic policies."
    )]
    async fn archive_policy(
        &self,
        ctx: RequestContext<RoleServer>,
        Parameters(args): Parameters<PolicyRequest>,
    ) -> Result<Json<ArchivePolicyResponse>, ErrorData> {
        let policy_id = parse_policy_request(args)?;
        let context = authorize_token_workspace(&ctx, WorkspacePermission::WriteControls)?;
        let archived = self
            .policy_handlers
            .archive
            .handle(
                ArchivePolicy {
                    connection: context.agent_connection_context(),
                    policy_id,
                },
                ExecutionMetadata::background(),
            )
            .await
            .map_err(archive_policy_error)?;
        let archived_at = archived.archived_at;

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
        description = "Attach an active policy to a control without changing the control or its other mappings; for guidance, call get_proofplane_guide with topic policies."
    )]
    async fn attach_policy_to_control(
        &self,
        ctx: RequestContext<RoleServer>,
        Parameters(args): Parameters<PolicyControlRequest>,
    ) -> Result<Json<PolicyControlResponse>, ErrorData> {
        let (policy_id, control_id) = parse_policy_control_request(args)?;
        let context = authorize_token_workspace(&ctx, WorkspacePermission::WriteControls)?;
        let mapping = self
            .policy_handlers
            .attach_to_controls
            .handle(
                AttachPolicyToControls {
                    connection: context.agent_connection_context(),
                    policy_id,
                    control_ids: vec![control_id],
                },
                ExecutionMetadata::background(),
            )
            .await
            .map_err(attach_policy_single_error)?;
        let mapping = mapping
            .mappings
            .into_iter()
            .next()
            .ok_or_else(|| dependency_failure("saved policy mapping was not returned"))?;

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
        description = "Detach an active policy from a control without changing the control or its other mappings; for guidance, call get_proofplane_guide with topic policies."
    )]
    async fn detach_policy_from_control(
        &self,
        ctx: RequestContext<RoleServer>,
        Parameters(args): Parameters<PolicyControlRequest>,
    ) -> Result<Json<PolicyControlResponse>, ErrorData> {
        let (policy_id, control_id) = parse_policy_control_request(args)?;
        let context = authorize_token_workspace(&ctx, WorkspacePermission::WriteControls)?;
        self.policy_handlers
            .detach_from_controls
            .handle(
                DetachPolicyFromControls {
                    connection: context.agent_connection_context(),
                    policy_id,
                    control_ids: vec![control_id],
                },
                ExecutionMetadata::background(),
            )
            .await
            .map_err(detach_policy_single_error)?;

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

    #[tool(
        name = "attach_policy_to_controls",
        description = "Attach one active policy to many controls in a single all-or-nothing batch; if any control id is unknown or already attached the whole batch is rejected; for guidance, call get_proofplane_guide with topic policies."
    )]
    async fn attach_policy_to_controls(
        &self,
        ctx: RequestContext<RoleServer>,
        Parameters(args): Parameters<AttachPolicyToControlsRequest>,
    ) -> Result<Json<AttachPolicyToControlsResponse>, ErrorData> {
        let payload = parse_attach_policy_to_controls_request(args)?;
        let context = authorize_token_workspace(&ctx, WorkspacePermission::WriteControls)?;
        let workspace_id = context.connection.workspace_id;
        let policy_id = payload.policy_id;
        let control_ids = self
            .policy_handlers
            .attach_to_controls
            .handle(
                AttachPolicyToControls {
                    connection: context.agent_connection_context(),
                    policy_id: payload.policy_id,
                    control_ids: payload.control_ids,
                },
                ExecutionMetadata::background(),
            )
            .await
            .map_err(attach_policy_batch_error)?;
        let control_ids = control_ids.control_ids;

        let control_id_strings = control_ids
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>();

        AuditEvent::new(
            "policy_control_mappings.created",
            AuditOutcome::Success,
            context.audit_actor(),
            AuditClientType::Mcp,
            "attach_policy_to_controls",
        )
        .workspace_id(workspace_id.into())
        .request_id(context.request_id.0)
        .metadata("policy_id", Uuid::from(policy_id))
        .metadata("control_ids", json!(control_id_strings))
        .metadata("count", control_ids.len())
        .object(AuditObject::new("policy", Uuid::from(policy_id)))
        .emit();

        Ok(Json(AttachPolicyToControlsResponse {
            policy_id: policy_id.to_string(),
            count: control_ids.len(),
            control_ids: control_id_strings,
        }))
    }

    #[tool(
        name = "attach_control_to_policies",
        description = "Attach one control to many active policies in a single all-or-nothing batch; if any policy id is unknown, archived, or already attached the whole batch is rejected; for guidance, call get_proofplane_guide with topic policies."
    )]
    async fn attach_control_to_policies(
        &self,
        ctx: RequestContext<RoleServer>,
        Parameters(args): Parameters<AttachControlToPoliciesRequest>,
    ) -> Result<Json<AttachControlToPoliciesResponse>, ErrorData> {
        let payload = parse_attach_control_to_policies_request(args)?;
        let context = authorize_token_workspace(&ctx, WorkspacePermission::WriteControls)?;
        let workspace_id = context.connection.workspace_id;
        let control_id = payload.control_id;
        let policy_ids = self
            .policy_handlers
            .attach_control_to_policies
            .handle(
                AttachControlToPolicies {
                    connection: context.agent_connection_context(),
                    control_id: payload.control_id,
                    policy_ids: payload.policy_ids,
                },
                ExecutionMetadata::background(),
            )
            .await
            .map_err(attach_control_batch_error)?;
        let policy_ids = policy_ids.policy_ids;

        let policy_id_strings = policy_ids
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>();

        AuditEvent::new(
            "policy_control_mappings.created",
            AuditOutcome::Success,
            context.audit_actor(),
            AuditClientType::Mcp,
            "attach_control_to_policies",
        )
        .workspace_id(workspace_id.into())
        .request_id(context.request_id.0)
        .metadata("control_id", Uuid::from(control_id))
        .metadata("policy_ids", json!(policy_id_strings))
        .metadata("count", policy_ids.len())
        .object(AuditObject::new("control", Uuid::from(control_id)))
        .emit();

        Ok(Json(AttachControlToPoliciesResponse {
            control_id: control_id.to_string(),
            count: policy_ids.len(),
            policy_ids: policy_id_strings,
        }))
    }

    #[tool(
        name = "detach_policy_from_controls",
        description = "Remove the mappings between one active policy and many controls in a single all-or-nothing batch; if any control id is unknown or not currently mapped the whole batch is rejected; for guidance, call get_proofplane_guide with topic policies."
    )]
    async fn detach_policy_from_controls(
        &self,
        ctx: RequestContext<RoleServer>,
        Parameters(args): Parameters<DetachPolicyFromControlsRequest>,
    ) -> Result<Json<DetachPolicyFromControlsResponse>, ErrorData> {
        let payload = parse_detach_policy_from_controls_request(args)?;
        let context = authorize_token_workspace(&ctx, WorkspacePermission::WriteControls)?;
        let workspace_id = context.connection.workspace_id;
        let policy_id = payload.policy_id;
        let control_ids = self
            .policy_handlers
            .detach_from_controls
            .handle(
                DetachPolicyFromControls {
                    connection: context.agent_connection_context(),
                    policy_id: payload.policy_id,
                    control_ids: payload.control_ids,
                },
                ExecutionMetadata::background(),
            )
            .await
            .map_err(detach_policy_batch_error)?;
        let control_ids = control_ids.control_ids;

        let control_id_strings = control_ids
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>();

        AuditEvent::new(
            "policy_control_mappings.deleted",
            AuditOutcome::Success,
            context.audit_actor(),
            AuditClientType::Mcp,
            "detach_policy_from_controls",
        )
        .workspace_id(workspace_id.into())
        .request_id(context.request_id.0)
        .metadata("policy_id", Uuid::from(policy_id))
        .metadata("control_ids", json!(control_id_strings))
        .metadata("count", control_ids.len())
        .object(AuditObject::new("policy", Uuid::from(policy_id)))
        .emit();

        Ok(Json(DetachPolicyFromControlsResponse {
            policy_id: policy_id.to_string(),
            count: control_ids.len(),
            control_ids: control_id_strings,
        }))
    }

    #[tool(
        name = "detach_control_from_policies",
        description = "Remove the mappings between one control and many active policies in a single all-or-nothing batch; if any policy id is unknown, archived, or not currently mapped the whole batch is rejected; for guidance, call get_proofplane_guide with topic policies."
    )]
    async fn detach_control_from_policies(
        &self,
        ctx: RequestContext<RoleServer>,
        Parameters(args): Parameters<DetachControlFromPoliciesRequest>,
    ) -> Result<Json<DetachControlFromPoliciesResponse>, ErrorData> {
        let payload = parse_detach_control_from_policies_request(args)?;
        let context = authorize_token_workspace(&ctx, WorkspacePermission::WriteControls)?;
        let workspace_id = context.connection.workspace_id;
        let control_id = payload.control_id;
        let policy_ids = self
            .policy_handlers
            .detach_control_from_policies
            .handle(
                DetachControlFromPolicies {
                    connection: context.agent_connection_context(),
                    control_id: payload.control_id,
                    policy_ids: payload.policy_ids,
                },
                ExecutionMetadata::background(),
            )
            .await
            .map_err(detach_control_batch_error)?;
        let policy_ids = policy_ids.policy_ids;

        let policy_id_strings = policy_ids
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>();

        AuditEvent::new(
            "policy_control_mappings.deleted",
            AuditOutcome::Success,
            context.audit_actor(),
            AuditClientType::Mcp,
            "detach_control_from_policies",
        )
        .workspace_id(workspace_id.into())
        .request_id(context.request_id.0)
        .metadata("control_id", Uuid::from(control_id))
        .metadata("policy_ids", json!(policy_id_strings))
        .metadata("count", policy_ids.len())
        .object(AuditObject::new("control", Uuid::from(control_id)))
        .emit();

        Ok(Json(DetachControlFromPoliciesResponse {
            control_id: control_id.to_string(),
            count: policy_ids.len(),
            policy_ids: policy_id_strings,
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
        Self {
            id: detail.id.to_string(),
            name: detail.name,
            description: detail.description,
            controls: detail
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
            created_at: format_datetime(detail.created_at),
            updated_at: format_datetime(detail.updated_at),
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
    created_by_user_id: String,
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
            created_by_user_id: document.created_by_user_id.to_string(),
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

#[derive(Debug, Deserialize, JsonSchema)]
struct AttachPolicyToControlsRequest {
    policy_id: Option<String>,
    control_ids: Option<Vec<String>>,
}

#[derive(Debug, Serialize, JsonSchema)]
struct AttachPolicyToControlsResponse {
    policy_id: String,
    count: usize,
    control_ids: Vec<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct AttachControlToPoliciesRequest {
    control_id: Option<String>,
    policy_ids: Option<Vec<String>>,
}

#[derive(Debug, Serialize, JsonSchema)]
struct AttachControlToPoliciesResponse {
    control_id: String,
    count: usize,
    policy_ids: Vec<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct DetachPolicyFromControlsRequest {
    policy_id: Option<String>,
    control_ids: Option<Vec<String>>,
}

#[derive(Debug, Serialize, JsonSchema)]
struct DetachPolicyFromControlsResponse {
    policy_id: String,
    count: usize,
    control_ids: Vec<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct DetachControlFromPoliciesRequest {
    control_id: Option<String>,
    policy_ids: Option<Vec<String>>,
}

#[derive(Debug, Serialize, JsonSchema)]
struct DetachControlFromPoliciesResponse {
    control_id: String,
    count: usize,
    policy_ids: Vec<String>,
}

fn parse_policy_request(args: PolicyRequest) -> Result<PolicyId, ErrorData> {
    validate! {
        policy_id <- required_uuid("policy_id", args.policy_id).map(PolicyId::from),
        => policy_id,
    }
    .into_result()
    .map_err(argument_errors)
}

fn parse_create_policy_request(
    args: CreatePolicyRequest,
) -> Result<CreatePolicyPayload, ErrorData> {
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
) -> Result<(PolicyId, UpdatePolicyPayload), ErrorData> {
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
) -> Result<(PolicyId, ControlId), ErrorData> {
    validate! {
        policy_id <- required_uuid("policy_id", args.policy_id).map(PolicyId::from),
        control_id <- required_uuid("control_id", args.control_id).map(ControlId::from),
        => (policy_id, control_id),
    }
    .into_result()
    .map_err(argument_errors)
}

fn parse_attach_policy_to_controls_request(
    args: AttachPolicyToControlsRequest,
) -> Result<CreatePolicyControlMappingsPayload, ErrorData> {
    let policy_id = required_uuid("policy_id", args.policy_id)
        .map(PolicyId::from)
        .into_result()
        .map_err(argument_errors)?;

    let values = args.control_ids.unwrap_or_default();
    let mut control_ids = Vec::with_capacity(values.len());
    for value in values {
        let control_id = required_uuid("control_ids", Some(value))
            .map(ControlId::from)
            .into_result()
            .map_err(argument_errors)?;

        control_ids.push(control_id);
    }

    let control_ids = validate_batch("control_ids", control_ids)?;

    Ok(CreatePolicyControlMappingsPayload {
        policy_id,
        control_ids,
    })
}

fn parse_attach_control_to_policies_request(
    args: AttachControlToPoliciesRequest,
) -> Result<CreateControlPolicyMappingsPayload, ErrorData> {
    let control_id = required_uuid("control_id", args.control_id)
        .map(ControlId::from)
        .into_result()
        .map_err(argument_errors)?;

    let values = args.policy_ids.unwrap_or_default();
    let mut policy_ids = Vec::with_capacity(values.len());
    for value in values {
        let policy_id = required_uuid("policy_ids", Some(value))
            .map(PolicyId::from)
            .into_result()
            .map_err(argument_errors)?;

        policy_ids.push(policy_id);
    }

    let policy_ids = validate_batch("policy_ids", policy_ids)?;

    Ok(CreateControlPolicyMappingsPayload {
        control_id,
        policy_ids,
    })
}

fn parse_detach_policy_from_controls_request(
    args: DetachPolicyFromControlsRequest,
) -> Result<DeletePolicyControlMappingsPayload, ErrorData> {
    let policy_id = required_uuid("policy_id", args.policy_id)
        .map(PolicyId::from)
        .into_result()
        .map_err(argument_errors)?;

    let values = args.control_ids.unwrap_or_default();
    let mut control_ids = Vec::with_capacity(values.len());
    for value in values {
        let control_id = required_uuid("control_ids", Some(value))
            .map(ControlId::from)
            .into_result()
            .map_err(argument_errors)?;

        control_ids.push(control_id);
    }

    let control_ids = validate_batch("control_ids", control_ids)?;

    Ok(DeletePolicyControlMappingsPayload {
        policy_id,
        control_ids,
    })
}

fn parse_detach_control_from_policies_request(
    args: DetachControlFromPoliciesRequest,
) -> Result<DeleteControlPolicyMappingsPayload, ErrorData> {
    let control_id = required_uuid("control_id", args.control_id)
        .map(ControlId::from)
        .into_result()
        .map_err(argument_errors)?;

    let values = args.policy_ids.unwrap_or_default();
    let mut policy_ids = Vec::with_capacity(values.len());
    // TODO: This loop makes it so that we can't really use `validate!`
    // for the validation of this input. It's a pattern across all the
    // batch payloads. I think we can refactor this using functional
    // patterns to make it so that we can do this in one of the
    // applicative function calls in `validate!`.
    for value in values {
        let policy_id = required_uuid("policy_ids", Some(value))
            .map(PolicyId::from)
            .into_result()
            .map_err(argument_errors)?;

        policy_ids.push(policy_id);
    }

    let policy_ids = validate_batch("policy_ids", policy_ids)?;

    Ok(DeleteControlPolicyMappingsPayload {
        control_id,
        policy_ids,
    })
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

fn policy_catalog_error(error: PolicyCatalogError) -> ErrorData {
    match error {
        PolicyCatalogError::Unavailable => not_found(),
        PolicyCatalogError::Repository(error) => repository_error(error),
    }
}

fn policy_mutation_error(error: PolicyCommandError) -> ErrorData {
    match error {
        PolicyCommandError::Unavailable => not_found(),
        PolicyCommandError::InvalidDefinition(errors) => domain_errors(errors),
        PolicyCommandError::NameTaken => conflict(
            "policy_name_taken",
            "an active policy with this name already exists in the workspace",
        ),
        PolicyCommandError::Rejected { unknown, .. } => {
            if unknown.is_empty() {
                dependency_failure("policy mapping command was unexpectedly rejected")
            } else {
                invalid_field("control_ids", "control_ids contains unknown ids")
            }
        }
        PolicyCommandError::Repository(error) => repository_error(error),
    }
}

fn archive_policy_error(error: ArchivePolicyError) -> ErrorData {
    match error {
        ArchivePolicyError::Unavailable => not_found(),
        ArchivePolicyError::DocumentInProgress => policy_document_in_progress(),
        ArchivePolicyError::Repository(error) => repository_error(error),
    }
}

fn attach_policy_single_error(error: PolicyCommandError) -> ErrorData {
    match error {
        PolicyCommandError::Rejected {
            unknown,
            already_mapped: _,
        } if !unknown.is_empty() => {
            invalid_field("control_ids", "control_ids contains unknown ids")
        }
        PolicyCommandError::Rejected { already_mapped, .. } if !already_mapped.is_empty() => {
            conflict(
                "policy_control_mapping_exists",
                "this control is already mapped to the policy",
            )
        }
        other => policy_mutation_error(other),
    }
}

fn detach_policy_single_error(error: PolicyCommandError) -> ErrorData {
    match error {
        PolicyCommandError::Rejected { .. } | PolicyCommandError::Unavailable => not_found(),
        other => policy_mutation_error(other),
    }
}

fn attach_policy_batch_error(error: PolicyCommandError) -> ErrorData {
    match error {
        PolicyCommandError::Unavailable => not_found(),
        PolicyCommandError::Rejected {
            unknown,
            already_mapped,
        } => batch_policy_controls_rejected(unknown, already_mapped, false),
        PolicyCommandError::Repository(error) => repository_error(error),
        other => policy_mutation_error(other),
    }
}

fn detach_policy_batch_error(error: PolicyCommandError) -> ErrorData {
    match error {
        PolicyCommandError::Unavailable => not_found(),
        PolicyCommandError::Rejected {
            unknown,
            already_mapped,
        } => batch_policy_controls_rejected(unknown, already_mapped, true),
        PolicyCommandError::Repository(error) => repository_error(error),
        other => policy_mutation_error(other),
    }
}

fn attach_control_batch_error(error: ControlPolicyCommandError) -> ErrorData {
    match error {
        ControlPolicyCommandError::Unavailable => not_found(),
        ControlPolicyCommandError::Rejected {
            unknown,
            archived,
            invalid,
        } => batch_control_policies_rejected(unknown, archived, invalid, false),
        ControlPolicyCommandError::Repository(error) => repository_error(error),
    }
}

fn detach_control_batch_error(error: ControlPolicyCommandError) -> ErrorData {
    match error {
        ControlPolicyCommandError::Unavailable => not_found(),
        ControlPolicyCommandError::Rejected {
            unknown,
            archived,
            invalid,
        } => batch_control_policies_rejected(unknown, archived, invalid, true),
        ControlPolicyCommandError::Repository(error) => repository_error(error),
    }
}

fn repository_error(error: RepositoryError) -> ErrorData {
    ServiceError::Repository(error).into()
}

fn dependency_failure(context: &'static str) -> ErrorData {
    tracing::error!(context, "MCP policy dependency invariant failed");
    ErrorData::internal_error(
        "dependency failure",
        Some(json!({
            "problem": {
                "code": "dependency_failed",
                "message": "a dependency failed while handling the tool call",
            }
        })),
    )
}

fn batch_policy_controls_rejected(
    unknown: Vec<ControlId>,
    invalid: Vec<ControlId>,
    detaching: bool,
) -> ErrorData {
    let (message, invalid_key) = if detaching {
        (
            "control_ids contains unknown or not-mapped ids",
            "not_mapped_ids",
        )
    } else {
        (
            "control_ids contains unknown or already-attached ids",
            "already_mapped_ids",
        )
    };
    batch_rejected(
        "control_ids",
        message,
        vec![
            ("unknown_ids", unknown.into_iter().map(Uuid::from).collect()),
            (invalid_key, invalid.into_iter().map(Uuid::from).collect()),
        ],
    )
}

fn batch_control_policies_rejected(
    unknown: Vec<PolicyId>,
    archived: Vec<PolicyId>,
    invalid: Vec<PolicyId>,
    detaching: bool,
) -> ErrorData {
    let (message, invalid_key) = if detaching {
        (
            "policy_ids contains unknown, archived, or not-mapped ids",
            "not_mapped_ids",
        )
    } else {
        (
            "policy_ids contains unknown, archived, or already-attached ids",
            "already_mapped_ids",
        )
    };
    batch_rejected(
        "policy_ids",
        message,
        vec![
            ("unknown_ids", unknown.into_iter().map(Uuid::from).collect()),
            (
                "archived_ids",
                archived.into_iter().map(Uuid::from).collect(),
            ),
            (invalid_key, invalid.into_iter().map(Uuid::from).collect()),
        ],
    )
}

fn conflict(code: &'static str, message: &'static str) -> ErrorData {
    ErrorData::new(
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

fn policy_document_in_progress() -> ErrorData {
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
