use std::sync::Arc;

use axum::{middleware, Router};
use metrics_exporter_prometheus::PrometheusHandle;
use rmcp::transport::streamable_http_server::{
    session::local::LocalSessionManager, StreamableHttpServerConfig, StreamableHttpService,
};
use tokio_util::sync::CancellationToken;

use super::{auth::authenticate_request, server::ProofplaneMcp};
use crate::{
    authentication::ApiTokenAuthenticator,
    config::HealthConfig,
    repository::Postgres,
    routes::{
        health::{self, ReadyState},
        metrics::{self, MetricsState},
        request_context::attach_request_id,
    },
};

pub const ENDPOINT: &str = "/mcp";

#[derive(Clone)]
pub struct McpAppDependencies {
    pub postgres: Arc<Postgres>,
    pub metrics: PrometheusHandle,
    pub authenticator: Arc<ApiTokenAuthenticator>,
    pub health: HealthConfig,
    pub cancellation_token: CancellationToken,
}

pub fn create_app(dependencies: McpAppDependencies) -> Router {
    let protocol = protocol_router(
        dependencies.authenticator,
        dependencies.cancellation_token.clone(),
    );

    Router::new()
        .merge(protocol)
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
}

pub fn protocol_router(
    authenticator: Arc<ApiTokenAuthenticator>,
    cancellation_token: CancellationToken,
) -> Router {
    let transport = StreamableHttpService::<ProofplaneMcp, LocalSessionManager>::new(
        || Ok(ProofplaneMcp),
        Default::default(),
        StreamableHttpServerConfig::default()
            .with_cancellation_token(cancellation_token.child_token()),
    );

    Router::new()
        .nest_service(ENDPOINT, transport)
        .layer(middleware::from_fn_with_state(
            authenticator,
            authenticate_request,
        ))
}
