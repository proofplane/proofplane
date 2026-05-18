# 006 - Observability Scaffold

## Goal

Add structured logging from the beginning using `tracing_subscriber`.

## Design

Implement a shared observability module that initializes:

- `tracing_subscriber`
- `RUST_LOG` level filtering
- JSON structured logging by default

This story does not add OpenTelemetry traces, distributed trace propagation, custom spans, Prometheus metrics, metrics endpoints, or placeholder counters. Metrics and request/message-level fields should be added when later stories introduce real HTTP and worker runtime boundaries.

## Acceptance Criteria

- Every binary initializes `tracing_subscriber` once on successful startup.
- Logs are JSON structured and respect `RUST_LOG`.
- Configured default filters are used when `RUST_LOG` is absent.
- Logging avoids secrets and large payload bodies.

## Tests

- Unit tests verify log filter parsing for valid and invalid `RUST_LOG` values.
- Integration tests verify API, worker, MCP, and seed startup logs are structured JSON.
- Integration tests verify `RUST_LOG=off` suppresses startup info logs.
- Integration tests verify missing config still fails with a clear stderr error.

## QA Guide

1. Run the API with `RUST_LOG=debug`.
2. Inspect startup structured JSON logs.
3. Run with `RUST_LOG=off` and confirm startup info logs are suppressed.
4. Confirm no configured secrets appear in logs.
5. Run `make check`.
6. Run `make test-integration`.
