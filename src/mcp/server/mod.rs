mod attachment_grants;
mod auditor_access_grants;
mod common;
mod controls;
mod evidence_requests;
mod evidence_submissions;
mod guide;

use rmcp::{
    handler::server::router::tool::ToolRouter,
    model::{Implementation, ServerCapabilities, ServerInfo},
    tool_handler, ServerHandler,
};

use crate::{
    services::{
        attachment_upload_grants::AttachmentUploadGrantService,
        auditor_access_grants::AuditorAccessGrantService, controls::ControlService,
        evidence_requests::EvidenceRequestService, evidence_submissions::EvidenceSubmissionService,
    },
    VERSION,
};
use url::Url;

const SERVER_INSTRUCTIONS: &str = concat!(
    "Proofplane manages SOC 2 and compliance evidence. Core workflow: first, find evidence ",
    "requests with list_evidence_requests or list_due_evidence_requests and read ",
    "collection_instructions; second, create an evidence submission for the request with ",
    "create_evidence_submission; third, use manage_evidence_submission_attachment to get a ",
    "short-lived human browser flow for attachments. A human uploads files there; file bytes ",
    "never pass through MCP or the model. Frameworks contain requirements, requirements are ",
    "satisfied by controls, and control mappings link controls to evidence requests. Each ",
    "evidence request can have submissions, and each submission can have attachments. Controls ",
    "define what must be proven, so review their mappings when deciding which proof satisfies a ",
    "request. Submissions record the connected agent's provenance. Treat the browser URL as a ",
    "bearer secret and share it only with the human managing the attachment before it expires. ",
    "Call get_proofplane_guide without a topic to see its topic index."
);

#[derive(Clone)]
pub struct ProofplaneMcp {
    evidence_requests: EvidenceRequestService,
    evidence_submissions: EvidenceSubmissionService,
    attachment_upload_grants: AttachmentUploadGrantService,
    auditor_access_grants: AuditorAccessGrantService,
    controls: ControlService,
    public_api_base_url: Url,
    tool_router: ToolRouter<Self>,
}

impl ProofplaneMcp {
    pub fn new(
        evidence_requests: EvidenceRequestService,
        evidence_submissions: EvidenceSubmissionService,
        attachment_upload_grants: AttachmentUploadGrantService,
        auditor_access_grants: AuditorAccessGrantService,
        controls: ControlService,
        public_api_base_url: Url,
    ) -> Self {
        Self {
            evidence_requests,
            evidence_submissions,
            attachment_upload_grants,
            auditor_access_grants,
            controls,
            public_api_base_url,
            tool_router: Self::tool_router(),
        }
    }

    fn tool_router() -> ToolRouter<Self> {
        ToolRouter::new()
            + Self::evidence_requests_tool_router()
            + Self::evidence_submissions_tool_router()
            + Self::attachment_grants_tool_router()
            + Self::auditor_access_grants_tool_router()
            + Self::controls_tool_router()
            + Self::guide_tool_router()
    }
}

