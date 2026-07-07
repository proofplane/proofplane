use std::{sync::Arc, time::Duration};

use axum::{
    extract::MatchedPath,
    http::{header::InvalidHeaderValue, HeaderValue, Request},
    middleware,
    response::Response,
    Router,
};
use metrics_exporter_prometheus::PrometheusHandle;
use rmcp::transport::streamable_http_server::{
    session::local::LocalSessionManager, StreamableHttpServerConfig, StreamableHttpService,
};
use tokio_util::sync::CancellationToken;
use tower_http::{cors::CorsLayer, trace::TraceLayer};
use tracing::Span;

use super::{
    auth::{authenticate_request, AuthenticationState},
    server::ProofplaneMcp,
};
use crate::authentication::paseto::{
    DownloadGrantDecryptor, DownloadGrantEncryptor, UploadGrantDecryptor, UploadGrantEncryptor,
};
use crate::{
    authentication::{
        auth0::{TokenVerifier, VerifiedMcpClaims},
        ApiTokenAuthenticator,
    },
    config::{Auth0McpConfig, HealthConfig},
    domain::WorkspacePermission,
    object_storage::FilesystemObjectStore,
    repository::Postgres,
    routes::{
        health::{self, ReadyState},
        metrics::{self, MetricsState},
        protected_resource_metadata::{
            self, ProtectedResourceMetadataState, PROTECTED_RESOURCE_METADATA_ENDPOINT,
        },
        request_context::attach_request_id,
    },
    services::{
        attachment_upload_grants::AttachmentUploadGrantService, controls::ControlService,
        evidence_requests::EvidenceRequestService, evidence_submissions::EvidenceSubmissionService,
    },
};
use url::Url;

pub const ENDPOINT: &str = "/mcp";

