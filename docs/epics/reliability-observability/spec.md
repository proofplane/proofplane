# Reliability And Observability Spec

## Goal

Prove external dependency failure behavior and expose low-cardinality metrics
for the API, dequeuer, worker, storage, authorization, and MCP runtimes.

## Existing Baseline

- `/readyz` checks Postgres with a timeout, but interruption/recovery integration
  coverage is absent.
- Authorization uses SpiceDB and fails closed through route middleware.
- Outbox publish retry and worker delivery behavior have integration coverage.
- Attachment scan/finalization tests already cover concrete Postgres rollback,
  scanner failure, and object-store failure.
- `/metrics` exists, but application-specific `proofplane_` metrics do not.

The existing worker-handler coverage is baseline. Do not recreate it as pending
work or replace concrete integration tests with internal mocks.

## Failure Contracts

Cover externally visible behavior for:

- Postgres unavailable/timeout/recovery through readiness and representative API
  requests;
- SpiceDB unavailable while authentication remains ordered first;
- Pub/Sub publish failure and later outbox recovery;
- object-storage write/read failure through attachment API surfaces;
- ClamAV unavailable/timeout through worker retry and final delivery;
- GCS and production Pub/Sub adapter failures after those adapters land.

Stable API errors must not expose dependency internals. Logs include request,
actor, operation, and dependency context without credentials or attachment
bytes.

## Metrics Contract

Use the `proofplane_` prefix. Allowed labels are matched route, method,
status class, operation, dependency, permission, event type, and coarse result.
Never label with workspace, actor, request, object, submission, attachment,
credential, error string, or raw path.

Initial families:

- HTTP request count, duration, and in-flight;
- authentication/authorization outcomes;
- dependency readiness;
- outbox claimed/published/retried/backlog;
- worker delivery and handler outcomes/duration;
- object-store operations/bytes/failures;
- scanner outcomes/duration;
- MCP tool outcomes/duration;
- audit-log emission outcomes.

Metrics are per process. The MVP does not build a central collector; deployment
documentation defines scrape endpoints.

## Structured Audit Logs

Audit records are structured application logs emitted through `tracing`, not
Postgres rows. Every audit record includes:

- `type = "audit_log"`;
- stable event name and outcome;
- timestamp and generated event ID;
- workspace ID when scoped;
- actor ID, user ID, or identified system client;
- request/session correlation ID;
- client type and operation/tool name;
- affected object type and ID where applicable.

Audit records never include credentials, authorization headers, credential
hashes, object bytes, source-material bodies, or unbounded error strings.
Domain tickets define their stable event names and allowed fields.

Mutation success logs are emitted only after the database transaction commits.
This avoids false success records on rollback, but accepts a small crash window
where the commit succeeds and the process exits before logging. The generated
event ID and operation/object fields support downstream deduplication and
reconciliation where necessary.

Production routes `type = "audit_log"` records to a dedicated restricted Cloud
Logging sink with longer retention than ordinary application logs. Sink
destination, retention period, access controls, and export/analysis procedures
are deployment configuration. Proofplane does not expose an audit-history API
in the MVP.

The unused `audit_events` table is removed in a migration after confirming no
runtime code writes it.

## Test Harness

Add reusable dependency controls to `tests/integration/support.rs` only where
multiple tests need them. Prefer stopping a container, severing a proxy, or
injecting an adapter failure at the true external boundary. Tests must be
deterministic alone and in the full integration target.

## Revisions

- 2026-06-11: Reconciled legacy story 022 with existing concrete worker rollback
  coverage and removed stale claims that all failure work was unimplemented.
- 2026-06-11: Replaced database-backed audit events and query APIs with
  structured application logs routed to a dedicated Cloud Logging sink.
