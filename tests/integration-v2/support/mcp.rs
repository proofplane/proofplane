use std::collections::HashMap;

use axum_test::TestServer;
use http::{HeaderName, HeaderValue};
use proofplane::{mcp::ENDPOINT, routes::request_context::REQUEST_ID_HEADER};
use rmcp::{
    model::{CallToolRequestParams, ClientInfo, ErrorCode, JsonObject, ReadResourceRequestParams},
    service::{RoleClient, RunningService},
    transport::{
        streamable_http_client::StreamableHttpClientTransportConfig, StreamableHttpClientTransport,
    },
    ServiceError, ServiceExt,
};
use serde_json::{json, Value};
use uuid::Uuid;

pub struct McpClient {
    service: RunningService<RoleClient, ClientInfo>,
}

/// The `problem` payload a tool returns when it refuses, alongside the JSON-RPC
/// code the client sees.
pub struct McpError {
    pub code: ErrorCode,
    pub data: Value,
}

#[track_caller]
pub fn assert_validation_error(error: &McpError, field_issues: Value) {
    assert_eq!(error.code, ErrorCode::INVALID_PARAMS);
    assert_eq!(
        error.data,
        json!({
            "problem": {
                "code": "validation_failed",
                "message": "tool argument validation failed",
                "field_issues": field_issues,
            }
        })
    );
}

#[track_caller]
pub fn assert_not_found(error: &McpError) {
    assert_eq!(error.code, ErrorCode::RESOURCE_NOT_FOUND);
    assert_eq!(
        error.data,
        json!({
            "problem": {
                "code": "not_found",
                "message": "resource not found",
            }
        })
    );
}

impl McpClient {
    pub async fn connect(server: &TestServer, raw_token: &str) -> Self {
        Self::connect_with_headers(server, raw_token, HashMap::new()).await
    }

    pub async fn connect_with_request_id(
        server: &TestServer,
        raw_token: &str,
        request_id: Uuid,
    ) -> Self {
        let mut headers = HashMap::new();
        headers.insert(
            REQUEST_ID_HEADER,
            HeaderValue::from_str(&request_id.to_string()).expect("request id is a header value"),
        );

        Self::connect_with_headers(server, raw_token, headers).await
    }

    async fn connect_with_headers(
        server: &TestServer,
        raw_token: &str,
        headers: HashMap<HeaderName, HeaderValue>,
    ) -> Self {
        let uri = server
            .server_url(ENDPOINT)
            .expect("MCP server exposes an HTTP URL")
            .to_string();
        let transport = StreamableHttpClientTransport::from_config(
            StreamableHttpClientTransportConfig::with_uri(uri)
                .auth_header(raw_token)
                .custom_headers(headers),
        );

        Self {
            service: ClientInfo::default()
                .serve(transport)
                .await
                .expect("MCP client initializes"),
        }
    }

    pub async fn list_tools(&self) -> Vec<Value> {
        self.service
            .list_tools(None)
            .await
            .expect("tools list succeeds")
            .tools
            .into_iter()
            .map(|tool| serde_json::to_value(tool).expect("tool serializes"))
            .collect()
    }

    pub async fn list_resources(&self) -> Value {
        let result = self
            .service
            .list_resources(None)
            .await
            .expect("resources list succeeds");

        serde_json::to_value(result).expect("resource list serializes")
    }

    pub async fn read_resource(&self, uri: &str) -> Value {
        let result = self
            .service
            .read_resource(ReadResourceRequestParams::new(uri))
            .await
            .unwrap_or_else(|error| panic!("resource {uri} reads: {error:?}"));

        serde_json::to_value(result).expect("resource result serializes")
    }

    pub async fn read_resource_error(&self, uri: &str) -> McpError {
        match self
            .service
            .read_resource(ReadResourceRequestParams::new(uri))
            .await
        {
            Ok(result) => panic!("expected resource {uri} fails, got success: {result:?}"),
            Err(ServiceError::McpError(error)) => McpError {
                code: error.code,
                data: error.data.expect("MCP error has problem data"),
            },
            Err(error) => panic!("expected resource {uri} fails with an MCP error, got: {error:?}"),
        }
    }

    pub async fn call_tool(&self, name: &'static str, arguments: Value) -> Value {
        self.service
            .call_tool(call_tool_params(name, arguments))
            .await
            .unwrap_or_else(|error| panic!("{name} succeeds: {error:?}"))
            .structured_content
            .expect("tool returns structured content")
    }

    pub async fn call_tool_error(&self, name: &'static str, arguments: Value) -> McpError {
        match self
            .service
            .call_tool(call_tool_params(name, arguments))
            .await
        {
            Ok(result) => panic!("expected {name} fails, got success: {result:?}"),
            Err(ServiceError::McpError(error)) => McpError {
                code: error.code,
                data: error.data.expect("MCP error has problem data"),
            },
            Err(error) => panic!("expected {name} fails with an MCP error, got: {error:?}"),
        }
    }
}

fn call_tool_params(name: &'static str, arguments: Value) -> CallToolRequestParams {
    let arguments = match arguments {
        Value::Object(arguments) => arguments,
        other => panic!("tool arguments must be a JSON object: {other}"),
    };

    CallToolRequestParams::new(name).with_arguments(JsonObject::from(arguments))
}
