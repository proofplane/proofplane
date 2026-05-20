# 009 - Integration Test Harness

## Goal

Create the dedicated integration test target and harness when feature work needs process or infrastructure integration coverage.

## Design

This story is intentionally deferred until there is product behavior worth validating across a real process or external dependency boundary. No Postgres/testcontainers coverage exists before this story; this story owns adding it.

Add an integration suite at `tests/integration/main.rs` and register it in `Cargo.toml` with `[[test]]`. It should expose reusable harness helpers for:

- starting Postgres
- starting the Pub/Sub emulator when a Pub/Sub story needs it
- creating temporary filesystem object storage roots when object storage exists
- generating temporary YAML config
- running migrations
- running seeds
- starting API, worker, and MCP binaries when needed

Tests should exercise public process and network boundaries whenever possible.

## Acceptance Criteria

- Integration test target runs independently from unit tests.
- Testcontainers starts all required dependencies without relying on docker compose.
- Harness waits for readiness deterministically.
- Harness creates isolated databases, topics, subscriptions, buckets, and temp config per test or test suite.
- Failing tests print enough container and application logs to diagnose.

## Tests

- Harness self-test starts all dependencies and verifies health.
- Harness self-test runs migrations and seed data.
- API smoke test starts API and calls `/readyz`.
- Worker smoke test starts worker and verifies it can connect to Pub/Sub once the worker and Pub/Sub runtime exist.

## QA Guide

1. Ensure Docker is running.
2. Run only the integration test target.
3. Confirm containers are cleaned up after tests.
4. Intentionally break a dependency endpoint and confirm the harness reports a useful failure.
