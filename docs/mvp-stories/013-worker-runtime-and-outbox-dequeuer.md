# 013 - Worker Runtime and Outbox Dequeuer

## Goal

Build the worker binary with a worker pool, Pub/Sub subscription consumption, and the transactional outbox dequeuer.

## Design

The worker runs multiple concurrent loops:

- Pub/Sub subscription puller
- channel-based dispatch to worker tasks
- `tokio::task::JoinSet` worker pool
- outbox dequeuer loop
- liveness/readiness/metrics HTTP server

Use bounded channels to provide backpressure. Worker tasks receive messages pulled from Pub/Sub and call service-layer handlers. The outbox dequeuer claims pending outbox rows and publishes them to Pub/Sub.

## Acceptance Criteria

- Worker binary starts from YAML config.
- Worker pool size is configurable.
- Pub/Sub messages flow through puller -> channel -> handler -> ack/nack.
- Outbox dequeuer publishes pending records and updates statuses.
- Shutdown drains in-flight work within a configurable grace period.
- Worker exposes liveness, readiness, and metrics.

## Tests

- Unit tests cover worker pool dispatch with fake messages.
- Unit tests cover graceful shutdown behavior.
- Integration tests publish a message and verify the worker handles and acks it.
- Integration tests insert outbox rows and verify the dequeuer publishes them.
- Metrics tests verify worker counters and histograms are exposed.

## QA Guide

1. Start local dependencies.
2. Run worker with local config.
3. Insert or create an outbox message.
4. Confirm Pub/Sub receives the event.
5. Publish a test inbound message and confirm it is processed and acked.
