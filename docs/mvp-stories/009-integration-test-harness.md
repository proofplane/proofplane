# 009 - Integration Test Harness

## Goal

Create a dedicated integration test target that can spin up required dependencies with testcontainers.

## Design

The integration suite lives at `tests/integration/main.rs` and is registered in `Cargo.toml` with `[[test]]`. It should expose reusable harness helpers for:

- starting Postgres
- starting Pub/Sub emulator
- creating temporary filesystem object storage roots
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
- Worker smoke test starts worker and verifies it can connect to Pub/Sub.

## QA Guide

1. Ensure Docker is running.
2. Run only the integration test target.
3. Confirm containers are cleaned up after tests.
4. Intentionally break a dependency endpoint and confirm the harness reports a useful failure.
