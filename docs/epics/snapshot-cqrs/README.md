# Snapshot CQRS Epic

This epic separates Proofplane's write model, read models, and asynchronous
contracts while retaining aggregate snapshots in Postgres as the source of
truth. The core principle is explicitness: one concrete handler per operation,
explicit transition events, and typed versioned messages without a mediator or
event-sourced reconstruction.

Full rationale, schema, and decisions live in [spec.md](./spec.md) — the single
source of technical depth.

## Tracking

Tickets live on GitHub, not in this directory.

- Epic: [#125 Epic: Snapshot CQRS](https://github.com/proofplane/proofplane/issues/125)
- Tickets: attached to that issue as sub-issues, and labeled
  [`epic:snapshot-cqrs`](https://github.com/proofplane/proofplane/issues?q=is%3Aissue+label%3Aepic%3Asnapshot-cqrs)

The epic issue carries the ticket index, status, and sequencing. See
[`docs/agents/issue-tracker.md`](../../agents/issue-tracker.md) for the
workflow.
