# 013 - Worker Runtime and Subscription Handling

## Goal

Build the broader worker binary runtime with a worker pool and Pub/Sub subscription consumption. Story 012 owns the transactional outbox publish dequeuer.

## Design

The worker runs multiple concurrent loops:

- Pub/Sub subscription puller
- channel-based dispatch to worker tasks
- `tokio::task::JoinSet` worker pool
- liveness/readiness/metrics HTTP server

Use bounded channels to provide backpressure. Worker tasks receive messages pulled from Pub/Sub and call service-layer handlers.

## Acceptance Criteria

- Worker binary starts from YAML config.
- Worker pool size is configurable.
- Pub/Sub messages flow through puller -> channel -> handler -> ack/nack.
- Shutdown drains in-flight work within a configurable grace period.
- Worker exposes liveness, readiness, and metrics.

## Tests

- Unit tests cover worker pool dispatch with fake messages.
- Unit tests cover graceful shutdown behavior.
- Integration tests publish a message and verify the worker handles and acks it.
- Metrics tests verify worker counters and histograms are exposed.

## QA Guide

1. Start local dependencies.
2. Run worker with local config.
3. Publish a test inbound message and confirm it is processed and acked.
