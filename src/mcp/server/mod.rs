mod auditor_access_grants;
mod common;
mod controls;
mod document_grants;
mod evidence;
mod evidence_submissions;
mod guide;
mod policies;
mod policy_document_grants;
mod resources;

use rmcp::{
    handler::server::router::tool::ToolRouter,
    model::{
        Implementation, ListResourcesResult, PaginatedRequestParams, ReadResourceRequestParams,
        ReadResourceResult, ServerCapabilities, ServerInfo,
    },
    service::RequestContext,
    tool_handler, ErrorData, RoleServer, ServerHandler,
};

use crate::{
    mcp::server::common::authorize_connection,
    services::{
        auditor_access_grants::AuditorAccessGrantService, controls::ControlService,
        document_upload_grants::DocumentUploadGrantService, evidence::EvidenceService,
        evidence_submissions::EvidenceSubmissionService, policies::PolicyService,
        policy_document_upload_grants::PolicyDocumentUploadGrantService,
    },
    VERSION,
};
use url::Url;

// The lead should be 512 characters max. OpenAI specifically documents that ChatGPT pays special
// attention to these first 512 characters, even though that behavior isn't part of the MCP spec.
const SERVER_INSTRUCTION_LEAD: &str = concat!(
    "Proofplane manages SOC 2 and compliance evidence. Core workflow: first, find evidence with ",
    "list_evidence and read its collection_instructions; second, call manage_evidence_submissions ",
    "with the evidence ID and the coverage window the proof covers to get a short-lived human ",
    "browser flow; third, a human uploads files there and each file becomes one submission for ",
    "that window. File bytes never pass through MCP or the model. "
);

const SERVER_INSTRUCTION_DETAIL: &str = concat!(
    "Frameworks contain requirements, requirements are satisfied by controls, and control mappings ",
    "link controls to evidence. A submission is one file with a coverage window and the time it was ",
    "received. Several submissions may share a coverage window when one file cannot cover the ",
    "period. To replace proof, archive a submission and upload another. Controls define what must ",
    "be proven, so review their mappings when deciding which proof satisfies evidence. Submissions ",
    "record the connected agent's provenance. Treat the browser URL as a bearer secret and share it ",
    "only with the human uploading the files before it expires. Call get_proofplane_guide without a ",
    "topic to see its topic index. Clients that surface MCP resources can also browse these guides ",
    "at proofplane://docs/{topic}."
);

fn server_instructions() -> String {
    let mut instructions =
        String::with_capacity(SERVER_INSTRUCTION_LEAD.len() + SERVER_INSTRUCTION_DETAIL.len());
    instructions.push_str(SERVER_INSTRUCTION_LEAD);
    instructions.push_str(SERVER_INSTRUCTION_DETAIL);
    instructions
}

#[derive(Clone)]
pub struct ProofplaneMcp {
    evidence: EvidenceService,
    evidence_submissions: EvidenceSubmissionService,
    document_upload_grants: DocumentUploadGrantService,
    policy_document_upload_grants: PolicyDocumentUploadGrantService,
    auditor_access_grants: AuditorAccessGrantService,
    controls: ControlService,
    policies: PolicyService,
    public_api_base_url: Url,
    tool_router: ToolRouter<Self>,
}

pub(super) struct DocumentGrantServices {
    pub evidence: DocumentUploadGrantService,
    pub policy: PolicyDocumentUploadGrantService,
}

impl ProofplaneMcp {
    pub(super) fn new(
        evidence: EvidenceService,
        evidence_submissions: EvidenceSubmissionService,
        document_grants: DocumentGrantServices,
        auditor_access_grants: AuditorAccessGrantService,
        controls: ControlService,
        policies: PolicyService,
        public_api_base_url: Url,
    ) -> Self {
        Self {
            evidence,
            evidence_submissions,
            document_upload_grants: document_grants.evidence,
            policy_document_upload_grants: document_grants.policy,
            auditor_access_grants,
            controls,
            policies,
            public_api_base_url,
            tool_router: Self::tool_router(),
        }
    }

    fn tool_router() -> ToolRouter<Self> {
        ToolRouter::new()
            + Self::evidence_tool_router()
            + Self::evidence_submissions_tool_router()
            + Self::document_grants_tool_router()
            + Self::policy_document_grants_tool_router()
            + Self::auditor_access_grants_tool_router()
            + Self::controls_tool_router()
            + Self::policies_tool_router()
            + Self::guide_tool_router()
    }
}

