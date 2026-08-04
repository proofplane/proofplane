mod convergence;
mod happy_path;
mod helpers;
mod preparation_rejections;
mod transfer_rejections;

pub(super) use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
pub(super) use http::StatusCode;
pub(super) use proofplane::{
    domain::WorkspacePermission,
    worker::{DOCUMENT_FINALIZATION_REQUESTED, DOCUMENT_SCAN_REQUESTED},
};
pub(super) use serde_json::{json, Value};
pub(super) use uuid::Uuid;

pub(super) use crate::support::{
    agent_connections::get_agent_connection_id_for,
    evidence_documents::{VALID_FROM, VALID_UNTIL},
    harness::{self},
    json::{assert_rfc3339, object_keys},
    machine_uploads::{
        execute_transfer, fail_transfer_on_purpose, interrupted_transfer,
        machine_transfer as parse_machine_transfer, sha256, tamper, uuid_at, HttpResult,
        MachineTransfer, MAX_DOCUMENT_BYTES,
    },
    mcp::{assert_not_found, assert_validation_error, McpClient},
    oauth::authorize_agent_connection,
    scenario::ScenarioBuilder,
};

pub(super) const CONTENT_TYPE: &str = "text/plain";
pub(super) const PERMISSIONS: &[WorkspacePermission] = &[
    WorkspacePermission::ReadEvidenceSubmissions,
    WorkspacePermission::WriteEvidenceSubmissions,
];