#[derive(Debug, thiserror::Error)]
pub enum McpAppError {
    #[error("MCP resource metadata URL cannot be constructed")]
    ResourceMetadataUrl(#[source] url::ParseError),
    #[error("MCP authentication challenge is not a valid HTTP header")]
    AuthenticationChallenge(#[source] InvalidHeaderValue),
}

pub struct McpAppDependencies<V> {
    pub postgres: Arc<Postgres>,
    pub object_store: Arc<FilesystemObjectStore>,
    pub metrics: PrometheusHandle,
    pub authenticator: Arc<ApiTokenAuthenticator>,
    pub auth0_verifier: Arc<V>,
    pub auth0_issuer: Url,
    pub auth0_mcp: Auth0McpConfig,
    pub public_api_base_url: Url,
    pub download_grant_encryptor: DownloadGrantEncryptor,
    pub download_grant_decryptor: DownloadGrantDecryptor,
    pub upload_grant_encryptor: UploadGrantEncryptor,
    pub upload_grant_decryptor: UploadGrantDecryptor,
    pub health: HealthConfig,
    pub cancellation_token: CancellationToken,
}

impl<V> Clone for McpAppDependencies<V> {
    fn clone(&self) -> Self {
        Self {
            postgres: self.postgres.clone(),
            object_store: self.object_store.clone(),
            metrics: self.metrics.clone(),
            authenticator: self.authenticator.clone(),
            auth0_verifier: self.auth0_verifier.clone(),
            auth0_issuer: self.auth0_issuer.clone(),
            auth0_mcp: self.auth0_mcp.clone(),
            public_api_base_url: self.public_api_base_url.clone(),
            download_grant_encryptor: self.download_grant_encryptor.clone(),
            download_grant_decryptor: self.download_grant_decryptor.clone(),
            upload_grant_encryptor: self.upload_grant_encryptor.clone(),
            upload_grant_decryptor: self.upload_grant_decryptor.clone(),
            health: self.health.clone(),
            cancellation_token: self.cancellation_token.clone(),
        }
    }
}

pub fn create_app<V>(dependencies: McpAppDependencies<V>) -> Result<Router, McpAppError>
where
    V: TokenVerifier<Claims = VerifiedMcpClaims> + 'static,
{
    let evidence_requests = EvidenceRequestService::new(dependencies.postgres.clone());
    let evidence_submissions = EvidenceSubmissionService::new(
        dependencies.postgres.clone(),
        dependencies.object_store.clone(),
    );
    let attachment_upload_grants = AttachmentUploadGrantService::new(
        dependencies.postgres.clone(),
        dependencies.public_api_base_url,
        dependencies.upload_grant_encryptor,
        dependencies.upload_grant_decryptor,
    );
    let controls = ControlService::new(dependencies.postgres.clone());
    let protocol = protocol_router(
        dependencies.authenticator,
        dependencies.auth0_verifier,
        dependencies.auth0_mcp.clone(),
        ProofplaneMcp::new(
            evidence_requests,
            evidence_submissions,
            attachment_upload_grants,
            controls,
        ),
        dependencies.cancellation_token.clone(),
    )?;
    let protected_resource_metadata =
        protected_resource_metadata::router(ProtectedResourceMetadataState {
            resource: dependencies.auth0_mcp.resource.clone(),
            authorization_server: dependencies.auth0_issuer.clone(),
        });

    Ok(Router::new()
        .merge(protocol)
        .merge(protected_resource_metadata)
        .nest(&dependencies.health.live_path, health::livez_router())
        .nest(
            &dependencies.health.ready_path,
            health::readyz_router(ReadyState {
                postgres: dependencies.postgres,
                dependency_timeout_ms: dependencies.health.dependency_timeout_ms,
            }),
        )
        .nest(
            "/metrics",
            metrics::router(MetricsState {
                handle: dependencies.metrics,
            }),
        )
        .layer(middleware::from_fn(attach_request_id))
        .layer(CorsLayer::permissive())
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(|request: &Request<_>| {
                    let method = request.method();
                    let path = trace_path(request);

                    tracing::info_span!(
                        "http_request",
                        %method,
                        path,
                        request_id = tracing::field::Empty,
                        api_token_id = tracing::field::Empty,
                        user_id = tracing::field::Empty
                    )
                })
                .on_response(|response: &Response, latency: Duration, span: &Span| {
                    tracing::info!(
                        parent: span,
                        status = response.status().as_u16(),
                        latency_ms = latency.as_secs_f64() * 1000.0,
                        "http request completed"
                    );
                }),
        ))
}

pub fn protocol_router<V>(
    authenticator: Arc<ApiTokenAuthenticator>,
    auth0_verifier: Arc<V>,
    auth0_config: Auth0McpConfig,
    server: ProofplaneMcp,
    cancellation_token: CancellationToken,
) -> Result<Router, McpAppError>
where
    V: TokenVerifier<Claims = VerifiedMcpClaims> + 'static,
{
    let challenge = authentication_challenge(&auth0_config)?;
    let server_factory = move || Ok(server.clone());
    let transport = StreamableHttpService::<ProofplaneMcp, LocalSessionManager>::new(
        server_factory,
        Default::default(),
        StreamableHttpServerConfig::default()
            .with_cancellation_token(cancellation_token.child_token()),
    );

    Ok(Router::new()
        .nest_service(ENDPOINT, transport)
        .layer(middleware::from_fn_with_state(
            AuthenticationState {
                api_tokens: authenticator,
                auth0: auth0_verifier,
                challenge,
            },
            authenticate_request,
        )))
}

fn authentication_challenge(config: &Auth0McpConfig) -> Result<HeaderValue, McpAppError> {
    let scopes = WorkspacePermission::ALL
        .iter()
        .map(|permission| permission.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    let metadata = config
        .resource
        .join(PROTECTED_RESOURCE_METADATA_ENDPOINT)
        .map_err(McpAppError::ResourceMetadataUrl)?;
    let challenge = format!(
        "Bearer realm=\"proofplane-mcp\", resource_metadata=\"{metadata}\", scope=\"{scopes}\""
    );
    HeaderValue::from_str(&challenge).map_err(McpAppError::AuthenticationChallenge)
}

fn trace_path<B>(request: &Request<B>) -> &str {
    request
        .extensions()
        .get::<MatchedPath>()
        .map(MatchedPath::as_str)
        .unwrap_or_else(|| request.uri().path())
}

#[cfg(test)]
mod tests {
    use axum::{body::Body, http::Request};
    use url::Url;

    use super::{authentication_challenge, trace_path, Auth0McpConfig, McpAppError};

    #[test]
    fn http_trace_path_never_contains_query_parameters() {
        let request = Request::builder()
            .uri("/mcp?session_id=secret")
            .body(Body::empty())
            .unwrap();

        assert_eq!(trace_path(&request), "/mcp");
    }

    #[test]
    fn authentication_challenge_rejects_non_hierarchical_resource_url() {
        let config = Auth0McpConfig {
            resource: Url::parse("mailto:mcp@proofplane.com").unwrap(),
            allowed_client_ids: vec!["client-123".to_owned()],
        };

        assert!(matches!(
            authentication_challenge(&config),
            Err(McpAppError::ResourceMetadataUrl(_))
        ));
    }
}
