use rmcp::{
    handler::server::wrapper::Parameters,
    schemars::{self, JsonSchema},
    service::RequestContext,
    tool, tool_router, ErrorData, Json, RoleServer,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{
    common::{
        argument_errors, authorize_token_workspace, conflict, domain_errors, not_found,
        required_arg, required_non_negative_u64, required_uuid,
    },
    machine_upload_descriptor::MachineUploadDescriptor,
    ProofplaneMcp,
};
use crate::{
    domain::{AgentPolicyDocumentUploadDeclaration, PolicyId, WorkspacePermission},
    observability::{
        agent_policy_document_uploads::{record_grant, AgentPolicyDocumentUploadGrantResult},
        audit::{AuditClientType, AuditEvent, AuditObject, AuditOutcome},
    },
    services::{
        agent_policy_document_upload_grants::{
            AgentPolicyDocumentUploadGrantError, IssuedAgentPolicyDocumentUploadGrant,
        },
        Error as ServiceError,
    },
    validate,
};

#[tool_router(router = agent_policy_document_upload_grants_tool_router, vis = "pub(super)")]
impl ProofplaneMcp {
    #[tool(
        name = "prepare_policy_document_upload",
        description = "Use this when a trusted runtime can read a local policy file and execute HTTP PUT: prepare a short-lived bearer-secret descriptor without sending the file path or bytes through MCP; for guidance, call get_proofplane_guide with topic policies."
    )]
    async fn prepare_policy_document_upload(
        &self,
        ctx: RequestContext<RoleServer>,
        Parameters(args): Parameters<PreparePolicyDocumentUploadRequest>,
    ) -> Result<Json<PreparePolicyDocumentUploadResponse>, ErrorData> {
        let (policy_id, declaration) =
            match parse_prepare_policy_document_upload_request(args, self.max_document_bytes) {
                Ok(parsed) => parsed,
                Err(error) => {
                    record_grant(AgentPolicyDocumentUploadGrantResult::ValidationRejected);
                    return Err(error);
                }
            };
        let context = match authorize_token_workspace(&ctx, WorkspacePermission::WriteControls) {
            Ok(context) => context,
            Err(error) => {
                record_grant(authorization_error_result(&error));
                return Err(error);
            }
        };
        let issued = match self
            .agent_policy_document_upload_grants
            .issue(&context.agent_connection_context(), policy_id, declaration)
            .await
        {
            Ok(issued) => issued,
            Err(error @ AgentPolicyDocumentUploadGrantError::Unavailable) => {
                record_grant(AgentPolicyDocumentUploadGrantResult::Unavailable);
                return Err(error.into());
            }
            Err(error @ AgentPolicyDocumentUploadGrantError::CurrentDocument) => {
                record_grant(AgentPolicyDocumentUploadGrantResult::CurrentDocument);
                return Err(error.into());
            }
            Err(error) => {
                record_grant(AgentPolicyDocumentUploadGrantResult::Failed);
                return Err(error.into());
            }
        };
        record_grant(AgentPolicyDocumentUploadGrantResult::Issued);
        AuditEvent::new(
            "agent_policy_document_upload_grant.issued",
            AuditOutcome::Success,
            context.audit_actor(),
            AuditClientType::Mcp,
            "prepare_policy_document_upload",
        )
        .workspace_id(context.connection.workspace_id.into())
        .request_id(context.request_id.0)
        .metadata("policy_id", Uuid::from(issued.grant.policy_id()))
        .object(AuditObject::new(
            "agent_policy_document_upload_grant",
            issued.grant.id().into(),
        ))
        .emit();
        let url = self
            .public_api_base_url
            .join(&format!(
                "agent-policy-document-uploads/{}",
                issued.grant.id()
            ))
            .map_err(|error| {
                tracing::error!(%error, "MCP agent policy document upload URL construction failed");
                ErrorData::internal_error("internal error", None)
            })?;

        Ok(Json(PreparePolicyDocumentUploadResponse::new(
            issued,
            url,
            self.max_document_bytes,
        )))
    }
}

