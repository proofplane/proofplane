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

Repositories expose narrow complete-snapshot `get` and `save` operations.
Authorization, parent eligibility, relationships, and orchestration remain in
handlers. Lists, details, reverse mappings, portal views, catalogs, downloads,
and authority resolution are query models.

## Commands, queries, and execution metadata

Commands and queries are immutable task-oriented values. Each operation has a
concrete handler and operation-specific result and error. Commands carry intent
and authenticated scope; execution metadata carries request, correlation, and
causation identifiers where applicable. Query handlers do not open write
transactions or rehydrate mutable aggregates.

Routes, MCP tools, OAuth endpoints, and workers receive only their required
typed handlers from the composition root. During incremental migration a
compatibility façade may delegate to a handler, but no new behavior is added to
the façade and it is removed in ticket 013.

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

Forward-only migration columns retain the legacy outbox fields while adding
message kind, type, version, message ID, subject, correlation ID, and causation
ID. Existing `document.scan_requested` and
`document.finalization_requested` rows are backfilled as version-zero legacy
commands. New producers populate both the typed columns and compatible legacy
fields during the rolling cutover.

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

Migration proceeds operation by operation. Existing public schemas and status
codes remain stable while internal callers move to handlers. Legacy messages
already queued or retried remain processable. Schema changes are forward-only
and existing records are backfilled before new columns become required.

## Verification

Each operation follows red-green-refactor through public behavior. Coverage
includes success, rejection, replay, expiry boundaries, authorization,
concurrency, rollback, emitted events, read-only queries, worker duplicates,
unknown message versions and types, malformed payloads, retry exhaustion, and
correlation propagation. Focused tests and `cargo check --all-targets` run per
ticket; `make check` and a review against `56ae208` close the epic. Modified
runtime code contains no `.expect(`.
