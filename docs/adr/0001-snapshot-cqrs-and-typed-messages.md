# Use snapshot CQRS and typed integration messages

Proofplane separates mutations into task-specific command handlers that load
and save complete aggregate snapshots, and reads into query handlers that
return purpose-built DTOs from the same Postgres database. Aggregate
transitions may return explicit domain events, while only facts with an actual
external reaction are translated into versioned integration commands or
events and stored atomically in the outbox. We chose this over event sourcing,
a separate read database, and a generic mediator because it preserves current
synchronous consistency and operational simplicity while making write
invariants, read models, and asynchronous contracts independent.

## Consequences

- Snapshots, not event streams, remain the write-side source of truth.
- Commands and queries are invoked through concrete typed handlers; there is
  no runtime handler registry or service locator.
- Purpose-built query DTOs are read models loaded by read gateways. The term
  projection is reserved for a process that maintains derived read-side state.
- Domain events are explicit transition results and are not replayed to
  reconstruct aggregates.
- A unit of work owns transaction lifetime only. Workspace scoping is applied
  separately when obtaining aggregate repositories, while ordinary read-side
  handlers call workspace-scoped read gateways directly.
- Authentication and authorization remain application concerns; repository
  APIs receive workspace identity only where it is required for tenant-safe
  SQL filtering.
- Existing asynchronous document processing remains at-least-once and workers
  retain compatibility with queued legacy messages during the cutover.
