use axum::{
    extract::{rejection::JsonRejection, State},
    http::{header, HeaderMap},
    routing::post,
    Json, Router,
};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{
    domain::{canonical_permissions, AgentConnection, WorkspacePermission},
    routes::error::ApiError,
    services::agent_connections::{AgentConnectionError, AgentConnectionService},
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
    let Json(request) = payload.map_err(json_rejection)?;
    let subject = required("subject", request.subject)?;
    let client_id = required("client_id", request.client_id)?;
    let resource = canonical_resource(request.resource)?;
    let scopes = parse_scopes(request.scopes)?;

    let connection = state
        .service
        .find_reusable(&subject, &client_id, &resource, scopes)
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
    let Json(request) = payload.map_err(json_rejection)?;
    let continuation_token = required("continuation_token", request.continuation_token)?;
    let nonce = required("nonce", request.nonce)?;

    let connection = state
        .service
        .consume_continuation(&continuation_token, &nonce)
        .await
        .map_err(service_error)?;

    Ok(Json(match connection {
        Some(connection) => {
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
        None => ConsumeContinuationResponse::InvalidContinuation,
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

fn parse_scopes(values: Vec<String>) -> Result<Vec<WorkspacePermission>, ApiError> {
    if values.is_empty() {
        return Err(bad_request("scopes must not be empty"));
    }
    let parsed = values
        .into_iter()
        .map(|value| {
            value
                .parse()
                .map_err(|_| bad_request(format!("unknown scope: {value}")))
        })
        .collect::<Result<Vec<_>, _>>()?;
    canonical_permissions(parsed).map_err(|error| bad_request(error.to_string()))
}

fn canonical_resource(value: String) -> Result<String, ApiError> {
    let value = required("resource", value)?;
    let url =
        url::Url::parse(&value).map_err(|_| bad_request("resource must be an absolute URL"))?;
    if url.query().is_some() || url.fragment().is_some() || url.as_str() != value {
        return Err(bad_request(
            "resource must be a canonical URL without query or fragment",
        ));
    }
    Ok(value)
}

fn required(field: &str, value: String) -> Result<String, ApiError> {
    if value.trim().is_empty() {
        return Err(bad_request(format!("{field} must not be blank")));
    }
    Ok(value)
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
        AgentConnectionError::Invalid(message) => bad_request(message),
        AgentConnectionError::AlreadyExists | AgentConnectionError::Repository(_) => {
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

    use super::authenticate_action;

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
}
