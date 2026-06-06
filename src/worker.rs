use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use axum::{
    body::Bytes,
    extract::{MatchedPath, State},
    http::{Request, StatusCode},
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
use tracing::{Instrument, Span};
use uuid::Uuid;

use crate::{
    handlers::attachment_scan::AttachmentScanHandler,
    object_storage::FilesystemObjectStore,
    repository::Postgres,
    routes::{
        error::not_found,
        health::{self, ReadyState},
        metrics::{self, MetricsState},
    },
    scanner::NoopMalwareScanner,
    validate,
    validation::Validation,
};

pub const ATTACHMENT_SCAN_REQUESTED: &str = "attachment.scan_requested";

#[derive(Debug, Clone, PartialEq)]
pub struct WorkerMessage {
    pub message_id: String,
    pub event_type: String,
    pub aggregate_type: String,
    pub aggregate_id: String,
    pub request_id: Option<Uuid>,
    pub payload: Value,
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
    #[error("message data envelope is invalid")]
    DataValidation(Vec<WorkerMessageDataValidationError>),
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum WorkerMessageDataValidationError {
    #[error("message data envelope must include {0}")]
    MissingField(&'static str),
    #[error("message data request_id must be a UUID")]
    InvalidRequestId,
}

#[derive(Debug, Error)]
#[error("retryable handler failure: {0}")]
pub struct RetryableWorkerError(pub String);

pub struct WorkerAppDependencies {
    pub postgres: Arc<Postgres>,
    pub object_store: Arc<FilesystemObjectStore>,
    pub scanner: Arc<NoopMalwareScanner>,
    pub worker_max_delivery_attempts: u16,
    pub metrics: PrometheusHandle,
    pub live_path: String,
    pub ready_path: String,
    pub dependency_timeout_ms: u64,
}

#[async_trait]
trait AttachmentScanWorkerHandler: Send + Sync {
    async fn handle_scan_requested(
        &self,
        message: WorkerMessage,
    ) -> Result<(), RetryableWorkerError>;

    async fn handle_scan_requested_final_delivery(
        &self,
        message: WorkerMessage,
    ) -> Result<(), RetryableWorkerError>;
}

#[async_trait]
impl AttachmentScanWorkerHandler
    for AttachmentScanHandler<Postgres, FilesystemObjectStore, NoopMalwareScanner>
{
    async fn handle_scan_requested(
        &self,
        message: WorkerMessage,
    ) -> Result<(), RetryableWorkerError> {
        AttachmentScanHandler::handle_scan_requested(self, message).await
    }

    async fn handle_scan_requested_final_delivery(
        &self,
        message: WorkerMessage,
    ) -> Result<(), RetryableWorkerError> {
        AttachmentScanHandler::handle_scan_requested_final_delivery(self, message).await
    }
}

#[derive(Clone)]
pub struct WorkerState {
    attachment_scan_handler: Arc<dyn AttachmentScanWorkerHandler>,
    worker_max_delivery_attempts: u16,
}

impl WorkerState {
    pub fn new(
        postgres: Arc<Postgres>,
        object_store: Arc<FilesystemObjectStore>,
        scanner: Arc<NoopMalwareScanner>,
        worker_max_delivery_attempts: u16,
    ) -> Self {
        Self {
            attachment_scan_handler: Arc::new(AttachmentScanHandler::new(
                postgres,
                object_store,
                scanner,
            )),
            worker_max_delivery_attempts,
        }
    }

    #[cfg(test)]
    fn test(handler: Arc<dyn AttachmentScanWorkerHandler>) -> Self {
        Self {
            attachment_scan_handler: handler,
            worker_max_delivery_attempts: 5,
        }
    }
}

pub fn create_worker_app(dependencies: WorkerAppDependencies) -> Router {
    let state = WorkerState::new(
        dependencies.postgres.clone(),
        dependencies.object_store,
        dependencies.scanner,
        dependencies.worker_max_delivery_attempts,
    );

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
        .merge(router(state))
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

                    tracing::info_span!(
                        "http_request",
                        %method,
                        path
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

pub fn router(state: WorkerState) -> Router {
    Router::new()
        .route("/pubsub/messages", post(pubsub_message))
        .with_state(state)
}

async fn pubsub_message(State(state): State<WorkerState>, body: Bytes) -> StatusCode {
    let message = match decode_worker_message(&body) {
        Ok(message) => message,
        Err(error) => {
            tracing::warn!(%error, "acknowledging malformed Pub/Sub push message");
            return StatusCode::NO_CONTENT;
        }
    };

    let span = tracing::info_span!(
        "worker_message",
        request_id = tracing::field::Empty,
        message_id = %message.message_id,
        event_type = %message.event_type,
        aggregate_type = %message.aggregate_type,
        aggregate_id = %message.aggregate_id,
        delivery_attempt = tracing::field::Empty,
    );
    if let Some(request_id) = message.request_id {
        span.record("request_id", request_id.to_string());
    }
    if let Some(delivery_attempt) = message.delivery_attempt {
        span.record("delivery_attempt", delivery_attempt);
    }

    async move {
        tracing::info!("worker message processing started");
        let status = dispatch(state, message).await;
        tracing::info!(
            acknowledgement_status = status.as_u16(),
            "worker message processing completed"
        );
        status
    }
    .instrument(span)
    .await
}

pub async fn dispatch(state: WorkerState, message: WorkerMessage) -> StatusCode {
    match message.event_type.as_str() {
        ATTACHMENT_SCAN_REQUESTED => {
            let is_final_delivery = message
                .delivery_attempt
                .is_some_and(|attempt| attempt >= u32::from(state.worker_max_delivery_attempts));
            let result = if is_final_delivery {
                state
                    .attachment_scan_handler
                    .handle_scan_requested_final_delivery(message)
                    .await
            } else {
                state
                    .attachment_scan_handler
                    .handle_scan_requested(message)
                    .await
            };

            match result {
                Ok(()) => StatusCode::NO_CONTENT,
                Err(error) => {
                    tracing::error!(%error, "retryable worker handler failure");
                    StatusCode::INTERNAL_SERVER_ERROR
                }
            }
        }
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
    let data = serde_json::from_slice::<WorkerMessageDataDTO>(&data)
        .map_err(|_| WorkerMessageDecodeError::PayloadJson)?
        .into_message_data()
        .into_result()
        .map_err(WorkerMessageDecodeError::DataValidation)?;

    Ok(WorkerMessage {
        message_id: message.message_id,
        event_type: data.event_type,
        aggregate_type: data.aggregate_type,
        aggregate_id: data.aggregate_id,
        request_id: data.request_id,
        payload: data.payload,
        delivery_attempt: envelope.delivery_attempt,
    })
}

#[derive(Debug, Deserialize)]
struct WorkerMessageDataDTO {
    event_type: Option<String>,
    aggregate_type: Option<String>,
    aggregate_id: Option<String>,
    #[serde(default)]
    request_id: Option<String>,
    payload: Option<Value>,
}

impl WorkerMessageDataDTO {
    fn into_message_data(self) -> Validation<WorkerMessageData, WorkerMessageDataValidationError> {
        validate! {
            event_type <- required_message_string("event_type", self.event_type),
            aggregate_type <- required_message_string("aggregate_type", self.aggregate_type),
            aggregate_id <- required_message_string("aggregate_id", self.aggregate_id),
            request_id <- optional_request_id(self.request_id),
            payload <- required_message_value("payload", self.payload),
            => WorkerMessageData {
                event_type,
                aggregate_type,
                aggregate_id,
                request_id,
                payload,
            },
        }
    }
}

struct WorkerMessageData {
    event_type: String,
    aggregate_type: String,
    aggregate_id: String,
    request_id: Option<Uuid>,
    payload: Value,
}

fn optional_request_id(
    request_id: Option<String>,
) -> Validation<Option<Uuid>, WorkerMessageDataValidationError> {
    match request_id {
        Some(request_id) => Uuid::parse_str(&request_id)
            .map(Some)
            .map(Validation::valid)
            .unwrap_or_else(|_| {
                Validation::invalid(WorkerMessageDataValidationError::InvalidRequestId)
            }),
        None => Validation::valid(None),
    }
}

fn required_message_string(
    field: &'static str,
    value: Option<String>,
) -> Validation<String, WorkerMessageDataValidationError> {
    match value {
        Some(value) if !value.trim().is_empty() => Validation::valid(value),
        _ => Validation::invalid(WorkerMessageDataValidationError::MissingField(field)),
    }
}

fn required_message_value(
    field: &'static str,
    value: Option<Value>,
) -> Validation<Value, WorkerMessageDataValidationError> {
    value.map(Validation::valid).unwrap_or_else(|| {
        Validation::invalid(WorkerMessageDataValidationError::MissingField(field))
    })
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
}

#[cfg(test)]
mod tests {
    use std::{
        io::{self, Write},
        sync::{
            atomic::{AtomicUsize, Ordering},
            Arc, Mutex,
        },
    };

    use axum_test::TestServer;
    use serde_json::json;

    use super::*;

    #[test]
    fn decodes_valid_push_envelope_into_worker_message() {
        let message = decode_worker_message(&valid_envelope("attachment.scan_requested"))
            .expect("worker message decodes");

        assert_eq!(message.message_id, "message-1");
        assert_eq!(message.event_type, "attachment.scan_requested");
        assert_eq!(message.aggregate_type, "evidence_attachment");
        assert_eq!(message.aggregate_id, "attachment-1");
        assert_eq!(message.payload, json!({ "scan_id": "scan-1" }));
        assert_eq!(message.request_id, Some(request_id()));
        assert_eq!(message.delivery_attempt, Some(2));
    }

    #[test]
    fn ignores_pubsub_attributes_when_decoding_worker_message() {
        let envelope = json!({
            "message": {
                "messageId": "message-1",
                "data": STANDARD.encode(
                    json!({
                        "event_type": "attachment.scan_requested",
                        "aggregate_type": "evidence_attachment",
                        "aggregate_id": "attachment-1",
                        "request_id": request_id(),
                        "payload": { "scan_id": "scan-1" }
                    })
                    .to_string()
                    .as_bytes()
                ),
                "attributes": {
                    "event_type": "wrong.event",
                    "aggregate_type": "wrong_aggregate",
                    "aggregate_id": "wrong-id"
                }
            },
            "deliveryAttempt": 2
        });
        let message =
            decode_worker_message(envelope.to_string().as_bytes()).expect("worker message decodes");

        assert_eq!(message.message_id, "message-1");
        assert_eq!(message.event_type, "attachment.scan_requested");
        assert_eq!(message.aggregate_type, "evidence_attachment");
        assert_eq!(message.aggregate_id, "attachment-1");
        assert_eq!(message.payload, json!({ "scan_id": "scan-1" }));
        assert_eq!(message.request_id, Some(request_id()));
    }

    #[test]
    fn decodes_missing_and_null_request_ids() {
        for request_id_value in [None, Some(Value::Null)] {
            let mut data = json!({
                "event_type": "attachment.scan_requested",
                "aggregate_type": "evidence_attachment",
                "aggregate_id": "attachment-1",
                "payload": { "scan_id": "scan-1" }
            });
            if let Some(request_id_value) = request_id_value {
                data["request_id"] = request_id_value;
            }

            let message = decode_worker_message(&envelope_with_data(data))
                .expect("worker message without request ID decodes");

            assert_eq!(message.request_id, None);
        }
    }

    #[test]
    fn rejects_non_uuid_request_id() {
        let data = json!({
            "event_type": "attachment.scan_requested",
            "aggregate_type": "evidence_attachment",
            "aggregate_id": "attachment-1",
            "request_id": "not-a-uuid",
            "payload": { "scan_id": "scan-1" }
        });

        assert_eq!(
            decode_worker_message(&envelope_with_data(data))
                .expect_err("invalid request ID is rejected"),
            WorkerMessageDecodeError::DataValidation(vec![
                WorkerMessageDataValidationError::InvalidRequestId,
            ])
        );
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
    fn rejects_missing_routing_metadata() {
        let envelope = json!({
            "message": {
                "messageId": "message-1",
                "data": STANDARD.encode(br#"{"scan_id":"scan-1"}"#),
            }
        });

        assert_eq!(
            decode_worker_message(envelope.to_string().as_bytes())
                .expect_err("routing metadata is missing"),
            WorkerMessageDecodeError::DataValidation(vec![
                WorkerMessageDataValidationError::MissingField("event_type"),
                WorkerMessageDataValidationError::MissingField("aggregate_type"),
                WorkerMessageDataValidationError::MissingField("aggregate_id"),
                WorkerMessageDataValidationError::MissingField("payload"),
            ])
        );
    }

    #[tokio::test]
    async fn dispatches_known_event_successfully() {
        let status = dispatch(
            WorkerState::test(Arc::new(FakeAttachmentScanHandler::ok())),
            decode_worker_message(&valid_envelope("attachment.scan_requested")).unwrap(),
        )
        .await;

        assert_eq!(status, StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn dispatches_unknown_event_as_non_retryable_success() {
        let status = dispatch(
            WorkerState::test(Arc::new(FakeAttachmentScanHandler::ok())),
            decode_worker_message(&valid_envelope("unknown.event")).unwrap(),
        )
        .await;

        assert_eq!(status, StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn worker_route_acknowledges_valid_known_event() {
        let server = TestServer::new(router(WorkerState::test(Arc::new(
            FakeAttachmentScanHandler::ok(),
        ))));

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
        let server = TestServer::new(router(WorkerState::test(Arc::new(
            FakeAttachmentScanHandler::ok(),
        ))));

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
    async fn successful_processing_logs_message_context() {
        let logs = Arc::new(Mutex::new(Vec::new()));
        let subscriber = tracing_subscriber::fmt()
            .json()
            .with_writer(LogWriterFactory(logs.clone()))
            .finish();
        let _subscriber_guard = tracing::subscriber::set_default(subscriber);
        let handler = Arc::new(FakeAttachmentScanHandler::ok());
        let server = TestServer::new(router(WorkerState::test(handler.clone())));

        let response = server
            .post("/pubsub/messages")
            .json(
                &serde_json::from_slice::<Value>(&valid_envelope("attachment.scan_requested"))
                    .unwrap(),
            )
            .await;

        response.assert_status(StatusCode::NO_CONTENT);
        assert_eq!(handler.calls.load(Ordering::Relaxed), 1);

        let logs = captured_logs(&logs);
        for expected in [
            "worker message processing started",
            "worker handler called",
            "worker message processing completed",
            &request_id().to_string(),
            "message-1",
            "attachment.scan_requested",
            "evidence_attachment",
            "attachment-1",
            "\"delivery_attempt\":2",
            "\"acknowledgement_status\":204",
        ] {
            assert!(
                logs.contains(expected),
                "missing {expected:?} in logs: {logs}"
            );
        }
    }

    #[tokio::test]
    async fn missing_request_id_is_not_generated_and_inbound_header_is_ignored() {
        let logs = Arc::new(Mutex::new(Vec::new()));
        let subscriber = tracing_subscriber::fmt()
            .json()
            .with_writer(LogWriterFactory(logs.clone()))
            .finish();
        let _subscriber_guard = tracing::subscriber::set_default(subscriber);
        let server = TestServer::new(router(WorkerState::test(Arc::new(
            FakeAttachmentScanHandler::ok(),
        ))));
        let inbound_request_id = Uuid::from_u128(2).to_string();
        let data = json!({
            "event_type": "attachment.scan_requested",
            "aggregate_type": "evidence_attachment",
            "aggregate_id": "attachment-1",
            "payload": { "scan_id": "scan-1" }
        });
        let envelope = serde_json::from_slice::<Value>(&envelope_with_data(data)).unwrap();

        let response = server
            .post("/pubsub/messages")
            .add_header("x-request-id", &inbound_request_id)
            .json(&envelope)
            .await;

        response.assert_status(StatusCode::NO_CONTENT);
        assert!(response.maybe_header("x-request-id").is_none());

        let logs = captured_logs(&logs);
        assert!(!logs.contains("request_id"), "captured logs: {logs}");
        assert!(!logs.contains(&inbound_request_id), "captured logs: {logs}");
    }

    #[tokio::test]
    async fn invalid_embedded_request_id_is_acknowledged_without_dispatch() {
        let handler = Arc::new(FakeAttachmentScanHandler::ok());
        let server = TestServer::new(router(WorkerState::test(handler.clone())));
        let data = json!({
            "event_type": "attachment.scan_requested",
            "aggregate_type": "evidence_attachment",
            "aggregate_id": "attachment-1",
            "request_id": "not-a-uuid",
            "payload": { "scan_id": "scan-1" }
        });

        server
            .post("/pubsub/messages")
            .json(&serde_json::from_slice::<Value>(&envelope_with_data(data)).unwrap())
            .await
            .assert_status(StatusCode::NO_CONTENT);

        assert_eq!(handler.calls.load(Ordering::Relaxed), 0);
    }

    fn valid_envelope(event_type: &str) -> Vec<u8> {
        envelope_with_data(json!({
            "event_type": event_type,
            "aggregate_type": "evidence_attachment",
            "aggregate_id": "attachment-1",
            "request_id": request_id(),
            "payload": { "scan_id": "scan-1" }
        }))
    }

    fn envelope_with_data(data: Value) -> Vec<u8> {
        json!({
            "message": {
                "messageId": "message-1",
                "data": STANDARD.encode(data.to_string().as_bytes())
            },
            "deliveryAttempt": 2
        })
        .to_string()
        .into_bytes()
    }

    fn request_id() -> Uuid {
        Uuid::from_u128(1)
    }

    fn captured_logs(logs: &Arc<Mutex<Vec<u8>>>) -> String {
        String::from_utf8(logs.lock().expect("log buffer locks").clone())
            .expect("captured logs are UTF-8")
    }

    #[derive(Clone)]
    struct LogWriterFactory(Arc<Mutex<Vec<u8>>>);

    impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for LogWriterFactory {
        type Writer = LogWriter;

        fn make_writer(&'a self) -> Self::Writer {
            LogWriter(self.0.clone())
        }
    }

    struct LogWriter(Arc<Mutex<Vec<u8>>>);

    impl Write for LogWriter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0
                .lock()
                .expect("log buffer locks")
                .extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    struct FakeAttachmentScanHandler {
        result: Result<(), RetryableWorkerError>,
        calls: AtomicUsize,
    }

    impl FakeAttachmentScanHandler {
        fn ok() -> Self {
            Self {
                result: Ok(()),
                calls: AtomicUsize::new(0),
            }
        }
    }

    #[async_trait]
    impl AttachmentScanWorkerHandler for FakeAttachmentScanHandler {
        async fn handle_scan_requested(
            &self,
            _message: WorkerMessage,
        ) -> Result<(), RetryableWorkerError> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            tracing::info!("worker handler called");
            self.result
                .as_ref()
                .map(|_| ())
                .map_err(|error| RetryableWorkerError(error.0.clone()))
        }

        async fn handle_scan_requested_final_delivery(
            &self,
            _message: WorkerMessage,
        ) -> Result<(), RetryableWorkerError> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            self.result
                .as_ref()
                .map(|_| ())
                .map_err(|error| RetryableWorkerError(error.0.clone()))
        }
    }
}
