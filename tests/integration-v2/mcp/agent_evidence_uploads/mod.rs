mod convergence;
mod happy_path;
mod helpers;
mod preparation_rejections;
mod transfer_rejections;

pub(super) use axum_test::TestResponse;
pub(super) use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
pub(super) use bytes::Bytes;
pub(super) use http::StatusCode;
pub(super) use proofplane::{
    domain::WorkspacePermission,
    routes::request_context::REQUEST_ID_HEADER,
    worker::{DOCUMENT_FINALIZATION_REQUESTED, DOCUMENT_SCAN_REQUESTED},
};
pub(super) use serde_json::{json, Value};
pub(super) use sha2::{Digest, Sha256};
pub(super) use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};
pub(super) use uuid::Uuid;

pub(super) use crate::support::{
    agent_connections::get_agent_connection_id_for,
    evidence_documents::{VALID_FROM, VALID_UNTIL},
    harness::{self, TestApp},
    http::local_path,
    json::{assert_rfc3339, object_keys},
    mcp::{assert_not_found, assert_validation_error, McpClient},
    oauth::authorize_agent_connection,
    scenario::ScenarioBuilder,
};

pub(super) const MAX_DOCUMENT_BYTES: u64 = 25 * 1024 * 1024;
pub(super) const CONTENT_TYPE: &str = "text/plain";
pub(super) const PERMISSIONS: &[WorkspacePermission] = &[
    WorkspacePermission::ReadEvidenceSubmissions,
    WorkspacePermission::WriteEvidenceSubmissions,
];
