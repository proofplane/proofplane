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

#[tool_handler(router = self.tool_router)]
impl ServerHandler for ProofplaneMcp {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("proofplane", VERSION))
    }
}