fn authorization_error_result(error: &ErrorData) -> AgentPolicyDocumentUploadGrantResult {
    if error.code == rmcp::model::ErrorCode::INTERNAL_ERROR {
        AgentPolicyDocumentUploadGrantResult::Failed
    } else {
        AgentPolicyDocumentUploadGrantResult::Unavailable
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct PreparePolicyDocumentUploadRequest {
    #[schemars(required)]
    policy_id: Option<String>,
    #[schemars(required)]
    filename: Option<String>,
    #[schemars(required)]
    content_type: Option<String>,
    #[schemars(required)]
    content_length: Option<i64>,
    checksum_sha256: Option<String>,
}

#[derive(Serialize, JsonSchema)]
struct PreparePolicyDocumentUploadResponse {
    upload_id: String,
    upload: MachineUploadDescriptor,
}

impl PreparePolicyDocumentUploadResponse {
    fn new(issued: IssuedAgentPolicyDocumentUploadGrant, url: url::Url, max_bytes: u64) -> Self {
        let upload = MachineUploadDescriptor::new(
            url,
            &issued.credential,
            issued.grant.declaration().content_type(),
            issued.grant.expires_at(),
            max_bytes,
        );
        Self {
            upload_id: issued.grant.id().to_string(),
            upload,
        }
    }
}

fn parse_prepare_policy_document_upload_request(
    args: PreparePolicyDocumentUploadRequest,
    max_bytes: u64,
) -> Result<(PolicyId, AgentPolicyDocumentUploadDeclaration), ErrorData> {
    let (policy_id, filename, content_type, content_length) = validate! {
        policy_id <- required_uuid("policy_id", args.policy_id).map(PolicyId::from),
        filename <- required_arg("filename", args.filename),
        content_type <- required_arg("content_type", args.content_type),
        content_length <- required_non_negative_u64("content_length", args.content_length),
        => (policy_id, filename, content_type, content_length),
    }
    .into_result()
    .map_err(argument_errors)?;

    validate! {
        declaration <- AgentPolicyDocumentUploadDeclaration::new(
            filename,
            content_type,
            content_length,
            args.checksum_sha256,
            max_bytes,
        ),
        => (policy_id, declaration),
    }
    .into_result()
    .map_err(domain_errors)
}

impl From<AgentPolicyDocumentUploadGrantError> for ErrorData {
    fn from(error: AgentPolicyDocumentUploadGrantError) -> Self {
        match error {
            AgentPolicyDocumentUploadGrantError::Unavailable => not_found(),
            AgentPolicyDocumentUploadGrantError::CurrentDocument => conflict(
                "policy_document_exists",
                "policy already has a current document; call get_policy to inspect it",
            ),
            AgentPolicyDocumentUploadGrantError::Internal => {
                tracing::error!(%error, "MCP agent policy document upload grant failure");
                ErrorData::internal_error("internal error", None)
            }
            AgentPolicyDocumentUploadGrantError::Repository(repository_error) => {
                ServiceError::from(repository_error).into()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, Utc};
    use rmcp::ErrorData;
    use secrecy::SecretString;
    use serde_json::json;
    use url::Url;
    use uuid::Uuid;

    use super::{
        authorization_error_result, parse_prepare_policy_document_upload_request,
        PreparePolicyDocumentUploadRequest, PreparePolicyDocumentUploadResponse,
    };
    use crate::{
        domain::{
            AgentConnectionId, AgentPolicyDocumentUploadDeclaration,
            AgentPolicyDocumentUploadGrant, AgentPolicyDocumentUploadGrantId, PolicyId, UserId,
            WorkspaceId,
        },
        observability::agent_policy_document_uploads::AgentPolicyDocumentUploadGrantResult,
        services::agent_policy_document_upload_grants::{
            AgentPolicyDocumentUploadGrantError, IssuedAgentPolicyDocumentUploadGrant,
        },
    };

    #[test]
    fn preparation_accepts_zero_length_and_an_optional_checksum() {
        let policy_id = Uuid::new_v4();
        let (_, declaration) = parse_prepare_policy_document_upload_request(
            PreparePolicyDocumentUploadRequest {
                policy_id: Some(policy_id.to_string()),
                filename: Some("information-security-policy.pdf".to_owned()),
                content_type: Some("application/pdf".to_owned()),
                content_length: Some(0),
                checksum_sha256: None,
            },
            1024,
        )
        .expect("request is valid");

        assert_eq!(declaration.expected_content_length(), 0);
        assert!(declaration.expected_sha256().is_none());
    }

    #[test]
    fn preparation_reports_all_missing_required_fields() {
        let error = parse_prepare_policy_document_upload_request(
            PreparePolicyDocumentUploadRequest {
                policy_id: None,
                filename: None,
                content_type: None,
                content_length: None,
                checksum_sha256: None,
            },
            1024,
        )
        .expect_err("missing fields fail");

        let fields = error.data.as_ref().expect("error data")["problem"]["field_issues"]
            .as_array()
            .expect("field issues")
            .iter()
            .map(|issue| issue["field"].as_str().expect("field"))
            .collect::<Vec<_>>();
        assert_eq!(
            fields,
            ["policy_id", "filename", "content_type", "content_length"]
        );
    }

    #[test]
    fn preparation_accumulates_declaration_validation_failures() {
        let error = parse_prepare_policy_document_upload_request(
            PreparePolicyDocumentUploadRequest {
                policy_id: Some(Uuid::new_v4().to_string()),
                filename: Some("../secret.pdf".to_owned()),
                content_type: Some("not a media type".to_owned()),
                content_length: Some(5),
                checksum_sha256: Some("A".repeat(64)),
            },
            4,
        )
        .expect_err("invalid declaration fails");

        let fields = error.data.as_ref().expect("error data")["problem"]["field_issues"]
            .as_array()
            .expect("field issues")
            .iter()
            .map(|issue| issue["field"].as_str().expect("field"))
            .collect::<Vec<_>>();
        assert_eq!(
            fields,
            [
                "filename",
                "content_type",
                "content_length",
                "checksum_sha256"
            ]
        );
    }

    #[test]
    fn current_document_returns_a_stable_conflict_that_directs_policy_inspection() {
        let error = ErrorData::from(AgentPolicyDocumentUploadGrantError::CurrentDocument);

        assert_eq!(
            error.data.as_ref().expect("error data")["problem"]["code"],
            "policy_document_exists"
        );
        assert!(error.message.contains("get_policy"));
    }

    #[test]
    fn descriptor_contains_only_the_trusted_runtime_transfer_contract() {
        let issued_at = Utc::now();
        let upload_id = AgentPolicyDocumentUploadGrantId::from(Uuid::new_v4());
        let grant = AgentPolicyDocumentUploadGrant::issue(
            upload_id,
            WorkspaceId::from(Uuid::new_v4()),
            PolicyId::from(Uuid::new_v4()),
            AgentPolicyDocumentUploadDeclaration::new(
                "information-security-policy.pdf".to_owned(),
                "application/pdf".to_owned(),
                483_920,
                None,
                1_000_000,
            )
            .into_result()
            .expect("declaration is valid"),
            UserId::from(Uuid::new_v4()),
            AgentConnectionId::from(Uuid::new_v4()),
            issued_at,
            issued_at + Duration::minutes(5),
        )
        .expect("grant is valid");
        let response = PreparePolicyDocumentUploadResponse::new(
            IssuedAgentPolicyDocumentUploadGrant {
                grant,
                credential: SecretString::from("credential"),
            },
            Url::parse(&format!(
                "https://api.proofplane.test/agent-policy-document-uploads/{upload_id}"
            ))
            .expect("URL is valid"),
            1_000_000,
        );

        assert_eq!(
            serde_json::to_value(response).expect("response serializes"),
            json!({
                "upload_id": upload_id.to_string(),
                "upload": {
                    "method": "PUT",
                    "url": format!("https://api.proofplane.test/agent-policy-document-uploads/{upload_id}"),
                    "authorization": "Proofplane-Upload credential",
                    "content_type": "application/pdf",
                    "expires_at": (issued_at + Duration::minutes(5)).to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
                    "max_bytes": 1_000_000,
                }
            })
        );
    }

    #[test]
    fn authorization_metrics_distinguish_concealment_from_missing_context() {
        let unavailable = ErrorData::resource_not_found("resource not found", None);
        let internal = ErrorData::internal_error("internal error", None);

        assert!(matches!(
            authorization_error_result(&unavailable),
            AgentPolicyDocumentUploadGrantResult::Unavailable
        ));
        assert!(matches!(
            authorization_error_result(&internal),
            AgentPolicyDocumentUploadGrantResult::Failed
        ));
    }

    #[test]
    fn schemas_require_only_metadata_and_return_no_file_content_fields() {
        let input = serde_json::to_value(rmcp::schemars::schema_for!(
            PreparePolicyDocumentUploadRequest
        ))
        .expect("input schema serializes");
        let output = serde_json::to_value(rmcp::schemars::schema_for!(
            PreparePolicyDocumentUploadResponse
        ))
        .expect("output schema serializes");

        assert_eq!(
            input["required"],
            json!(["policy_id", "filename", "content_type", "content_length"])
        );
        assert!(input["properties"].get("checksum_sha256").is_some());
        assert_eq!(input["additionalProperties"], false);
        assert!(output["properties"].get("upload_id").is_some());
        assert!(output["properties"].get("upload").is_some());
        let input_properties = input["properties"].as_object().expect("input properties");
        let output_properties = output["properties"].as_object().expect("output properties");
        let descriptor_properties = output["$defs"]["MachineUploadDescriptor"]["properties"]
            .as_object()
            .expect("descriptor properties");
        for forbidden in ["bytes", "path", "attachment", "object_key", "base64"] {
            assert!(
                !input_properties.contains_key(forbidden)
                    && !output_properties.contains_key(forbidden)
                    && !descriptor_properties.contains_key(forbidden),
                "schemas exclude {forbidden:?}"
            );
        }
    }
}
