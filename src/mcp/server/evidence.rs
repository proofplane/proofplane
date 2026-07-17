use rmcp::{
    handler::server::wrapper::Parameters, schemars, schemars::JsonSchema, service::RequestContext,
    tool, tool_router, Json, RoleServer,
};
use serde::{Deserialize, Serialize};

use super::{
    common::{
        argument_errors, authorize_token_workspace, domain_errors, format_datetime, not_found,
        required_uuid, service_error,
    },
    ProofplaneMcp,
};
use crate::{
    domain::{
        required_text, CreateEvidencePayload, Evidence, EvidenceId, EvidenceStatus,
        WorkspacePermission,
    },
    observability::audit::{AuditClientType, AuditEvent, AuditObject, AuditOutcome},
    validate,
};
use uuid::Uuid;

#[tool_router(router = evidence_tool_router, vis = "pub(super)")]
impl ProofplaneMcp {
    #[tool(
        name = "create_evidence",
        description = "Create evidence that states what must be proven and how to collect the proof; for guidance, call get_proofplane_guide with topic submitting-evidence."
    )]
    async fn create_evidence(
        &self,
        ctx: RequestContext<RoleServer>,
        Parameters(args): Parameters<CreateEvidenceArgs>,
    ) -> Result<Json<GetEvidenceResponse>, rmcp::ErrorData> {
        let payload = parse_create_evidence(args)?;
        let context = authorize_token_workspace(&ctx, WorkspacePermission::WriteEvidence)?;
        let workspace_id = context.connection.workspace_id;
        let evidence = self
            .evidence
            .create(context.agent_connection_context(), payload)
            .await
            .map_err(service_error)?;

        AuditEvent::new(
            "evidence.created",
            AuditOutcome::Success,
            context.audit_actor(),
            AuditClientType::Mcp,
            "create_evidence",
        )
        .workspace_id(workspace_id.into())
        .request_id(context.request_id.0)
        .metadata("evidence_id", Uuid::from(evidence.id))
        .object(AuditObject::new("evidence", Uuid::from(evidence.id)))
        .emit();

        Ok(Json(GetEvidenceResponse {
            evidence: evidence.into(),
        }))
    }

    #[tool(
        name = "list_evidence",
        description = "List evidence with its collection instructions and status; for guidance, call get_proofplane_guide with topic submitting-evidence."
    )]
    async fn list_evidence(
        &self,
        ctx: RequestContext<RoleServer>,
    ) -> Result<Json<ListEvidenceResponse>, rmcp::ErrorData> {
        let context = authorize_token_workspace(&ctx, WorkspacePermission::ReadEvidence)?;
        let evidence = self
            .evidence
            .list_by_workspace(context.agent_connection_context())
            .await
            .map_err(service_error)?;

        Ok(Json(ListEvidenceResponse {
            evidence: evidence.into_iter().map(Into::into).collect(),
        }))
    }

    #[tool(
        name = "get_evidence",
        description = "Get one piece of evidence with its collection instructions and status by evidence ID; for guidance, call get_proofplane_guide with topic submitting-evidence."
    )]
    async fn get_evidence(
        &self,
        ctx: RequestContext<RoleServer>,
        Parameters(args): Parameters<EvidenceArgs>,
    ) -> Result<Json<GetEvidenceResponse>, rmcp::ErrorData> {
        let evidence_id = parse_evidence_args(args)?;
        let context = authorize_token_workspace(&ctx, WorkspacePermission::ReadEvidence)?;
        let evidence = self
            .evidence
            .get(context.agent_connection_context(), evidence_id)
            .await
            .map_err(service_error)?
            .ok_or_else(not_found)?;

        Ok(Json(GetEvidenceResponse {
            evidence: evidence.into(),
        }))
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
struct CreateEvidenceArgs {
    title: Option<String>,
    description: Option<String>,
    collection_instructions: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(super) struct EvidenceArgs {
    pub(super) evidence_id: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
struct ListEvidenceResponse {
    evidence: Vec<EvidenceResponseDTO>,
}

#[derive(Debug, Serialize, JsonSchema)]
struct GetEvidenceResponse {
    evidence: EvidenceResponseDTO,
}

#[derive(Debug, Serialize, JsonSchema)]
struct EvidenceResponseDTO {
    id: String,
    workspace_id: String,
    title: String,
    description: String,
    collection_instructions: String,
    status: &'static str,
    created_at: String,
    updated_at: String,
}

impl From<Evidence> for EvidenceResponseDTO {
    fn from(evidence: Evidence) -> Self {
        Self {
            id: evidence.id.to_string(),
            workspace_id: evidence.workspace_id.to_string(),
            title: evidence.title,
            description: evidence.description,
            collection_instructions: evidence.collection_instructions,
            status: evidence.status.as_str(),
            created_at: format_datetime(evidence.created_at),
            updated_at: format_datetime(evidence.updated_at),
        }
    }
}

pub(super) fn parse_evidence_args(args: EvidenceArgs) -> Result<EvidenceId, rmcp::ErrorData> {
    validate! {
        evidence_id <- required_uuid("evidence_id", args.evidence_id)
            .map(EvidenceId::from),
        => evidence_id,
    }
    .into_result()
    .map_err(argument_errors)
}

fn parse_create_evidence(
    args: CreateEvidenceArgs,
) -> Result<CreateEvidencePayload, rmcp::ErrorData> {
    validate! {
        title <- required_text("title", args.title.unwrap_or_default()),
        description <- required_text("description", args.description.unwrap_or_default()),
        collection_instructions <- required_text(
            "collection_instructions",
            args.collection_instructions.unwrap_or_default()
        ),
        => CreateEvidencePayload {
            title,
            description,
            collection_instructions,
            status: EvidenceStatus::Active,
        },
    }
    .into_result()
    .map_err(domain_errors)
}

#[cfg(test)]
mod tests {
    use super::{parse_create_evidence, CreateEvidenceArgs};
    use crate::domain::EvidenceStatus;

    #[test]
    fn create_evidence_parses_payload() {
        let payload = parse_create_evidence(CreateEvidenceArgs {
            title: Some("Quarterly access review".to_owned()),
            description: Some("Collect access review evidence.".to_owned()),
            collection_instructions: Some("Upload the access review export.".to_owned()),
        })
        .expect("valid request parses");

        assert_eq!(payload.title, "Quarterly access review");
        assert_eq!(payload.status, EvidenceStatus::Active);
    }

    #[test]
    fn create_evidence_rejects_blank_required_text() {
        let error = parse_create_evidence(CreateEvidenceArgs {
            title: Some("   ".to_owned()),
            description: None,
            collection_instructions: None,
        })
        .expect_err("blank fields are rejected");

        let issues = error.data.as_ref().expect("error data")["problem"]["field_issues"]
            .as_array()
            .expect("field issues")
            .iter()
            .map(|issue| issue["field"].as_str().expect("field").to_owned())
            .collect::<Vec<_>>();

        assert_eq!(issues, ["title", "description", "collection_instructions"]);
    }
}
