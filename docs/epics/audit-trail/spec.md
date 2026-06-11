# Audit Trail Spec

## Goal

Record a durable, queryable history of meaningful identity, compliance, and
agent actions. Audit writes that explain a state change commit in the same
Postgres transaction as that state change.

## Event Model

Evolve the existing `audit_events` table to support:

- workspace ID;
- optional actor ID and optional user ID;
- event type;
- object type and object ID;
- request/correlation ID;
- optional client type and client ID;
- structured event payload;
- creation timestamp.

At least one of actor ID, user ID, or an explicitly identified system client is
required. Payloads contain relevant before/after fields or rationale, but never
credentials, authorization headers, object bytes, or full attachment content.

Event names are stable dot-separated identifiers such as
`evidence_submission.created` and `api_credential.revoked`.

## Transactional Writer

The repository exposes an append primitive on transaction contexts. Services
compose state mutation and event append in one transaction. A small application
writer may build common metadata, but it must accept the caller's existing
transaction rather than open an independent one.

Authentication events without an accompanying state mutation may use a
deduplicated standalone write. `user.logged_in` is not emitted once per request.

## MVP Event Set

Identity emission is owned by
[Auth Hierarchy API ticket 004](../auth-hierarchy-api/tickets/004-auth-and-identity-audit-events.md).

This epic owns data-plane events:

- Evidence Request create/update;
- control create/update and mapping add/remove;
- evidence submission create and attachment accepted;
- trusted source material create/update/read;
- auditor packet generated/exported;
- MCP agent action explicitly logged.

Routine list/get requests are not all audit events. Audit meaningful retrieval
of trusted source material, attachment content, packet exports, and audit
history. Request logs remain the source for ordinary HTTP access diagnostics.

## Query API

Add:

```text
GET /workspaces/{workspace_id}/audit-events
```

Support cursor pagination plus filters for actor/user, event type, object type,
object ID, client type, and bounded time range. Results are newest first with ID
as the tie-breaker. Actor data-plane callers need a dedicated
`read_audit_events` permission. A later human management route may expose the
same service after its product need is defined.

Querying audit history is itself audited without creating an infinite loop:
emit one `audit_history.read` event after a successful query and exclude that
new event from the current response.

## Retention

MVP retention is indefinite. Export, deletion, legal hold, and tenant-configured
retention are post-MVP. Audit rows are append-only through application APIs.

## Revisions

- 2026-06-11: Split shared audit schema/writer/query ownership from identity
  event emission in the existing auth epic.
