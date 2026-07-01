use std::sync::Arc;

use axum::{middleware, Router};
use metrics_exporter_prometheus::PrometheusHandle;
use rmcp::transport::streamable_http_server::{
    session::local::LocalSessionManager, StreamableHttpServerConfig, StreamableHttpService,
};
use tokio_util::sync::CancellationToken;

use super::{auth::authenticate_request, server::ProofplaneMcp};
use crate::authentication::paseto::{
    DownloadGrantDecryptor, DownloadGrantEncryptor, UploadGrantDecryptor, UploadGrantEncryptor,
};
use crate::{
    authentication::ApiTokenAuthenticator,
    config::HealthConfig,
    object_storage::FilesystemObjectStore,
    repository::Postgres,
    routes::{
        health::{self, ReadyState},
        metrics::{self, MetricsState},
        request_context::attach_request_id,
    },
    services::{
        attachment_upload_grants::AttachmentUploadGrantService, controls::ControlService,
        evidence_requests::EvidenceRequestService, evidence_submissions::EvidenceSubmissionService,
    },
};
use url::Url;

pub const ENDPOINT: &str = "/mcp";

#[derive(Clone)]
pub struct McpAppDependencies {
    pub postgres: Arc<Postgres>,
    pub object_store: Arc<FilesystemObjectStore>,
    pub metrics: PrometheusHandle,
    pub authenticator: Arc<ApiTokenAuthenticator>,
    pub public_api_base_url: Url,
    pub download_grant_encryptor: DownloadGrantEncryptor,
    pub download_grant_decryptor: DownloadGrantDecryptor,
    pub upload_grant_encryptor: UploadGrantEncryptor,
    pub upload_grant_decryptor: UploadGrantDecryptor,
    pub health: HealthConfig,
    pub cancellation_token: CancellationToken,
}

pub fn create_app(dependencies: McpAppDependencies) -> Router {
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
        ProofplaneMcp::new(
            evidence_requests,
            evidence_submissions,
            attachment_upload_grants,
            controls,
        ),
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
    server: ProofplaneMcp,
    cancellation_token: CancellationToken,
) -> Router {
    let server_factory = move || Ok(server.clone());
    let transport = StreamableHttpService::<ProofplaneMcp, LocalSessionManager>::new(
        server_factory,
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
