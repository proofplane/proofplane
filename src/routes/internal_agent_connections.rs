use axum::{
    extract::{rejection::JsonRejection, State},
    http::{header, HeaderMap},
    routing::post,
    Json, Router,
};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use url::Url;

use crate::{
    domain::{canonical_permissions, AgentConnection, WorkspacePermission},
    routes::error::ApiError,
    services::agent_connections::{
        AgentConnectionError, AgentConnectionService, ConsumeContinuationOutcome,
        ConsumeContinuationPayload, FindReusableConnectionPayload,
    },
    validate,
    validation::Validation,
};

#[derive(Clone)]
pub struct InternalAgentConnectionsState {
    pub service: AgentConnectionService,
    pub action_shared_secret: SecretString,
}

pub fn router(state: InternalAgentConnectionsState) -> Router {
    Router::new()
        .route(
            "/internal/auth0-actions/agent-connections/resolve",
            post(resolve),
        )
        .route(
            "/internal/auth0-actions/agent-connections/continuations/consume",
            post(consume_continuation),
        )
        .with_state(state)
}

#[derive(Debug, Deserialize)]
struct ResolveRequest {
    subject: String,
    client_id: String,
    resource: String,
    scopes: Vec<String>,
}

impl ResolveRequest {
    fn into_payload(self) -> Validation<FindReusableConnectionPayload, String> {
        validate! {
            auth0_subject <- required("subject", self.subject),
            auth0_client_id <- required("client_id", self.client_id),
            resource <- canonical_resource(self.resource),
            permissions <- parse_scopes(self.scopes),
            => FindReusableConnectionPayload {
                auth0_subject,
                auth0_client_id,
                resource,
                permissions,
            },
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
enum ResolveResponse {
    Reusable {
        connection_id: String,
        workspace_id: String,
        scopes: Vec<String>,
    },
    InteractionRequired,
}

async fn resolve(
    State(state): State<InternalAgentConnectionsState>,
    headers: HeaderMap,
    payload: Result<Json<ResolveRequest>, JsonRejection>,
) -> Result<Json<ResolveResponse>, ApiError> {
    authenticate_action(&headers, &state.action_shared_secret)?;
    let Json(payload) = payload.map_err(json_rejection)?;
    let payload = payload
        .into_payload()
        .into_result()
        .map_err(ApiError::BadRequest)?;

    let connection = state
        .service
        .find_reusable(payload)
        .await
        .map_err(service_error)?;

    Ok(Json(match connection {
        Some(connection) => reusable_response(connection),
        None => ResolveResponse::InteractionRequired,
    }))
}

#[derive(Debug, Deserialize)]
struct ConsumeContinuationRequest {
    continuation_token: String,
    nonce: String,
}

impl ConsumeContinuationRequest {
    fn into_payload(self) -> Validation<ConsumeContinuationPayload, String> {
        validate! {
            continuation_token <- required("continuation_token", self.continuation_token),
            nonce <- required("nonce", self.nonce),
            => ConsumeContinuationPayload {
                continuation_token,
                nonce,
            },
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
enum ConsumeContinuationResponse {
    Approved {
        connection_id: String,
        workspace_id: String,
        subject: String,
        client_id: String,
        resource: String,
        scopes: Vec<String>,
    },
    InvalidContinuation,
}

async fn consume_continuation(
    State(state): State<InternalAgentConnectionsState>,
    headers: HeaderMap,
    payload: Result<Json<ConsumeContinuationRequest>, JsonRejection>,
) -> Result<Json<ConsumeContinuationResponse>, ApiError> {
    authenticate_action(&headers, &state.action_shared_secret)?;
    let Json(payload) = payload.map_err(json_rejection)?;
    let payload = payload
        .into_payload()
        .into_result()
        .map_err(ApiError::BadRequest)?;

    let connection = state
        .service
        .consume_continuation(payload)
        .await
        .map_err(service_error)?;

    Ok(Json(match connection {
        ConsumeContinuationOutcome::Approved(connection) => {
            let scopes = scope_strings(&connection);
            ConsumeContinuationResponse::Approved {
                connection_id: connection.id.to_string(),
                workspace_id: connection.workspace_id.to_string(),
                subject: connection.auth0_subject,
                client_id: connection.auth0_client_id,
                resource: connection.resource,
                scopes,
            }
        }
        ConsumeContinuationOutcome::Invalid => ConsumeContinuationResponse::InvalidContinuation,
    }))
}

fn reusable_response(connection: AgentConnection) -> ResolveResponse {
    ResolveResponse::Reusable {
        connection_id: connection.id.to_string(),
        workspace_id: connection.workspace_id.to_string(),
        scopes: scope_strings(&connection),
    }
}

fn scope_strings(connection: &AgentConnection) -> Vec<String> {
    connection
        .permissions
        .iter()
        .map(|permission| permission.as_str().to_owned())
        .collect()
}

fn parse_scopes(values: Vec<String>) -> Validation<Vec<WorkspacePermission>, String> {
    if values.is_empty() {
        return Validation::invalid("scopes must not be empty".to_owned());
    }
    let mut parsed = Vec::with_capacity(values.len());
    let mut errors = Vec::new();
    for value in values {
        match value.parse::<WorkspacePermission>() {
            Ok(permission) if parsed.contains(&permission) => {
                errors.push(format!("scopes contains duplicate value {value}"));
            }
            Ok(permission) => parsed.push(permission),
            Err(_) => errors.push(format!("unknown scope: {value}")),
        }
    }
    if !errors.is_empty() {
        return Validation::invalid_many(errors);
    }

    match canonical_permissions(parsed) {
        Ok(permissions) => Validation::valid(permissions),
        Err(error) => Validation::invalid(error.to_string()),
    }
}

fn canonical_resource(value: String) -> Validation<String, String> {
    let value = match required("resource", value) {
        Validation::Valid(value) => value,
        Validation::Invalid(errors) => return Validation::Invalid(errors),
    };
    let url = match Url::parse(&value) {
        Ok(url) => url,
        Err(_) => {
            return Validation::invalid("resource must be an absolute URL".to_owned());
        }
    };
    if url.query().is_some() || url.fragment().is_some() || url.as_str() != value {
        return Validation::invalid(
            "resource must be a canonical URL without query or fragment".to_owned(),
        );
    }
    Validation::valid(value)
}

fn required(field: &str, value: String) -> Validation<String, String> {
    if value.trim().is_empty() {
        return Validation::invalid(format!("{field} must not be blank"));
    }
    Validation::valid(value)
}

fn authenticate_action(headers: &HeaderMap, expected: &SecretString) -> Result<(), ApiError> {
    let provided = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .filter(|value| !value.is_empty())
        .ok_or(ApiError::Unauthorized)?;
    let provided_digest = Sha256::digest(provided.as_bytes());
    let expected_digest = Sha256::digest(expected.expose_secret().as_bytes());
    let equal = provided_digest
        .iter()
        .zip(expected_digest.iter())
        .fold(0_u8, |difference, (left, right)| {
            difference | (left ^ right)
        })
        == 0;
    if !equal {
        return Err(ApiError::Unauthorized);
    }
    Ok(())
}

fn json_rejection(error: JsonRejection) -> ApiError {
    bad_request(error.body_text())
}

fn service_error(error: AgentConnectionError) -> ApiError {
    match error {
        AgentConnectionError::PolicyRejected
        | AgentConnectionError::AlreadyExists
        | AgentConnectionError::Repository(_) => {
            tracing::error!(%error, "internal agent connection request failed");
            ApiError::Internal
        }
    }
}

fn bad_request(message: impl Into<String>) -> ApiError {
    ApiError::BadRequest(vec![message.into()])
}

#[cfg(test)]
mod tests {
    use axum::http::{header, HeaderMap, HeaderValue};
    use secrecy::SecretString;

    use super::{authenticate_action, ConsumeContinuationRequest, ResolveRequest};
    use crate::{
        domain::WorkspacePermission,
        services::agent_connections::{ConsumeContinuationPayload, FindReusableConnectionPayload},
    };

    #[test]
    fn action_authentication_requires_exact_bearer_secret() {
        let expected = SecretString::from("01234567890123456789012345678901");
        let mut headers = HeaderMap::new();
        assert!(authenticate_action(&headers, &expected).is_err());

        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Bearer wrong"),
        );
        assert!(authenticate_action(&headers, &expected).is_err());

        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Bearer 01234567890123456789012345678901"),
        );
        assert!(authenticate_action(&headers, &expected).is_ok());
    }

    #[test]
    fn resolve_request_maps_to_canonical_payload() {
        let payload = ResolveRequest {
            subject: "auth0|user".to_owned(),
            client_id: "mcp-client".to_owned(),
            resource: "https://mcp.proofplane.com/mcp".to_owned(),
            scopes: vec![
                "write_controls".to_owned(),
                "read_evidence_requests".to_owned(),
            ],
        }
        .into_payload()
        .into_result()
        .expect("request validates");

        assert_find_reusable(
            payload,
            vec![
                WorkspacePermission::ReadEvidenceRequests,
                WorkspacePermission::WriteControls,
            ],
        );
    }

    #[test]
    fn resolve_request_accumulates_required_url_and_empty_scope_errors() {
        let errors = ResolveRequest {
            subject: " ".to_owned(),
            client_id: "\t".to_owned(),
            resource: "not-an-absolute-url".to_owned(),
            scopes: Vec::new(),
        }
        .into_payload()
        .into_result()
        .expect_err("request is invalid");

        assert_eq!(
            errors,
            vec![
                "subject must not be blank",
                "client_id must not be blank",
                "resource must be an absolute URL",
                "scopes must not be empty",
            ]
        );
    }

    #[test]
    fn resolve_request_rejects_noncanonical_resource_and_unknown_scopes() {
        let errors = ResolveRequest {
            subject: "auth0|user".to_owned(),
            client_id: "mcp-client".to_owned(),
            resource: "https://mcp.proofplane.com/mcp?query=value".to_owned(),
            scopes: vec!["delete_everything".to_owned(), "admin".to_owned()],
        }
        .into_payload()
        .into_result()
        .expect_err("request is invalid");

        assert_eq!(
            errors,
            vec![
                "resource must be a canonical URL without query or fragment",
                "unknown scope: delete_everything",
                "unknown scope: admin",
            ]
        );
    }

    #[test]
    fn resolve_request_rejects_duplicate_scopes() {
        let errors = ResolveRequest {
            subject: "auth0|user".to_owned(),
            client_id: "mcp-client".to_owned(),
            resource: "https://mcp.proofplane.com/mcp".to_owned(),
            scopes: vec!["read_controls".to_owned(), "read_controls".to_owned()],
        }
        .into_payload()
        .into_result()
        .expect_err("request is invalid");

        assert_eq!(
            errors,
            vec!["scopes contains duplicate value read_controls"]
        );
    }

    #[test]
    fn continuation_request_maps_to_payload_and_accumulates_blank_fields() {
        let payload = ConsumeContinuationRequest {
            continuation_token: "continuation".to_owned(),
            nonce: "nonce".to_owned(),
        }
        .into_payload()
        .into_result()
        .expect("request validates");
        let ConsumeContinuationPayload {
            continuation_token,
            nonce,
        } = payload;
        assert_eq!(continuation_token, "continuation");
        assert_eq!(nonce, "nonce");

        let errors = ConsumeContinuationRequest {
            continuation_token: " ".to_owned(),
            nonce: "\t".to_owned(),
        }
        .into_payload()
        .into_result()
        .expect_err("request is invalid");
        assert_eq!(
            errors,
            vec![
                "continuation_token must not be blank",
                "nonce must not be blank",
            ]
        );
    }

    fn assert_find_reusable(
        payload: FindReusableConnectionPayload,
        expected_permissions: Vec<WorkspacePermission>,
    ) {
        assert_eq!(payload.auth0_subject, "auth0|user");
        assert_eq!(payload.auth0_client_id, "mcp-client");
        assert_eq!(payload.resource, "https://mcp.proofplane.com/mcp");
        assert_eq!(payload.permissions, expected_permissions);
    }
}
