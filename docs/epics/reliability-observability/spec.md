# Reliability And Observability Spec

## Goal

Prove external dependency failure behavior and expose low-cardinality metrics
for the API, dequeuer, worker, storage, and MCP runtimes.

## Existing Baseline

- `/readyz` checks Postgres with a timeout.
- Authentication resolves persisted users, memberships, MCP OAuth agent
  connections, and permissions from Postgres. (`ppat_` API tokens were removed
  in PR #42 — see the [Agent Connector
  Onboarding](../agent-connector-onboarding/spec.md) 2026-07-09 decision
  banner.) Authorization is local policy over that context; there is no separate
  authorization service or synchronization path.
- Outbox publish retry and worker delivery behavior have integration-v2
  coverage.
- Document scan/finalization tests already cover concrete Postgres rollback,
  scanner failure, and object-store failure.
- `/metrics` exists, but application-specific `proof_` metrics do not.

The existing worker-handler coverage is baseline. Do not recreate it as pending
work or replace concrete integration-v2 tests with internal mocks.

Do not add live Postgres interruption/recovery tests. Those primarily exercise
the database and connection-pool dependencies rather than Proofplane behavior.
Continue testing application-owned transaction rollback, retry, and consistency
rules against concrete Postgres where those rules are implemented.

## Failure Contracts

Cover externally visible behavior for:

- Pub/Sub publish failure and later outbox recovery;
- initial quarantine-write failure in the document upload API: return a stable
  error and commit no document row or scan-request outbox event;
- final-object read failure in the human download route: return a stable error
  without changing the persisted document lifecycle;
- worker finalization copy failure: return a retryable delivery error and leave
  the document `finalizing`;
- database failure after a successful finalization copy: retry
  `mark_document_uploaded` within the handler using the shared `Retryable`
  trait and configured `worker.retry_attempts`; after local retries are
  exhausted, return a retryable delivery error so Pub/Sub redelivers;
- worker quarantine-delete failure after a successful copy: keep the document
  `uploaded` and treat deletion as best-effort cleanup;
- ClamAV unavailable/timeout through worker retry and final delivery;
- GCS and production Pub/Sub adapter failures after those adapters land.

Stable API errors must not expose dependency internals. Logs include request,
user/API-token or system identity, operation, and dependency context without
credentials or document bytes.

Existing integration-v2 authentication and authorization tests are baseline.
They already cover invalid credentials, workspace mismatch, missing permissions,
and not-found concealment. Do not create an artificial
authorization-dependency failure fixture now that authorization is
Postgres-sourced application policy.

The API owns the initial stream into quarantine storage before creating the
document row. The worker later owns the copy from quarantine to the final
object key. Document lifecycle state, exposed to agents through the normal
read tools, is the asynchronous status contract; the MVP browser app does not
poll or render document-processing status.

Finalization copy is idempotent. If the copy succeeds but the database update
fails, the handler retries only the database update in-process rather than
repeating the copy for every local attempt. Exhausting those bounded retries
does not mark the document `failed`, because finalized bytes may already
exist. The handler returns a retryable error, and a later Pub/Sub delivery may
repeat the idempotent copy before trying the database transition again. If all
Pub/Sub deliveries are exhausted, the message is dead-lettered and the
document remains `finalizing` for operational recovery.

## Metrics Contract

Use the `proofplane_` prefix. Allowed labels are matched route, method, status class,
operation, dependency, permission, event type, and coarse result.
Never label with workspace, actor, request, object, submission, document,
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
Postgres rows. The shared helper accepts only UUID identifiers and static event,
operation, object-type, and system-client names; it has no free-form payload or
error field. Every audit record includes:

- `type = "audit_log"`;
- stable event name and outcome;
- timestamp and generated event ID;
- workspace ID when scoped;
- an actor type plus user ID, user and API-token IDs, or an identified system
  client;
- request/session correlation IDs when available;
- client type and operation/tool name;
- affected object type and ID where applicable.

