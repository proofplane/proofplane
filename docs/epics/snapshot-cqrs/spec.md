# Snapshot CQRS Spec

## Purpose and constraints

Proofplane adopts CQRS without event sourcing or a second database. Complete
aggregate snapshots in Postgres remain the write-side source of truth. Command
handlers use aggregates inside transactions; query handlers read purpose-built
DTOs directly. Existing HTTP, MCP, OAuth, token, audit, cleanup, and
idempotency behavior is preserved.

Concrete handler types expose inherent `handle` methods. There are no marker
traits, generic mediator, runtime registry, dynamic dispatcher, or service
locator. A composition-root catalog may group concrete handlers for wiring.

## Aggregate boundaries

| Aggregate | Owned state |
| --- | --- |
| User | Provisioned identity, profile, and login lifecycle |
| Workspace | Workspace lifecycle and memberships, including the last-owner invariant |
| AgentConnection | Lifecycle, permissions, and authorization continuation |
| Evidence | Definition, status, and control mappings |
| Control | Definition and framework-requirement references |
| Policy | Definition, control mappings, and archive lifecycle |
| EvidenceSubmission | Coverage and submission provenance |
| Document | Upload, scan, finalization, failure, and archive lifecycle |
| Human upload grants | Evidence and policy issuance and redemption |
| Machine upload grants | Evidence and policy declaration and completion |
| Auditor access | Grant, session, and authentication-transaction roots |
| OAuthAuthorizationFlow | Request, subject, consent, code, cancellation, and consumption |

Aggregate repositories expose narrow complete-snapshot `get` and `save`
operations and return only aggregates named with the bare domain noun.
Read-only shapes live in the read-model boundary, use role-specific names,
and are returned by read gateways. Transaction-scoped reads support command
responses that must observe an aggregate save
before commit. Authorization, parent eligibility, relationships, and
orchestration remain in handlers. Lists, details, reverse mappings, portal
views, catalogs, downloads, and authority resolution are query models.

Every mutable repository maps its primary table through a private record named
for the aggregate. Records implement `try_from_row(&Row)`,
`from_domain(&Domain)`, and `into_domain(...)`; auxiliary arguments to
`into_domain` carry companion or child state loaded by the repository. Primary
and companion records are persisted with the shared full-snapshot upsert,
which updates every non-conflict column, applies uniform constraint
classification, and accepts only one affected row.

Repositories retain local orchestration for aggregate state spanning tables.
Evidence and policy mappings, control requirement references, workspace
memberships, and agent permissions are deleted and reinserted as the complete
current collection inside the transaction. Agent authorization state and OAuth
authorization codes are companion records; optional companion state is
inserted, updated, or removed to match the aggregate. Save paths trust the
application's authorized orchestration and do not add tenant-boundary,
authorization, eligibility, or relationship queries. Workspace filtering
remains on `get` and read-gateway operations.

`UnitOfWork` owns only the database transaction. Commands derive a
`WorkspaceUnitOfWork` scope from it when they need workspace-bound aggregate
repositories or transactional reads. Authentication and actor identity do not
enter the persistence boundary. Ordinary queries call read gateways directly
with the authorized workspace ID; the read gateway owns pooled-client
acquisition and workspace filtering.

## Commands, queries, and execution metadata

Commands and queries are immutable task-oriented values. Each operation has a
concrete handler and operation-specific result and error. Commands carry intent
and authenticated scope; execution metadata carries request, correlation, and
causation identifiers where applicable. Query handlers do not open write
transactions or rehydrate mutable aggregates.

Routes, MCP tools, OAuth endpoints, and workers receive their required typed
handlers from the composition root. Boundary coordinators remain only where an
operation also owns token, object-storage, scanner, or external identity work;
database lifecycle transitions inside those coordinators delegate to concrete
handlers. The incremental compatibility façades are removed at cutover.

## Domain transition events

Aggregate methods return explicit transition results containing their outcome
and any immutable past-tense facts. Aggregates do not retain hidden pending
events. Rejected transitions and idempotent replays emit no event. Events carry
domain IDs and values only—never SQL records, transport payloads, credentials,
URLs, or adapter types.

Handlers translate only events with a real reaction into audit work, outbox
messages, or follow-up commands. An event with no consumer is not emitted.

## Typed integration messaging and outbox

The closed integration contract distinguishes imperative commands from
completed-fact events. Every new message has a UUID message ID, kind, stable
type, positive version, subject, optional correlation and causation IDs, and a
typed payload serialized to JSON. `ScanDocument` and `FinalizeDocument` are
commands with one intended worker handler.

The baseline outbox schema retains the legacy fields while requiring message
kind, type, version, message ID, subject, correlation ID, and causation ID for
new rows. Producers populate both representations while adapters are cut over.

The dequeuer publishes the versioned envelope. Workers decode both the old
envelope and supported typed command versions; unknown types, versions, and
malformed payloads are acknowledged without side effects. Delivery remains
at-least-once. Aggregate state makes handlers idempotent; an inbox is deferred
unless a non-idempotent external side effect is demonstrated by a test.

Aggregate snapshots and resulting outbox messages commit in one transaction.

## Machine-upload-grant correction

Both machine-upload-grant aggregates reject rehydrated completions before
issuance or at/after expiry. Their database constraints mirror that invariant.
Transaction-backed `get` acquires `FOR UPDATE`; Postgres-backed verification
reads do not claim a lock that would be released immediately.

## Migration and compatibility

The application is not deployed, so schema work through the typed-outbox
foundation is consolidated into `V001__initial_schema.sql`. Existing public
schemas and status codes remain stable while internal callers move to handlers.
Workers retain legacy-envelope decoding for compatibility fixtures and any
externally replayed version-zero messages, but no database rolling-upgrade or
backfill path is required before the first deployment.

## Revisions

- 2026-08-09: Standardized mutable repositories on method-oriented private
  records and one unscoped full-snapshot saver. Multi-table synchronization
  remains repository-local, and auditor sessions and authentication
  transactions now persist domain-owned `updated_at` lifecycle state.
- 2026-08-09: Completed the adapter cutover. Snapshot handlers now own write
  transitions, typed scan/finalization commands commit atomically with document
  snapshots, and unused compatibility services and SQL-shaped worker mutations
  were removed. Legacy worker-envelope decoding remains intentionally supported.
- 2026-08-02: Consolidated pre-deployment migrations V002–V006 into V001 and
  removed the rolling database cutover requirement. Runtime legacy-envelope
  decoding remains part of the compatibility contract.

## Verification

Each operation follows red-green-refactor through public behavior. Coverage
includes success, rejection, replay, expiry boundaries, authorization,
concurrency, rollback, emitted events, read-only queries, worker duplicates,
unknown message versions and types, malformed payloads, retry exhaustion, and
correlation propagation. Focused tests and `cargo check --all-targets` run per
ticket; `make check` and a review against `56ae208` close the epic. Modified
runtime code contains no `.expect(`.
