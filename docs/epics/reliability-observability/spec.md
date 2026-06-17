# Reliability And Observability Spec

## Goal

Prove external dependency failure behavior and expose low-cardinality metrics
for the API, dequeuer, worker, storage, authorization, and MCP runtimes.

## Existing Baseline

- `/readyz` checks Postgres with a timeout.
- Authorization uses SpiceDB and fails closed through route middleware.
- Outbox publish retry and worker delivery behavior have integration coverage.
- Attachment scan/finalization tests already cover concrete Postgres rollback,
  scanner failure, and object-store failure.
- `/metrics` exists, but application-specific `proof_` metrics do not.

The existing worker-handler coverage is baseline. Do not recreate it as pending
work or replace concrete integration tests with internal mocks.

Do not add live Postgres interruption/recovery tests. Those primarily exercise
the database and connection-pool dependencies rather than Proofplane behavior.
Continue testing application-owned transaction rollback, retry, and consistency
rules against concrete Postgres where those rules are implemented.

## Failure Contracts

Cover externally visible behavior for:

- SpiceDB unavailable while authentication remains ordered first;
- Pub/Sub publish failure and later outbox recovery;
- initial quarantine-write failure in the attachment upload API: return a stable
  error and commit no attachment row or scan-request outbox event;
- final-object read failure in the human download route: return a stable error
  without changing the persisted attachment lifecycle;
- worker finalization copy failure: return a retryable delivery error and leave
  the attachment `finalizing`;
- database failure after a successful finalization copy: retry
  `mark_attachment_uploaded` within the handler using the shared `Retryable`
  trait and configured `worker.retry_attempts`; after local retries are
  exhausted, return a retryable delivery error so Pub/Sub redelivers;
- worker quarantine-delete failure after a successful copy: keep the attachment
  `uploaded` and treat deletion as best-effort cleanup;
- ClamAV unavailable/timeout through worker retry and final delivery;
- GCS and production Pub/Sub adapter failures after those adapters land.

Stable API errors must not expose dependency internals. Logs include request,
actor, operation, and dependency context without credentials or attachment
bytes.

The API owns the initial stream into quarantine storage before creating the
attachment row. The worker later owns the copy from quarantine to the final
object key. Attachment lifecycle state, exposed to agents through the normal
read tools, is the asynchronous status contract; the MVP browser app does not
poll or render document-processing status.

Finalization copy is idempotent. If the copy succeeds but the database update
fails, the handler retries only the database update in-process rather than
repeating the copy for every local attempt. Exhausting those bounded retries
does not mark the attachment `failed`, because finalized bytes may already
exist. The handler returns a retryable error, and a later Pub/Sub delivery may
repeat the idempotent copy before trying the database transition again. If all
Pub/Sub deliveries are exhausted, the message is dead-lettered and the
attachment remains `finalizing` for operational recovery.

## Metrics Contract

Use the `proof_` prefix. Allowed labels are matched route, method, status class,
operation, dependency, permission, event type, and coarse result.
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

## Evidence Lifecycle Audit Events

Evidence lifecycle audit events use the shared structured audit-log contract.
Stable event names are:

- `evidence_submission.created`;
- `evidence_attachment.accepted`;
- `evidence_attachment_download_grant.issued`;
- `evidence_attachment_download_grant.redeemed`;
- `evidence_attachment_scan.completed`;
- `evidence_attachment_finalization.completed`.

Allowed fields include workspace ID, actor ID/user ID/system client, request
correlation ID, event name, outcome, evidence request ID, submission ID,
attachment ID, grant ID, and coarse lifecycle status where applicable. Audit
records must not include raw grant tokens, API keys, authorization headers,
attachment bytes, storage object keys treated as internals, scanner raw error
strings, credentials, or unbounded dependency error strings.

Submission creation, attachment acceptance, and download-grant issuance success
records are emitted only after the database transaction commits. Download-grant
redemption records are emitted only after a grant is validated and the
attachment remains eligible for streaming. Scan and finalization terminal
outcomes are attributable to the worker/system actor and must avoid false
success records for retryable failures, duplicate delivery, stale delivery, or
rolled-back mutations; duplicate or stale deliveries may be omitted or logged
only with an explicit non-success outcome.

## Test Harness

Add reusable dependency controls to `tests/integration/support.rs` only where
multiple tests need them. Prefer stopping a container, severing a proxy, or
injecting an adapter failure at the true external boundary. Tests must be
deterministic alone and in the full integration target.

## Revisions

- 2026-06-11: Reconciled the plan with existing concrete worker rollback
  coverage and removed stale claims that all failure work was unimplemented.
- 2026-06-11: Replaced database-backed audit events and query APIs with
  structured application logs routed to a dedicated Cloud Logging sink.
- 2026-06-11: Split storage-write failure coverage between the API-owned
  quarantine upload and worker-owned finalization. Existing worker copy/delete
  tests remain baseline; the missing work is API quarantine-write coverage.
- 2026-06-11: Defined bounded in-handler database retries after a successful
  finalization copy using `Retryable` and `worker.retry_attempts`; exhausted
  local retries defer to Pub/Sub redelivery without falsely marking the
  attachment failed.
- 2026-06-11: Standardized application metrics on the `proof_` prefix.
- 2026-06-11: Removed live Postgres interruption/recovery tests from scope;
  concrete-Postgres tests remain for application-owned transaction behavior.
- 2026-06-16: Added the evidence lifecycle audit-event contract under
  Reliability and Observability so domain lifecycle logs share the structured
  audit foundation.
