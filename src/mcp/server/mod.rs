mod attachment_grants;
mod auditor_access_grants;
mod common;
mod controls;
mod evidence_requests;
mod evidence_submissions;

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
    "bearer secret and share it only with the human managing the attachment before it expires."
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
    use super::{server_info, SERVER_INSTRUCTIONS};

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
            "get_proofplane_guide",
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
}
