mod convergence;
mod happy_path;
mod helpers;
mod preparation_rejections;
mod races;
mod transfer_rejections;

pub(super) use http::StatusCode;
pub(super) use proofplane::{
    domain::WorkspacePermission,
    routes::request_context::REQUEST_ID_HEADER,
    worker::{DOCUMENT_FINALIZATION_REQUESTED, DOCUMENT_SCAN_REQUESTED},
};
pub(super) use rmcp::model::ErrorCode;
pub(super) use serde_json::{json, Value};
pub(super) use uuid::Uuid;

pub(super) use crate::support::{
    agent_connections::get_agent_connection_id_for,
    documents::upload_form,
    harness::{self},
    http::{local_path, request_cookie},
    json::{assert_rfc3339, object_keys},
    machine_uploads::{
        execute_transfer, fail_transfer_on_purpose, interrupted_transfer,
        machine_transfer as parse_machine_transfer, sha256, tamper, uuid_at, HttpResult,
        MachineTransfer, MAX_DOCUMENT_BYTES,
    },
    mcp::{assert_not_found, assert_validation_error, McpClient, McpError},
    oauth::authorize_agent_connection,
    policy_documents::{assert_policy_read_model, ExpectedPolicyDocument},
    scenario::ScenarioBuilder,
};

pub(super) const CONTENT_TYPE: &str = "text/plain";
pub(super) const MANAGEMENT_PATH: &str = "/policy-document-uploads";
pub(super) const BROWSER_UPLOAD_PATH: &str = "/policy-document-uploads/files";
pub(super) const PERMISSIONS: &[WorkspacePermission] = &[
    WorkspacePermission::ReadControls,
    WorkspacePermission::WriteControls,
];
