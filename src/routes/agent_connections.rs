use axum::{
    extract::{Path, Request, State},
    http::StatusCode,
    middleware::{self, Next},
    response::Response,
    routing::{delete, get},
    Extension, Json, Router,
};
use chrono::{DateTime, Utc};
use serde::Serialize;
use url::Url;
use uuid::Uuid;

use crate::{
    application::{
        commands::agent_connections::{
            AgentConnectionCommandError, RevokeAgentConnection, RevokeAgentConnectionHandler,
        },
        queries::agent_connections::{ListUserAgentConnections, ListUserAgentConnectionsHandler},
        ExecutionMetadata,
    },
    authentication::{
        auth0::{TokenVerifier, VerifiedClaims},
        UserContext,
    },
    domain::AgentConnectionId,
    observability::audit::{AuditActor, AuditClientType, AuditEvent, AuditObject, AuditOutcome},
    read_models::UserAgentConnectionSummary,
    routes::{
        authentication::authenticate_user, error::ApiError, me::UserRouteAuthState,
        request_context::RequestId,
    },
};

pub struct AgentConnectionsState<V: TokenVerifier<Claims = VerifiedClaims>> {
    pub list: ListUserAgentConnectionsHandler,
    pub revoke: RevokeAgentConnectionHandler,
    pub route_auth: UserRouteAuthState<V>,
    pub mcp_url: Url,
}

impl<V: TokenVerifier<Claims = VerifiedClaims>> Clone for AgentConnectionsState<V> {
    fn clone(&self) -> Self {
        Self {
            list: self.list.clone(),
            revoke: self.revoke.clone(),
            route_auth: self.route_auth.clone(),
            mcp_url: self.mcp_url.clone(),
        }
    }
}

pub fn router<V: TokenVerifier<Claims = VerifiedClaims> + 'static>(
    state: AgentConnectionsState<V>,
) -> Router {
    let route_auth = state.route_auth.clone();
    Router::new()
        .route("/agent-connections", get(list_connections::<V>))
        .route("/agent-connections/{id}", delete(revoke_connection::<V>))
        .route_layer(middleware::from_fn_with_state(
            route_auth,
            authenticate_user_route::<V>,
        ))
        .with_state(state)
}

async fn authenticate_user_route<V: TokenVerifier<Claims = VerifiedClaims>>(
    State(state): State<UserRouteAuthState<V>>,
    mut request: Request,
    next: Next,
) -> Result<Response, ApiError> {
    authenticate_user(&state.authenticator, &mut request).await?;
    Ok(next.run(request).await)
}

async fn list_connections<V: TokenVerifier<Claims = VerifiedClaims>>(
    State(state): State<AgentConnectionsState<V>>,
    Extension(user): Extension<UserContext>,
) -> Result<Json<AgentConnectionsResponse>, ApiError> {
    let connections = state
        .list
        .handle(ListUserAgentConnections {
            user_id: user.user_id,
        })
        .await
        .map_err(map_query_error)?;
    Ok(Json(AgentConnectionsResponse {
        mcp_url: state.mcp_url.to_string(),
        connections: connections.into_iter().map(Into::into).collect(),
    }))
}

async fn revoke_connection<V: TokenVerifier<Claims = VerifiedClaims>>(
    State(state): State<AgentConnectionsState<V>>,
    Extension(user): Extension<UserContext>,
    Extension(request_id): Extension<RequestId>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    let id = AgentConnectionId::from(id);
    if !state
        .revoke
        .handle(
            RevokeAgentConnection {
                connection_id: id,
                user_id: Some(user.user_id),
            },
            ExecutionMetadata::for_request(request_id.0),
        )
        .await
        .map_err(map_command_error)?
    {
        return Err(ApiError::NotFound);
    }

    AuditEvent::new(
        "agent_connection.revoked",
        AuditOutcome::Success,
        AuditActor::User {
            user_id: user.user_id.into(),
        },
        AuditClientType::Rest,
        "revoke_agent_connection",
    )
    .request_id(request_id.0)
    .object(AuditObject::new("agent_connection", id.into()))
    .emit();

    Ok(StatusCode::NO_CONTENT)
}

fn map_command_error(error: AgentConnectionCommandError) -> ApiError {
    tracing::error!(%error, "agent connection operation failed");
    ApiError::Internal
}

fn map_query_error(error: crate::persistence::Error) -> ApiError {
    tracing::error!(%error, "agent connection query failed");
    ApiError::Internal
}

#[derive(Serialize)]
struct AgentConnectionsResponse {
    mcp_url: String,
    connections: Vec<AgentConnectionResponse>,
}

#[derive(Serialize)]
struct AgentConnectionResponse {
    id: Uuid,
    client_name: String,
    status: &'static str,
    authorized_at: DateTime<Utc>,
    last_used_at: Option<DateTime<Utc>>,
}

impl From<UserAgentConnectionSummary> for AgentConnectionResponse {
    fn from(connection: UserAgentConnectionSummary) -> Self {
        Self {
            id: connection.id.into(),
            client_name: connection.client_name,
            status: connection.status.as_str(),
            authorized_at: connection.authorized_at,
            last_used_at: connection.last_used_at,
        }
    }
}