Audit records never include credentials, authorization headers, credential
hashes, bearer grant tokens or URLs, internal object keys, document or packet
bytes, submission summaries or descriptions, or unbounded error strings.
Domain tickets define their stable event names and allowed fields.

The helper emits at `info` with no tracing parent. The JSON subscriber supplies
the timestamp and does not serialize current-span fields, so unrelated request
or dependency context cannot enter an audit record.

Mutation success logs are emitted only after the database transaction commits.
This avoids false success records on rollback, but accepts a small crash window
where the commit succeeds and the process exits before logging. The generated
event ID and operation/object fields support downstream deduplication and
reconciliation where necessary.

Audit delivery is best-effort application logging. Routing, retention, IAM, and
export or analysis infrastructure are deferred to future production-deployment
planning. Proofplane does not expose an audit-history API in the MVP.

The unused `audit_events` table is omitted from the consolidated initial schema;
no runtime code writes it.

## Evidence Lifecycle Audit Events

Evidence lifecycle audit events use the shared structured audit-log contract.
Stable event names are:

- `evidence_submission.created`;
- `evidence_document.accepted`;
- `evidence_document_download_grant.issued`;
- `evidence_document_download_grant.redeemed`;
- `evidence_document_scan.completed`;
- `evidence_document_finalization.completed`;
- `agent_evidence_upload_grant.issued`;
- `agent_evidence_upload.completed`.

Allowed fields include workspace ID, user ID, agent connection ID, system
client, request correlation ID, event name, outcome, evidence ID,
submission ID, document ID, grant ID, and coarse lifecycle status where
applicable. (The actor identifier is the agent connection ID, not an API token
ID — `ppat_` was removed in PR #42.) Audit records must not include raw grant
tokens, access tokens, authorization headers, document bytes, storage object
keys treated as internals, scanner raw error strings, credentials, or unbounded
dependency error strings.

Submission creation, document acceptance, and download-grant issuance success
records are emitted only after the database transaction commits. Download-grant
redemption records are emitted only after a grant is validated and the
document remains eligible for streaming. Scan and finalization terminal
outcomes are attributable to the worker system client and must avoid false
success records for retryable failures, duplicate delivery, stale delivery, or
rolled-back mutations; duplicate or stale deliveries may be omitted or logged
only with an explicit non-success outcome.

## Test Harness

Add reusable dependency controls to `tests/integration-v2/support/` only where
multiple tests need them. Prefer stopping a container, severing a proxy, or
injecting an adapter failure at the true external boundary. Tests must be
deterministic alone and in the full `integration-v2` target.

## Revisions

- 2026-08-01: Reconciled the metric prefix with the shipped `proofplane_`
  application families and added the agent-native upload lifecycle events.
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
  document failed.
- 2026-06-11: Standardized application metrics on the `proof_` prefix.
- 2026-06-11: Removed live Postgres interruption/recovery tests from scope;
  concrete-Postgres tests remain for application-owned transaction behavior.
- 2026-06-16: Added the evidence lifecycle audit-event contract under
  Reliability and Observability so domain lifecycle logs share the structured
  audit foundation.
- 2026-06-17: Replaced planned actor/API-key audit attribution with user,
  API-token, legacy-provenance, and system-client fields for the PASETO
  migration.
- 2026-06-17: Removed legacy actor provenance because the pre-deployment PASETO
  cutover does not preserve actor-era data.
- 2026-06-17: Moved `audit_events` removal into the consolidated initial schema;
  there is no incremental drop migration.
- 2026-06-22: Removed obsolete SpiceDB failure scope and its pending ticket.
  Authorization now uses Postgres-sourced identity and permission context with
  no separate dependency to interrupt or synchronize.
- 2026-06-22: Narrowed ticket 005 to a transport-neutral `tracing` audit-event
  API. Cloud Logging routing, retention, IAM, exports, and analysis
  infrastructure are deferred to future production-deployment planning.
