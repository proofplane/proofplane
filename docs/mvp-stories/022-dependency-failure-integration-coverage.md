# 022 - Dependency Failure Integration Coverage

## Status

Partially implemented. Attachment scan and finalization handler integration
tests now use concrete Postgres and inject database failures to verify atomic
rollback and retry behavior. They also cover scanner and object-store failures
at those adapter boundaries. API readiness, SpiceDB interruption, Pub/Sub
interruption, and public attachment API storage-failure coverage remain open.

## Goal

Harden Proofplane's runtime boundaries by proving that dependency failures are
reported, contained, and recoverable in integration tests.

This story fills the gap between feature-level API integration coverage and the
final end-to-end release gate. It should focus on realistic dependency behavior,
not narrow unit tests for app internals.

## Design

Extend the integration harness from story 009 with helpers that can start the
application against healthy dependencies, interrupt one dependency at a time, and
assert the externally visible behavior.

Cover the dependencies that can affect MVP runtime availability:

- Postgres for API readiness and request handling
- SpiceDB for Evidence Request authorization
- Pub/Sub emulator once worker and outbox stories exist
- filesystem object storage once evidence attachment storage exists

Prefer process or network boundary checks. For example, exercise `/readyz`,
Evidence Request API calls, worker startup, worker shutdown, and failed adapter
operations through the same public surfaces used in production. Avoid
reintroducing deleted app-unit tests that mock internal state.

### API and Postgres

Add API integration tests that demonstrate:

- `/readyz` returns success when Postgres is reachable
- `/readyz` returns `503` with the stable `not_ready` error shape when Postgres
  is unavailable or the readiness query times out
- in-flight or subsequent API requests fail with stable error responses when
  Postgres disappears
- the app recovers on later requests if the dependency becomes reachable again,
  where the underlying pool and dependency support recovery

The harness may stop a testcontainer, point the app at an unroutable endpoint, or
use a timeout-oriented fixture. Pick the option that is deterministic in CI.

### SpiceDB Authorization

Add API integration tests that demonstrate:

- Evidence Request routes fail closed if SpiceDB permission checks fail due to
  dependency errors
- dependency errors are logged for operators without leaking API keys
- authentication still runs before authorization, so missing or invalid
  credentials return `401` even if the target workspace is ungranted or SpiceDB
  is unhealthy

### Worker, Pub/Sub, and Object Storage

When stories 011-014 and 017 introduce these boundaries, extend this story's
coverage or split implementation into slices:

- worker startup reports Pub/Sub connectivity failures clearly
- worker runtime handles Pub/Sub interruption without acknowledging unprocessed
  work
- outbox retry behavior is observable after transient Pub/Sub failures
- object storage write/read failures map to stable API errors once attachment
  endpoints exist
- attachment scan/finalization database failures leave attachment state and
  finalization outbox work consistent and retryable

## Acceptance Criteria

- Integration tests cover readiness behavior for a healthy and unhealthy
  Postgres dependency.
- Integration tests cover fail-closed Evidence Request authorization behavior
  when SpiceDB is unreachable or returns an authorization dependency error.
- Authentication-before-authorization behavior remains covered for dependency
  failure paths.
- Failure responses use the stable API error envelope and appropriate status
  codes.
- Logs include request and actor context where available, but never include raw
  API keys.
- Harness helpers for dependency interruption are reusable by worker, Pub/Sub,
  and object-storage tests.
- Any dependency boundary not implemented yet is called out in this story as a
  deferred slice, not covered by app-unit mocks.

## Tests

- `cargo test --test integration readiness_returns_not_ready_when_postgres_is_unavailable`
- `cargo test --test integration evidence_request_authz_fails_closed_when_spicedb_is_unavailable`
- `cargo test --test integration authentication_still_precedes_dependency_authorization_failures`
- Worker/Pub/Sub/object-storage integration tests when those stories land.
- `cargo test --test integration worker_handlers`

## QA Guide

1. Ensure Docker is running.
2. Run the integration test target.
3. Confirm dependency-failure tests pass consistently when run alone and with the
   full suite.
4. Review captured logs from a forced dependency failure and confirm they include
   request context without credential material.
5. Run `cargo test`, `cargo clippy --all-targets -- -D warnings`, and
   `make authz-schema-validate`.
