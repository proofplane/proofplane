# 006 - Observability Scaffold

## Goal

Add production-shaped observability from the beginning.

## Design

Implement a shared observability crate that initializes:

- `tracing_subscriber`
- `RUST_LOG` level filtering
- JSON structured logging by default
- request IDs and trace IDs where available
- Prometheus metrics registry and HTTP export

Metrics should start with:

- process health gauges
- HTTP request counters and histograms
- repository query counters and histograms
- Pub/Sub publish, receive, ack, nack, retry, and dead-letter counters
- outbox pending, sent, failed, and retry counters
- worker message processing counters and histograms

## Acceptance Criteria

- Every binary initializes tracing once.
- Logs are JSON structured and respect `RUST_LOG`.
- Prometheus metrics endpoint is available for API, worker, and MCP where applicable.
- Metrics names use a stable `proofplane_` prefix.
- Logging avoids secrets and large payload bodies.

## Tests

- Unit tests verify log filter parsing for valid and invalid `RUST_LOG` values.
- Integration tests call the metrics endpoint and assert Prometheus text output.
- API tests verify request logs include method, path template, status, latency, and request ID.
- Worker tests verify message processing metrics increment.

## QA Guide

1. Run the API with `RUST_LOG=debug`.
2. Make a request and inspect structured JSON logs.
3. Fetch `/metrics` and confirm Prometheus text is returned.
4. Confirm no configured secrets appear in logs.
