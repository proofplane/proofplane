# 022 - Dependency Failure Integration Coverage

## Status

Partially implemented. Attachment scan and finalization handler integration
tests now use concrete Postgres and inject database failures to verify
application-owned atomic rollback and retry behavior. They also cover scanner
and object-store failures at those adapter boundaries. SpiceDB errors, Pub/Sub
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

Cover dependency errors where Proofplane owns meaningful handling behavior:

- SpiceDB for Evidence Request authorization
- Pub/Sub emulator once worker and outbox stories exist
- filesystem object storage once evidence attachment storage exists

Exercise failed adapter operations through public surfaces where practical.
Avoid reintroducing deleted app-unit tests that mock internal state.

Live Postgres interruption/recovery tests are not remaining scope. They
primarily test Postgres and connection-pool behavior. Concrete-Postgres tests
remain appropriate for Proofplane's transaction rollback, retry, and consistency
rules.

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

- Integration tests cover fail-closed Evidence Request authorization behavior
  when SpiceDB is unreachable or returns an authorization dependency error.
- Authentication-before-authorization behavior remains covered for dependency
  failure paths.
- Failure responses use the stable API error envelope and appropriate status
  codes.
- Logs include request and actor context where available, but never include raw
  API keys.
- Failure fixtures are reusable where multiple Proofplane adapter-boundary tests
  require them.
- Any dependency boundary not implemented yet is called out in this story as a
  deferred slice, not covered by app-unit mocks.

## Tests

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
