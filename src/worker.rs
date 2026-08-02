use std::{sync::Arc, time::Duration};

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
    handlers::{
        document_finalization::DocumentFinalizationHandler, document_scan::DocumentScanHandler,
    },
    messaging::{
        IntegrationMessage, IntegrationMessageDecodeError, LEGACY_DOCUMENT_FINALIZATION_REQUESTED,
        LEGACY_DOCUMENT_SCAN_REQUESTED,
    },
    object_storage::FilesystemObjectStore,
    repository::Postgres,
    routes::{
        error::not_found,
        health::{self, ReadyState},
        metrics::{self, MetricsState},
    },
    scanner::ClamAvMalwareScanner,
    validate,
    validation::Validation,
};

pub const DOCUMENT_SCAN_REQUESTED: &str = LEGACY_DOCUMENT_SCAN_REQUESTED;
pub const DOCUMENT_FINALIZATION_REQUESTED: &str = LEGACY_DOCUMENT_FINALIZATION_REQUESTED;

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
    #[error("typed integration message is invalid: {0}")]
    TypedMessage(#[from] IntegrationMessageDecodeError),
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
/// Returning this error produces an HTTP 500 so Pub/Sub retries the delivery.
pub struct RetryableWorkerError(pub String);

pub struct WorkerAppDependencies {
    pub postgres: Arc<Postgres>,
    pub object_store: Arc<FilesystemObjectStore>,
    pub scanner: Arc<ClamAvMalwareScanner>,
    pub worker_max_delivery_attempts: u16,
    pub metrics: PrometheusHandle,
    pub live_path: String,
    pub ready_path: String,
    pub dependency_timeout_ms: u64,
}

#[derive(Clone)]
pub struct WorkerState {
    document_scan_handler: DocumentScanHandler,
    document_finalization_handler: DocumentFinalizationHandler<FilesystemObjectStore>,
}

impl WorkerState {
    pub fn new(
        postgres: Arc<Postgres>,
        object_store: Arc<FilesystemObjectStore>,
        scanner: Arc<ClamAvMalwareScanner>,
        worker_max_delivery_attempts: u16,
    ) -> Self {
        Self {
            document_scan_handler: DocumentScanHandler::new(
                postgres.clone(),
                object_store.clone(),
                scanner,
                worker_max_delivery_attempts,
            ),
            document_finalization_handler: DocumentFinalizationHandler::new(postgres, object_store),
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
        DOCUMENT_SCAN_REQUESTED => {
            let result = state
                .document_scan_handler
                .handle_scan_requested(message)
                .await;

            match result {
                Ok(()) => StatusCode::NO_CONTENT,
                Err(error) => {
                    tracing::error!(%error, "retryable worker handler failure");
                    StatusCode::INTERNAL_SERVER_ERROR
                }
            }
        }
        DOCUMENT_FINALIZATION_REQUESTED => {
            let result = state
                .document_finalization_handler
                .handle_finalization_requested(message)
                .await;

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
    let data: Value =
        serde_json::from_slice(&data).map_err(|_| WorkerMessageDecodeError::PayloadJson)?;

    if is_typed_envelope(&data) {
        decode_typed_worker_message(data, envelope.delivery_attempt)
    } else {
        decode_legacy_worker_message(data, message.message_id, envelope.delivery_attempt)
    }
}

fn is_typed_envelope(data: &Value) -> bool {
    data.as_object().is_some_and(|object| {
        [
            "message_id",
            "kind",
            "type",
            "version",
            "subject",
            "correlation_id",
            "causation_id",
        ]
        .iter()
        .any(|field| object.contains_key(*field))
    })
}

fn decode_typed_worker_message(
    data: Value,
    delivery_attempt: Option<u32>,
) -> Result<WorkerMessage, WorkerMessageDecodeError> {
    let integration_message = IntegrationMessage::from_envelope(data)?;
    let metadata = integration_message.metadata();
    let event_type = match &integration_message {
        IntegrationMessage::ScanDocument { .. } => DOCUMENT_SCAN_REQUESTED,
        IntegrationMessage::FinalizeDocument { .. } => DOCUMENT_FINALIZATION_REQUESTED,
    };
    let payload = serde_json::to_value(integration_message.payload())
        .map_err(|_| WorkerMessageDecodeError::PayloadJson)?;

    Ok(WorkerMessage {
        message_id: metadata.message_id.to_string(),
        event_type: event_type.to_owned(),
        aggregate_type: integration_message.payload().aggregate_type().to_owned(),
        aggregate_id: metadata.subject.clone(),
        request_id: metadata.correlation_id,
        payload,
        delivery_attempt,
    })
}

fn decode_legacy_worker_message(
    data: Value,
    message_id: String,
    delivery_attempt: Option<u32>,
) -> Result<WorkerMessage, WorkerMessageDecodeError> {
    let data = serde_json::from_value::<WorkerMessageDataDTO>(data)
        .map_err(|_| WorkerMessageDecodeError::PayloadJson)?
        .into_message_data()
        .into_result()
        .map_err(WorkerMessageDecodeError::DataValidation)?;

    Ok(WorkerMessage {
        message_id,
        event_type: data.event_type,
        aggregate_type: data.aggregate_type,
        aggregate_id: data.aggregate_id,
        request_id: data.request_id,
        payload: data.payload,
        delivery_attempt,
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
    use serde_json::json;

    use super::*;
    use crate::messaging::{FINALIZE_DOCUMENT_TYPE, SCAN_DOCUMENT_TYPE};

    #[test]
    fn decodes_valid_push_envelope_into_worker_message() {
        let message = decode_worker_message(&valid_envelope("document.scan_requested"))
            .expect("worker message decodes");

        assert_eq!(message.message_id, "message-1");
        assert_eq!(message.event_type, "document.scan_requested");
        assert_eq!(message.aggregate_type, "evidence_document");
        assert_eq!(message.aggregate_id, "document-1");
        assert_eq!(message.payload, json!({ "scan_id": "scan-1" }));
        assert_eq!(message.request_id, Some(request_id()));
        assert_eq!(message.delivery_attempt, Some(2));
    }

    #[test]
    fn decodes_typed_envelope_and_propagates_integration_metadata() {
        let message = decode_worker_message(&envelope_with_data(json!({
            "message_id": Uuid::from_u128(2),
            "kind": "command",
            "type": SCAN_DOCUMENT_TYPE,
            "version": 1,
            "subject": Uuid::from_u128(3),
            "correlation_id": Uuid::from_u128(4),
            "causation_id": Uuid::from_u128(5),
            "payload": {
                "evidence_submission_id": Uuid::from_u128(6),
                "object_key": "quarantine/document-1"
            }
        })))
        .expect("typed worker message decodes");

        assert_eq!(message.message_id, Uuid::from_u128(2).to_string());
        assert_eq!(message.event_type, DOCUMENT_SCAN_REQUESTED);
        assert_eq!(message.aggregate_type, "evidence_document");
        assert_eq!(message.aggregate_id, Uuid::from_u128(3).to_string());
        assert_eq!(message.request_id, Some(Uuid::from_u128(4)));
        assert_eq!(
            message.payload,
            json!({
                "evidence_submission_id": Uuid::from_u128(6),
                "object_key": "quarantine/document-1"
            })
        );
        assert_eq!(message.delivery_attempt, Some(2));
    }

    #[test]
    fn typed_looking_messages_never_fall_back_to_legacy_decoding() {
        let mixed = json!({
            "message_id": Uuid::from_u128(2),
            "kind": "command",
            "type": FINALIZE_DOCUMENT_TYPE,
            "version": 2,
            "subject": Uuid::from_u128(3),
            "correlation_id": null,
            "causation_id": null,
            "payload": {
                "evidence_submission_id": Uuid::from_u128(6),
                "object_key": "quarantine/document-1"
            },
            "event_type": DOCUMENT_FINALIZATION_REQUESTED,
            "aggregate_type": "evidence_document",
            "aggregate_id": Uuid::from_u128(3)
        });

        assert_eq!(
            decode_worker_message(&envelope_with_data(mixed))
                .expect_err("mixed envelope is rejected as typed"),
            WorkerMessageDecodeError::TypedMessage(
                IntegrationMessageDecodeError::MalformedEnvelope
            )
        );
    }

    #[test]
    fn rejects_unknown_typed_types_versions_and_malformed_payloads() {
        let base = json!({
            "message_id": Uuid::from_u128(2),
            "kind": "command",
            "type": SCAN_DOCUMENT_TYPE,
            "version": 1,
            "subject": Uuid::from_u128(3),
            "correlation_id": null,
            "causation_id": null,
            "payload": {
                "evidence_submission_id": Uuid::from_u128(6),
                "object_key": "quarantine/document-1"
            }
        });

        let mut unknown_type = base.clone();
        unknown_type["type"] = json!("UnknownCommand");
        assert!(matches!(
            decode_worker_message(&envelope_with_data(unknown_type)),
            Err(WorkerMessageDecodeError::TypedMessage(
                IntegrationMessageDecodeError::UnknownType(_)
            ))
        ));

        let mut unknown_version = base.clone();
        unknown_version["version"] = json!(2);
        assert!(matches!(
            decode_worker_message(&envelope_with_data(unknown_version)),
            Err(WorkerMessageDecodeError::TypedMessage(
                IntegrationMessageDecodeError::UnsupportedVersion { .. }
            ))
        ));

        let mut malformed_payload = base;
        malformed_payload["payload"] = json!({ "object_key": 7 });
        assert_eq!(
            decode_worker_message(&envelope_with_data(malformed_payload))
                .expect_err("malformed typed payload is rejected"),
            WorkerMessageDecodeError::TypedMessage(IntegrationMessageDecodeError::MalformedPayload)
        );
    }

    #[test]
    fn ignores_pubsub_attributes_when_decoding_worker_message() {
        let envelope = json!({
            "message": {
                "messageId": "message-1",
                "data": STANDARD.encode(
                    json!({
                        "event_type": "document.scan_requested",
                        "aggregate_type": "evidence_document",
                        "aggregate_id": "document-1",
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
        assert_eq!(message.event_type, "document.scan_requested");
        assert_eq!(message.aggregate_type, "evidence_document");
        assert_eq!(message.aggregate_id, "document-1");
        assert_eq!(message.payload, json!({ "scan_id": "scan-1" }));
        assert_eq!(message.request_id, Some(request_id()));
    }

    #[test]
    fn decodes_missing_and_null_request_ids() {
        for request_id_value in [None, Some(Value::Null)] {
            let mut data = json!({
                "event_type": "document.scan_requested",
                "aggregate_type": "evidence_document",
                "aggregate_id": "document-1",
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
            "event_type": "document.scan_requested",
            "aggregate_type": "evidence_document",
            "aggregate_id": "document-1",
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
            decode_worker_message(br#"{"message":{"messageId":"message-1","data":"%%","attributes":{"event_type":"document.scan_requested","aggregate_type":"evidence_document","aggregate_id":"document-1"}}}"#)
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

    fn valid_envelope(event_type: &str) -> Vec<u8> {
        envelope_with_data(json!({
            "event_type": event_type,
            "aggregate_type": "evidence_document",
            "aggregate_id": "document-1",
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
}
