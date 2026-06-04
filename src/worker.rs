use std::{collections::BTreeMap, sync::Arc, time::Duration};

use async_trait::async_trait;
use axum::{
    body::Bytes,
    extract::{MatchedPath, State},
    http::{Request, StatusCode},
    middleware,
    response::Response,
    routing::post,
    Router,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use metrics_exporter_prometheus::PrometheusHandle;
use serde::Deserialize;
use serde_json::Value;
use thiserror::Error;
use tower_http::trace::TraceLayer;
use tracing::Span;

use crate::{
    repository::Postgres,
    routes::{
        error::not_found,
        health::{self, ReadyState},
        metrics::{self, MetricsState},
        request_context::attach_request_id,
    },
};

pub const ATTACHMENT_SCAN_REQUESTED: &str = "attachment.scan_requested";

#[derive(Debug, Clone, PartialEq)]
pub struct WorkerMessage {
    pub message_id: String,
    pub event_type: String,
    pub aggregate_type: String,
    pub aggregate_id: String,
    pub payload: Value,
    pub attributes: BTreeMap<String, String>,
    pub delivery_attempt: Option<u32>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum WorkerMessageDecodeError {
    #[error("push envelope must be valid JSON")]
    EnvelopeJson,
    #[error("push envelope is missing message")]
    MissingMessage,
    #[error("message data must be valid base64")]
    InvalidBase64,
    #[error("message data must be a JSON payload")]
    PayloadJson,
    #[error("message attributes must include {0}")]
    MissingAttribute(&'static str),
}

#[derive(Debug, Error)]
#[error("retryable handler failure: {0}")]
pub struct RetryableWorkerError(pub String);

#[async_trait]
pub trait WorkerHandler: Clone + Send + Sync + 'static {
    async fn handle_scan_requested(
        &self,
        message: WorkerMessage,
    ) -> Result<(), RetryableWorkerError>;
}

#[derive(Clone, Default)]
pub struct LogOnlyWorkerHandler;

#[async_trait]
impl WorkerHandler for LogOnlyWorkerHandler {
    async fn handle_scan_requested(
        &self,
        message: WorkerMessage,
    ) -> Result<(), RetryableWorkerError> {
        tracing::info!(
            message_id = %message.message_id,
            event_type = %message.event_type,
            aggregate_type = %message.aggregate_type,
            aggregate_id = %message.aggregate_id,
            delivery_attempt = ?message.delivery_attempt,
            "worker message accepted"
        );
        Ok(())
    }
}

#[derive(Clone)]
pub struct WorkerRouteState<H> {
    handler: H,
}

impl<H> WorkerRouteState<H> {
    pub fn new(handler: H) -> Self {
        Self { handler }
    }
}

pub struct WorkerAppDependencies<H> {
    pub postgres: Arc<Postgres>,
    pub metrics: PrometheusHandle,
    pub live_path: String,
    pub ready_path: String,
    pub dependency_timeout_ms: u64,
    pub handler: H,
}

pub fn create_worker_app<H>(dependencies: WorkerAppDependencies<H>) -> Router
where
    H: WorkerHandler,
{
    Router::new()
        .nest(&dependencies.live_path, health::livez_router())
        .nest(
            &dependencies.ready_path,
            health::readyz_router(ReadyState {
                postgres: dependencies.postgres,
                dependency_timeout_ms: dependencies.dependency_timeout_ms,
            }),
        )
        .nest(
            "/metrics",
            metrics::router(MetricsState {
                handle: dependencies.metrics,
            }),
        )
        .merge(router(WorkerRouteState::new(dependencies.handler)))
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
                        request_id = tracing::field::Empty
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
        )
}

pub fn router<H>(state: WorkerRouteState<H>) -> Router
where
    H: WorkerHandler,
{
    Router::new()
        .route("/pubsub/messages", post(pubsub_message::<H>))
        .with_state(state)
}

async fn pubsub_message<H>(State(state): State<WorkerRouteState<H>>, body: Bytes) -> StatusCode
where
    H: WorkerHandler,
{
    let message = match decode_worker_message(&body) {
        Ok(message) => message,
        Err(error) => {
            tracing::warn!(%error, "acknowledging malformed Pub/Sub push message");
            return StatusCode::NO_CONTENT;
        }
    };

    dispatch(&state.handler, message).await
}

pub async fn dispatch<H>(handler: &H, message: WorkerMessage) -> StatusCode
where
    H: WorkerHandler,
{
    match message.event_type.as_str() {
        ATTACHMENT_SCAN_REQUESTED => match handler.handle_scan_requested(message).await {
            Ok(()) => StatusCode::NO_CONTENT,
            Err(error) => {
                tracing::error!(%error, "retryable worker handler failure");
                StatusCode::INTERNAL_SERVER_ERROR
            }
        },
        event_type => {
            tracing::warn!(%event_type, "acknowledging unknown worker event type");
            StatusCode::NO_CONTENT
        }
    }
}

pub fn decode_worker_message(body: &[u8]) -> Result<WorkerMessage, WorkerMessageDecodeError> {
    let envelope: PushEnvelope =
        serde_json::from_slice(body).map_err(|_| WorkerMessageDecodeError::EnvelopeJson)?;
    let message = envelope
        .message
        .ok_or(WorkerMessageDecodeError::MissingMessage)?;
    let data = STANDARD
        .decode(message.data.as_bytes())
        .map_err(|_| WorkerMessageDecodeError::InvalidBase64)?;
    let payload =
        serde_json::from_slice(&data).map_err(|_| WorkerMessageDecodeError::PayloadJson)?;
    let attributes = message.attributes;
    let event_type = required_attribute(&attributes, "event_type")?;
    let aggregate_type = required_attribute(&attributes, "aggregate_type")?;
    let aggregate_id = required_attribute(&attributes, "aggregate_id")?;

    Ok(WorkerMessage {
        message_id: message.message_id,
        event_type,
        aggregate_type,
        aggregate_id,
        payload,
        attributes,
        delivery_attempt: envelope.delivery_attempt,
    })
}

fn required_attribute(
    attributes: &BTreeMap<String, String>,
    key: &'static str,
) -> Result<String, WorkerMessageDecodeError> {
    attributes
        .get(key)
        .filter(|value| !value.trim().is_empty())
        .cloned()
        .ok_or(WorkerMessageDecodeError::MissingAttribute(key))
}

#[derive(Debug, Deserialize)]
struct PushEnvelope {
    message: Option<PushEnvelopeMessage>,
    #[serde(rename = "deliveryAttempt")]
    delivery_attempt: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct PushEnvelopeMessage {
    #[serde(rename = "messageId")]
    message_id: String,
    data: String,
    #[serde(default)]
    attributes: BTreeMap<String, String>,
}

#[cfg(test)]
mod tests {
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };

    use axum_test::TestServer;
    use serde_json::json;

    use super::*;

    #[derive(Clone, Default)]
    struct TestHandler {
        fail: Arc<AtomicBool>,
    }

    #[async_trait]
    impl WorkerHandler for TestHandler {
        async fn handle_scan_requested(
            &self,
            _message: WorkerMessage,
        ) -> Result<(), RetryableWorkerError> {
            if self.fail.load(Ordering::SeqCst) {
                return Err(RetryableWorkerError("injected failure".to_owned()));
            }

            Ok(())
        }
    }

    #[test]
    fn decodes_valid_push_envelope_into_worker_message() {
        let message = decode_worker_message(&valid_envelope("attachment.scan_requested"))
            .expect("worker message decodes");

        assert_eq!(message.message_id, "message-1");
        assert_eq!(message.event_type, "attachment.scan_requested");
        assert_eq!(message.aggregate_type, "evidence_attachment");
        assert_eq!(message.aggregate_id, "attachment-1");
        assert_eq!(message.payload, json!({ "scan_id": "scan-1" }));
        assert_eq!(
            message.attributes["event_type"],
            "attachment.scan_requested"
        );
        assert_eq!(message.delivery_attempt, Some(2));
    }

    #[test]
    fn rejects_invalid_base64_payload() {
        assert_eq!(
            decode_worker_message(br#"{"message":{"messageId":"message-1","data":"%%","attributes":{"event_type":"attachment.scan_requested","aggregate_type":"evidence_attachment","aggregate_id":"attachment-1"}}}"#)
                .expect_err("base64 is invalid"),
            WorkerMessageDecodeError::InvalidBase64
        );
    }

    #[test]
    fn rejects_non_json_payload() {
        let envelope = json!({
            "message": {
                "messageId": "message-1",
                "data": STANDARD.encode(b"not-json"),
                "attributes": routing_attributes("attachment.scan_requested")
            }
        });

        assert_eq!(
            decode_worker_message(envelope.to_string().as_bytes()).expect_err("payload is invalid"),
            WorkerMessageDecodeError::PayloadJson
        );
    }

    #[test]
    fn rejects_missing_message() {
        assert_eq!(
            decode_worker_message(br#"{"deliveryAttempt":1}"#).expect_err("message is missing"),
            WorkerMessageDecodeError::MissingMessage
        );
    }

    #[test]
    fn rejects_missing_routing_attributes() {
        let envelope = json!({
            "message": {
                "messageId": "message-1",
                "data": STANDARD.encode(br#"{"scan_id":"scan-1"}"#),
                "attributes": {}
            }
        });

        assert_eq!(
            decode_worker_message(envelope.to_string().as_bytes())
                .expect_err("routing metadata is missing"),
            WorkerMessageDecodeError::MissingAttribute("event_type")
        );
    }

    #[tokio::test]
    async fn dispatches_known_event_successfully() {
        let status = dispatch(
            &TestHandler::default(),
            decode_worker_message(&valid_envelope("attachment.scan_requested")).unwrap(),
        )
        .await;

        assert_eq!(status, StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn dispatches_unknown_event_as_non_retryable_success() {
        let status = dispatch(
            &TestHandler::default(),
            decode_worker_message(&valid_envelope("unknown.event")).unwrap(),
        )
        .await;

        assert_eq!(status, StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn dispatches_retryable_handler_failure_as_server_error() {
        let handler = TestHandler::default();
        handler.fail.store(true, Ordering::SeqCst);

        let status = dispatch(
            &handler,
            decode_worker_message(&valid_envelope("attachment.scan_requested")).unwrap(),
        )
        .await;

        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn worker_route_acknowledges_valid_known_event() {
        let server = TestServer::new(router(WorkerRouteState::new(TestHandler::default())));

        let response = server
            .post("/pubsub/messages")
            .json(
                &serde_json::from_slice::<Value>(&valid_envelope("attachment.scan_requested"))
                    .unwrap(),
            )
            .await;

        response.assert_status(StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn worker_route_acknowledges_malformed_and_unknown_events() {
        let server = TestServer::new(router(WorkerRouteState::new(TestHandler::default())));

        server
            .post("/pubsub/messages")
            .bytes(Bytes::from_static(b"not-json"))
            .await
            .assert_status(StatusCode::NO_CONTENT);

        server
            .post("/pubsub/messages")
            .json(&serde_json::from_slice::<Value>(&valid_envelope("unknown.event")).unwrap())
            .await
            .assert_status(StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn worker_route_returns_non_success_for_retryable_handler_error() {
        let handler = TestHandler::default();
        handler.fail.store(true, Ordering::SeqCst);
        let server = TestServer::new(router(WorkerRouteState::new(handler)));

        let response = server
            .post("/pubsub/messages")
            .json(
                &serde_json::from_slice::<Value>(&valid_envelope("attachment.scan_requested"))
                    .unwrap(),
            )
            .await;

        response.assert_status(StatusCode::INTERNAL_SERVER_ERROR);
    }

    fn valid_envelope(event_type: &str) -> Vec<u8> {
        json!({
            "message": {
                "messageId": "message-1",
                "data": STANDARD.encode(br#"{"scan_id":"scan-1"}"#),
                "attributes": routing_attributes(event_type)
            },
            "deliveryAttempt": 2
        })
        .to_string()
        .into_bytes()
    }

    fn routing_attributes(event_type: &str) -> BTreeMap<String, String> {
        BTreeMap::from([
            ("event_type".to_owned(), event_type.to_owned()),
            (
                "aggregate_type".to_owned(),
                "evidence_attachment".to_owned(),
            ),
            ("aggregate_id".to_owned(), "attachment-1".to_owned()),
        ])
    }
}
