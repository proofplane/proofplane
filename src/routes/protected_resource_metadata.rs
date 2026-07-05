use axum::{extract::State, routing::get, Json, Router};
use serde::Serialize;
use url::Url;

use crate::domain::WorkspacePermission;

pub const PROTECTED_RESOURCE_METADATA_ENDPOINT: &str = "/.well-known/oauth-protected-resource/mcp";

#[derive(Clone)]
pub struct ProtectedResourceMetadataState {
    pub resource: Url,
    pub authorization_server: Url,
}

#[derive(Serialize)]
struct ProtectedResourceMetadata {
    resource: String,
    authorization_servers: Vec<String>,
    bearer_methods_supported: Vec<&'static str>,
    scopes_supported: Vec<&'static str>,
}

pub fn router(state: ProtectedResourceMetadataState) -> Router {
    Router::new()
        .route(
            PROTECTED_RESOURCE_METADATA_ENDPOINT,
            get(protected_resource_metadata),
        )
        .with_state(state)
}

async fn protected_resource_metadata(
    State(state): State<ProtectedResourceMetadataState>,
) -> Json<ProtectedResourceMetadata> {
    Json(ProtectedResourceMetadata {
        resource: state.resource.to_string(),
        authorization_servers: vec![state.authorization_server.to_string()],
        bearer_methods_supported: vec!["header"],
        scopes_supported: WorkspacePermission::ALL
            .iter()
            .map(|permission| permission.as_str())
            .collect(),
    })
}
