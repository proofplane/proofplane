use std::{sync::Arc, time::Duration};

use axum::{extract::MatchedPath, http::Request, middleware, response::Response, Router};
use metrics_exporter_prometheus::PrometheusHandle;
use tower_http::trace::TraceLayer;
use tracing::Span;

use crate::{
    authentication::ApiKeyAuthenticator,
    authorization::workspaces::WorkspaceAuthorizer,
    config::AppConfig,
    repository::Postgres,
    routes::{
        controls::{self, ControlRouteAuthState, ControlState},
        error::not_found,
        evidence_requests::{self, EvidenceRequestRouteAuthState, EvidenceRequestState},
        evidence_submissions::{self, EvidenceSubmissionRouteAuthState, EvidenceSubmissionState},
        health::{self, ReadyState},
        metrics::{self, MetricsState},
        request_context::attach_request_id,
        version,
    },
    services::{
        controls::ControlService, evidence_requests::EvidenceRequestService,
        evidence_submissions::EvidenceSubmissionService,
    },
};

pub struct AppDependencies {
    pub config: AppConfig,
    pub postgres: Arc<Postgres>,
    pub metrics: PrometheusHandle,
    pub authenticator: ApiKeyAuthenticator,
    pub workspace_authorizer: WorkspaceAuthorizer,
}

pub fn create_app(dependencies: AppDependencies) -> Result<Router, crate::authentication::Error> {
    let live_path = dependencies.config.health.live_path.clone();
    let ready_path = dependencies.config.health.ready_path.clone();

    Ok(Router::new()
        .nest(&live_path, health::livez_router())
        .nest(
            &ready_path,
            health::readyz_router(ReadyState {
                postgres: dependencies.postgres.clone(),
                dependency_timeout_ms: dependencies.config.health.dependency_timeout_ms,
            }),
        )
        .nest(
            "/metrics",
            metrics::router(MetricsState {
                handle: dependencies.metrics,
            }),
        )
        .merge(evidence_requests::router(EvidenceRequestState {
            service: EvidenceRequestService::new(dependencies.postgres.clone()),
            route_auth: EvidenceRequestRouteAuthState {
                authenticator: dependencies.authenticator.clone(),
                authorizer: dependencies.workspace_authorizer.clone(),
            },
        }))
        .merge(evidence_submissions::router(EvidenceSubmissionState {
            service: EvidenceSubmissionService::new(dependencies.postgres.clone()),
            route_auth: EvidenceSubmissionRouteAuthState {
                authenticator: dependencies.authenticator.clone(),
                authorizer: dependencies.workspace_authorizer.clone(),
            },
        }))
        .merge(controls::router(ControlState {
            service: ControlService::new(dependencies.postgres.clone()),
            route_auth: ControlRouteAuthState {
                authenticator: dependencies.authenticator,
                authorizer: dependencies.workspace_authorizer,
            },
        }))
        .nest("/version", version::router())
        .fallback(not_found)
        .layer(middleware::from_fn(attach_request_id))
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(|request: &Request<_>| {
                    let method = request.method();
                    let path = request
                        .extensions()
                        .get::<MatchedPath>()
                        .map(MatchedPath::as_str)
                        .unwrap_or_else(|| request.uri().path());

                    tracing::info_span!(
                        "http_request",
                        %method,
                        path,
                        request_id = tracing::field::Empty,
                        actor_id = tracing::field::Empty
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