fn server_info() -> ServerInfo {
    ServerInfo::new(
        ServerCapabilities::builder()
            .enable_tools()
            .enable_resources()
            .build(),
    )
    .with_server_info(Implementation::new("proofplane", VERSION))
    .with_instructions(server_instructions())
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for ProofplaneMcp {
    fn get_info(&self) -> ServerInfo {
        server_info()
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, ErrorData> {
        authorize_connection(&context)?;
        Ok(resources::list_doc_resources())
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResult, ErrorData> {
        authorize_connection(&context)?;
        resources::read_doc_resource(&request.uri)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{server_info, server_instructions, ProofplaneMcp, SERVER_INSTRUCTION_LEAD};

    fn expected_tool_descriptions() -> BTreeMap<&'static str, &'static str> {
        BTreeMap::from([
            (
                "archive_policy",
                "Archive an active policy when its current document is not being processed; for guidance, call get_proofplane_guide with topic policies.",
            ),
            (
                "attach_policy_to_control",
                "Attach an active policy to a control without changing the control or its other mappings; for guidance, call get_proofplane_guide with topic policies.",
            ),
            (
                "attach_policy_to_controls",
                "Attach one active policy to many controls in a single all-or-nothing batch; if any control id is unknown or already attached the whole batch is rejected; for guidance, call get_proofplane_guide with topic policies.",
            ),
            (
                "attach_control_to_policies",
                "Attach one control to many active policies in a single all-or-nothing batch; if any policy id is unknown, archived, or already attached the whole batch is rejected; for guidance, call get_proofplane_guide with topic policies.",
            ),
            (
                "detach_policy_from_controls",
                "Remove the mappings between one active policy and many controls in a single all-or-nothing batch; if any control id is unknown or not currently mapped the whole batch is rejected; for guidance, call get_proofplane_guide with topic policies.",
            ),
            (
                "detach_control_from_policies",
                "Remove the mappings between one control and many active policies in a single all-or-nothing batch; if any policy id is unknown, archived, or not currently mapped the whole batch is rejected; for guidance, call get_proofplane_guide with topic policies.",
            ),
            (
                "create_auditor_access_link",
                "Create a bearer-secret browser link that lets the named auditor review compliance evidence whose coverage window overlaps the audit period from period_start to period_end, and cannot see or download anything outside it, until the grant expires.",
            ),
            (
                "create_control",
                "Create a control that defines what must be proven and link it to the supplied framework requirement IDs; for guidance, call get_proofplane_guide with topic controls-and-mappings.",
            ),
            (
                "create_evidence",
                "Create a piece of evidence that states what the organization must prove and how to collect it; for guidance, call get_proofplane_guide with topic submitting-evidence.",
            ),
            (
                "create_policy",
                "Create a policy with optional control mappings and return its complete active metadata; for guidance, call get_proofplane_guide with topic policies.",
            ),
            (
                "detach_policy_from_control",
                "Detach an active policy from a control without changing the control or its other mappings; for guidance, call get_proofplane_guide with topic policies.",
            ),
            (
                "get_control",
                "Get one control and its linked framework requirements by control ID; for guidance, call get_proofplane_guide with topic controls-and-mappings.",
            ),
            (
                "get_evidence",
                "Get one piece of evidence with its collection instructions and status by evidence ID; for guidance, call get_proofplane_guide with topic submitting-evidence.",
            ),
            (
                "get_evidence_submission",
                "Get one evidence submission with its coverage window, provenance, and document metadata by submission ID; for guidance, call get_proofplane_guide with topic submitting-evidence.",
            ),
            (
                "get_latest_evidence_submission",
                "Get the latest submission for a piece of evidence with its coverage window, provenance, and document metadata; for guidance, call get_proofplane_guide with topic submitting-evidence.",
            ),
            (
                "get_policy",
                "Get one active policy with its mapped controls and safe current document metadata by policy ID; for guidance, call get_proofplane_guide with topic policies.",
            ),
            (
                "get_proofplane_guide",
                "Return embedded Proofplane guidance for a topic, or the ordered topic index when the topic is omitted or unknown.",
            ),
            (
                "list_auditor_access_links",
                "List auditor access grants with email, creation, expiry, and revocation metadata without returning bearer-secret URLs.",
            ),
            (
                "list_controls",
                "List controls that define what must be proven for compliance; for guidance, call get_proofplane_guide with topic controls-and-mappings.",
            ),
            (
                "list_evidence",
                "List evidence with their collection instructions and status; for guidance, call get_proofplane_guide with topic submitting-evidence.",
            ),
            (
                "list_evidence_control_mappings",
                "List the controls mapped to a piece of evidence, including each mapping rationale; for guidance, call get_proofplane_guide with topic controls-and-mappings.",
            ),
            (
                "list_evidence_submissions",
                "List the submissions for a piece of evidence, each one file with its coverage window, provenance, and document metadata; for guidance, call get_proofplane_guide with topic submitting-evidence.",
            ),
            (
                "list_framework_requirements",
                "List a compliance framework’s requirements so their IDs can be assigned to controls; for guidance, call get_proofplane_guide with topic controls-and-mappings.",
            ),
            (
                "list_frameworks",
                "List the supported compliance frameworks that organize requirements used by controls; for guidance, call get_proofplane_guide with topic controls-and-mappings.",
            ),
            (
                "list_policies",
                "List active policies with their mapped-control counts and current document status; for guidance, call get_proofplane_guide with topic policies.",
            ),
            (
                "manage_evidence_submissions",
                "Create a short-lived browser URL for a human to upload files as evidence submissions for a coverage window; each file becomes one submission; for guidance, call get_proofplane_guide with topic submitting-evidence.",
            ),
            (
                "manage_policy_document",
                "Create a short-lived bearer-secret browser URL for a human to manage an active policy’s document; file bytes never pass through MCP; for guidance, call get_proofplane_guide with topic policies.",
            ),
            (
                "map_control_to_evidence",
                "Map one control to many pieces of evidence in a single all-or-nothing batch, each with its own rationale; if any evidence id is unknown or already mapped the whole batch is rejected; for guidance, call get_proofplane_guide with topic controls-and-mappings.",
            ),
            (
                "map_evidence_to_control",
                "Map a piece of evidence to a control with a rationale explaining how that proof supports it; for guidance, call get_proofplane_guide with topic controls-and-mappings.",
            ),
            (
                "map_evidence_to_controls",
                "Map one piece of evidence to many controls in a single all-or-nothing batch, each with its own rationale; if any control id is unknown or already mapped the whole batch is rejected; for guidance, call get_proofplane_guide with topic controls-and-mappings.",
            ),
            (
                "remove_evidence_control_mapping",
                "Remove the mapping between a piece of evidence and a control by their IDs; for guidance, call get_proofplane_guide with topic controls-and-mappings.",
            ),
            (
                "replace_control",
                "Replace a control’s code, title, description, and complete framework-requirement links by control ID; for guidance, call get_proofplane_guide with topic controls-and-mappings.",
            ),
            (
                "revoke_auditor_access_link",
                "Revoke an auditor access grant by grant ID and return its updated metadata.",
            ),
            (
                "unmap_control_from_evidence",
                "Remove the mappings between one control and many pieces of evidence in a single all-or-nothing batch; if any evidence id is unknown or not currently mapped the whole batch is rejected; for guidance, call get_proofplane_guide with topic controls-and-mappings.",
            ),
            (
                "unmap_evidence_from_controls",
                "Remove the mappings between one piece of evidence and many controls in a single all-or-nothing batch; if any control id is unknown or not currently mapped the whole batch is rejected; for guidance, call get_proofplane_guide with topic controls-and-mappings.",
            ),
            (
                "update_policy",
                "Update an active policy’s name and optional description without changing mappings or document state; for guidance, call get_proofplane_guide with topic policies.",
            ),
        ])
    }

    #[test]
    fn server_info_includes_non_empty_instructions() {
        let instructions = server_info()
            .instructions
            .expect("server instructions are attached");

        assert!(!instructions.trim().is_empty());
    }

    #[test]
    fn instruction_lead_teaches_the_domain_and_complete_core_workflow() {
        assert_eq!(
            SERVER_INSTRUCTION_LEAD,
            concat!(
                "Proofplane manages SOC 2 and compliance evidence. Core workflow: first, find evidence with ",
                "list_evidence and read its collection_instructions; second, call manage_evidence_submissions ",
                "with the evidence ID and the coverage window the proof covers to get a short-lived human ",
                "browser flow; third, a human uploads files there and each file becomes one submission for ",
                "that window. File bytes never pass through MCP or the model. "
            ),
            "the protected instruction lead remains byte-for-byte stable"
        );
        let lead_length = SERVER_INSTRUCTION_LEAD.chars().count();
        assert!(
            lead_length <= 512,
            "instruction lead is {lead_length} characters; maximum is 512"
        );

        for expected in [
            "SOC 2",
            "compliance evidence",
            "find evidence with list_evidence",
            "read its collection_instructions",
            "call manage_evidence_submissions",
            "each file becomes one submission",
            "File bytes never pass through MCP or the model",
        ] {
            assert!(
                SERVER_INSTRUCTION_LEAD.contains(expected),
                "instruction lead contains {expected:?}"
            );
        }
        assert!(
            !SERVER_INSTRUCTION_LEAD.contains("get_proofplane_guide"),
            "guide discovery stays outside the protected instruction lead"
        );
    }

    #[test]
    fn instructions_cover_relationships_and_operational_constraints() {
        let instructions = server_instructions();

        for expected in [
            "Frameworks contain requirements",
            "requirements are satisfied by controls",
            "control mappings link controls to evidence",
            "A submission is one file",
            "coverage window",
            "Controls define what must be proven",
            "connected agent's provenance",
            "browser URL as a bearer secret",
            "before it expires",
            "Call get_proofplane_guide without a topic to see its topic index",
            "Clients that surface MCP resources can also browse these guides at proofplane://docs/{topic}",
        ] {
            assert!(
                instructions.contains(expected),
                "instructions contain {expected:?}"
            );
        }
    }

    #[test]
    fn instructions_do_not_expose_internal_surfaces() {
        let normalized = server_instructions().to_ascii_lowercase();

        for forbidden in [
            "workspace",
            "tenant",
            "rest",
            "ppat_",
            "leading",
            "powerful",
            "seamless",
            "best",
        ] {
            assert!(
                !normalized.contains(forbidden),
                "instructions omit {forbidden:?}"
            );
        }
    }

    #[test]
    fn server_advertises_tools_and_static_resources_without_change_flags() {
        let capabilities = serde_json::to_value(server_info().capabilities)
            .expect("server capabilities serialize");

        assert_eq!(capabilities["tools"], serde_json::json!({}));
        assert_eq!(capabilities["resources"], serde_json::json!({}));
        assert!(capabilities.get("prompts").is_none());
    }

    #[test]
    fn tool_router_registers_the_expected_descriptions() {
        let tools = ProofplaneMcp::tool_router().list_all();
        let actual = tools
            .iter()
            .map(|tool| {
                (
                    tool.name.as_ref(),
                    tool.description.as_deref().unwrap_or_default(),
                )
            })
            .collect::<BTreeMap<_, _>>();

        assert_eq!(actual, expected_tool_descriptions());
    }

    #[test]
    fn tool_descriptions_are_one_sentence_and_reference_registered_guide_topics() {
        use std::collections::HashSet;

        use crate::mcp::docs::TOPICS;

        const GUIDE_POINTER: &str = "for guidance, call get_proofplane_guide with topic ";
        let registered_topics = TOPICS
            .iter()
            .map(|topic| topic.topic)
            .collect::<HashSet<_>>();

        for tool in ProofplaneMcp::tool_router().list_all() {
            let description = tool.description.as_deref().unwrap_or_default();
            assert!(!description.is_empty(), "{} has a description", tool.name);
            assert!(
                description.ends_with('.') && description.matches('.').count() == 1,
                "{} has one complete sentence: {description:?}",
                tool.name
            );
            assert!(
                description.len() <= 260,
                "{} has a concise description: {description:?}",
                tool.name
            );

            let normalized = description.to_ascii_lowercase();
            for forbidden in ["rest", "ppat_", "workspace", "tenant", "proofplane://docs"] {
                assert!(
                    !normalized.contains(forbidden),
                    "{} omits {forbidden:?}: {description:?}",
                    tool.name
                );
            }

            let expected_topic = match tool.name.as_ref() {
                "create_evidence"
                | "list_evidence"
                | "get_evidence"
                | "list_evidence_submissions"
                | "get_evidence_submission"
                | "get_latest_evidence_submission"
                | "manage_evidence_submissions" => Some("submitting-evidence"),
                "list_frameworks"
                | "list_framework_requirements"
                | "list_controls"
                | "get_control"
                | "create_control"
                | "replace_control"
                | "list_evidence_control_mappings"
                | "map_evidence_to_control"
                | "map_evidence_to_controls"
                | "map_control_to_evidence"
                | "unmap_evidence_from_controls"
                | "unmap_control_from_evidence"
                | "remove_evidence_control_mapping" => Some("controls-and-mappings"),
                "list_policies"
                | "get_policy"
                | "create_policy"
                | "update_policy"
                | "archive_policy"
                | "attach_policy_to_control"
                | "attach_policy_to_controls"
                | "attach_control_to_policies"
                | "detach_policy_from_control"
                | "detach_policy_from_controls"
                | "detach_control_from_policies"
                | "manage_policy_document" => Some("policies"),
                "create_auditor_access_link"
                | "list_auditor_access_links"
                | "revoke_auditor_access_link"
                | "get_proofplane_guide" => None,
                name => panic!("unexpected tool {name}"),
            };
            let actual_topic = description
                .split_once(GUIDE_POINTER)
                .map(|(_, topic)| topic.trim_end_matches('.'));
            assert_eq!(
                actual_topic, expected_topic,
                "{} has the expected guide pointer",
                tool.name
            );
            if let Some(topic) = actual_topic {
                assert!(
                    registered_topics.contains(topic),
                    "{} points to registered topic {topic:?}",
                    tool.name
                );
            }
        }
    }
}
