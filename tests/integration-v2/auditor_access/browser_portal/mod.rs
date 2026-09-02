mod assertions;
mod escaping;
mod framework_navigation;
mod initial_invite;
mod policy_pages;
mod unavailable_states;

use chrono::{DateTime, FixedOffset};
use http::StatusCode;
use proofplane::{
    domain::WorkspacePermission,
    routes::request_context::REQUEST_ID_HEADER,
    worker::{DOCUMENT_FINALIZATION_REQUESTED, DOCUMENT_SCAN_REQUESTED},
};
use serde_json::json;
use url::Url;
use uuid::Uuid;

use crate::support::{
    auditor_access::{assert_portal_read_audit_event, authenticate_auditor, invite_token},
    clamd::EICAR,
    documents::upload_form,
    harness,
    http::{local_path, request_cookie},
    mcp::McpClient,
    oauth::authorize_agent_connection,
    scenario::{
        types::{TestEvidenceSubmission, TestFrameworkRequirement},
        ScenarioBuilder,
    },
};

const PERIOD_START: &str = "2026-01-01T00:00:00Z";
const PERIOD_END: &str = "2026-12-31T23:59:59Z";
const EXPIRES_AT: &str = "2099-01-01T00:00:00Z";
const POLICY_UPLOAD_PATH: &str = "/policy-document-uploads/files";
