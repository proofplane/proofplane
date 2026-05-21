use std::{sync::Arc, time::Duration};

use axum::{extract::MatchedPath, http::Request, response::Response, Router};
use metrics_exporter_prometheus::PrometheusHandle;
use tower_http::trace::TraceLayer;
use tracing::Span;

use crate::{
    config::AppConfig,
    repository::Postgres,
    routes::{
        error::not_found,
        evidence_requests::{self, EvidenceRequestState},
        health::{self, ReadyState},
        metrics::{self, MetricsState},
        version,
    },
    services::evidence_requests::EvidenceRequestService,
};

pub struct AppDependencies {
    pub config: AppConfig,
    pub postgres: Arc<Postgres>,
    pub metrics: PrometheusHandle,
}

pub fn create_app(dependencies: AppDependencies) -> Router {
    let live_path = dependencies.config.health.live_path.clone();
    let ready_path = dependencies.config.health.ready_path.clone();

    Router::new()
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
        }))
        .nest("/version", version::router())
        .fallback(not_found)
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(|request: &Request<_>| {
                    let method = request.method();
                    let path = request
                        .extensions()
                        .get::<MatchedPath>()
                        .map(MatchedPath::as_str)
                        .unwrap_or_else(|| request.uri().path());

                    tracing::info_span!("http_request", %method, path)
                })
                .on_response(|response: &Response, latency: Duration, span: &Span| {
                    tracing::info!(
                        parent: span,
                        status = response.status().as_u16(),
                        latency_ms = latency.as_secs_f64() * 1000.0,
                        "http request completed"
                    );
                }),
        )
}

// #[cfg(test)]
// mod tests {
//     use axum::{body::Body, http::Request};
//     use secrecy::ExposeSecret;
//     use tower::ServiceExt;

//     use super::*;
//     use crate::{api::build_metrics_handle, store::conn_pool};

//     fn health_config() -> HealthConfig {
//         HealthConfig {
//             live_path: "/livez".to_owned(),
//             ready_path: "/readyz".to_owned(),
//             dependency_timeout_ms: 1,
//         }
//     }

//     fn unreachable_pool() -> Postgres {
//         conn_pool(
//             &secrecy::SecretString::from("postgres://proofplane:proofplane@127.0.0.1:1/proofplane")
//                 .expose_secret(),
//             16,
//         )
//         .await
//         .expect("pool config is valid")
//     }

//     #[tokio::test]
//     async fn liveness_route_uses_configured_path() {
//         let app = create_app(AppDependencies {
//             health: health_config(),
//             postgres: unreachable_pool(),
//             metrics: build_metrics_handle(),
//         });

//         let response = app
//             .oneshot(
//                 Request::builder()
//                     .uri("/livez")
//                     .body(Body::empty())
//                     .expect("request builds"),
//             )
//             .await
//             .expect("request succeeds");

//         assert_eq!(response.status(), axum::http::StatusCode::OK);
//     }

//     #[tokio::test]
//     async fn readiness_returns_unavailable_when_postgres_is_unreachable() {
//         let app = create_app(AppDependencies {
//             health: health_config(),
//             postgres: unreachable_pool(),
//             metrics: build_metrics_handle(),
//         });

//         let response = app
//             .oneshot(
//                 Request::builder()
//                     .uri("/readyz")
//                     .body(Body::empty())
//                     .expect("request builds"),
//             )
//             .await
//             .expect("request succeeds");

//         assert_eq!(
//             response.status(),
//             axum::http::StatusCode::SERVICE_UNAVAILABLE
//         );
//     }
// }