fn server_info() -> ServerInfo {
    ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
        .with_server_info(Implementation::new("proofplane", VERSION))
        .with_instructions(SERVER_INSTRUCTIONS)
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for ProofplaneMcp {
    fn get_info(&self) -> ServerInfo {
        server_info()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{server_info, ProofplaneMcp, SERVER_INSTRUCTIONS};

    fn expected_tool_descriptions() -> BTreeMap<&'static str, &'static str> {
        BTreeMap::from([
            (
                "create_auditor_access_link",
                "Create a bearer-secret browser link that lets the named auditor review compliance evidence until the grant expires.",
            ),
            (
                "create_control",
                "Create a control that defines what must be proven and link it to the supplied framework requirement IDs; for guidance, call get_proofplane_guide with topic controls-and-mappings.",
            ),
            (
                "create_evidence_request",
                "Create an evidence request that states what proof to collect, how to collect it, and when it is due; for guidance, call get_proofplane_guide with topic submitting-evidence.",
            ),
            (
                "create_evidence_submission",
                "Create a submission that records proof for an evidence request; call manage_evidence_submission_attachment afterward to obtain a human-browser attachment flow; for guidance, call get_proofplane_guide with topic submitting-evidence.",
            ),
            (
                "get_control",
                "Get one control and its linked framework requirements by control ID; for guidance, call get_proofplane_guide with topic controls-and-mappings.",
            ),
            (
                "get_evidence_request",
                "Get one evidence request with its collection instructions, due date, cadence, and status by evidence request ID; for guidance, call get_proofplane_guide with topic submitting-evidence.",
            ),
            (
                "get_evidence_submission",
                "Get one evidence submission with detailed provenance, coverage, collection, and attachment metadata by submission ID; for guidance, call get_proofplane_guide with topic submitting-evidence.",
            ),
            (
                "get_latest_evidence_submission",
                "Get the latest submission for an evidence request with compact provenance, coverage, summary, and attachment metadata; for guidance, call get_proofplane_guide with topic submitting-evidence.",
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
                "list_due_evidence_requests",
                "List evidence requests due at or before `now`, using the current time when `now` is omitted; for guidance, call get_proofplane_guide with topic submitting-evidence.",
            ),
            (
                "list_evidence_request_control_mappings",
                "List the controls mapped to an evidence request, including each mapping rationale; for guidance, call get_proofplane_guide with topic controls-and-mappings.",
            ),
            (
                "list_evidence_requests",
                "List evidence requests with their collection instructions, due dates, cadence, and status; for guidance, call get_proofplane_guide with topic submitting-evidence.",
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
                "manage_evidence_submission_attachment",
                "Create a short-lived bearer-secret browser URL for a human to upload or download an evidence submission’s attachments; file bytes never pass through MCP; for guidance, call get_proofplane_guide with topic attachments.",
            ),
            (
                "map_evidence_request_to_control",
                "Map an evidence request to a control with a rationale explaining how the requested proof supports it; for guidance, call get_proofplane_guide with topic controls-and-mappings.",
            ),
            (
                "remove_evidence_request_control_mapping",
                "Remove the mapping between an evidence request and a control by their IDs; for guidance, call get_proofplane_guide with topic controls-and-mappings.",
            ),
            (
                "replace_control",
                "Replace a control’s code, title, description, and complete framework-requirement links by control ID; for guidance, call get_proofplane_guide with topic controls-and-mappings.",
            ),
            (
                "revoke_auditor_access_link",
                "Revoke an auditor access grant by grant ID and return its updated metadata.",
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
        let lead = SERVER_INSTRUCTIONS.chars().take(512).collect::<String>();

        for expected in [
            "SOC 2",
            "compliance evidence",
            "find evidence requests",
            "read collection_instructions",
            "create an evidence submission",
            "human browser flow",
            "file bytes never pass through MCP or the model",
        ] {
            assert!(
                lead.contains(expected),
                "instruction lead contains {expected:?}"
            );
        }
        assert!(
            !lead.contains("get_proofplane_guide"),
            "guide discovery stays outside the protected instruction lead"
        );
    }

    #[test]
    fn instructions_cover_relationships_and_operational_constraints() {
        for expected in [
            "Frameworks contain requirements",
            "requirements are satisfied by controls",
            "control mappings link controls to evidence requests",
            "Each evidence request can have submissions",
            "each submission can have attachments",
            "Controls define what must be proven",
            "connected agent's provenance",
            "browser URL as a bearer secret",
            "before it expires",
            "Call get_proofplane_guide without a topic to see its topic index",
        ] {
            assert!(
                SERVER_INSTRUCTIONS.contains(expected),
                "instructions contain {expected:?}"
            );
        }
    }

    #[test]
    fn instructions_do_not_expose_internal_or_unavailable_surfaces() {
        let normalized = SERVER_INSTRUCTIONS.to_ascii_lowercase();

        for forbidden in [
            "workspace",
            "tenant",
            "rest",
            "ppat_",
            "proofplane://docs",
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
                "create_evidence_request"
                | "list_evidence_requests"
                | "get_evidence_request"
                | "list_due_evidence_requests"
                | "create_evidence_submission"
                | "get_evidence_submission"
                | "get_latest_evidence_submission" => Some("submitting-evidence"),
                "manage_evidence_submission_attachment" => Some("attachments"),
                "list_frameworks"
                | "list_framework_requirements"
                | "list_controls"
                | "get_control"
                | "create_control"
                | "replace_control"
                | "list_evidence_request_control_mappings"
                | "map_evidence_request_to_control"
                | "remove_evidence_request_control_mapping" => Some("controls-and-mappings"),
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
