# Snapshot CQRS Epic

This epic separates Proofplane's write model, read models, and asynchronous
contracts while retaining aggregate snapshots in Postgres as the source of
truth. The core principle is explicitness: one concrete handler per operation,
explicit transition events, and typed versioned messages without a mediator or
event-sourced reconstruction.

Full rationale, schema, and decisions live in [spec.md](./spec.md), the source
of technical depth. Tickets below are lean handoff units that link into it.

[rebase-onto-main.md](./rebase-onto-main.md) records how this branch was moved onto
the rewritten test suite on main, and what remains outstanding.

## Tickets

| Ticket | Status | Notes |
| --- | --- | --- |
| 001. [Architecture and machine-grant corrections](./tickets/001-architecture-and-machine-grant-corrections.md) | Done | Domain boundaries, ADR, rehydration, constraints, and locks. |
| 002. [CQRS application foundation](./tickets/002-cqrs-application-foundation.md) | Done | Handler conventions and an issuance reference slice. |
| 003. [Typed messaging and outbox](./tickets/003-typed-messaging-and-outbox.md) | Done | Versioned envelopes and legacy worker compatibility. |
| 004. [Human upload grants](./tickets/004-human-upload-grants.md) | Todo | Evidence and policy issue/redeem commands and queries. |
| 005. [Auditor access workflows](./tickets/005-auditor-access-workflows.md) | Todo | Access, sessions, authentication transactions, and portal queries. |
| 006. [Users and workspaces](./tickets/006-users-and-workspaces.md) | Todo | Identity and membership aggregates and handlers. |
| 007. [Agent connections](./tickets/007-agent-connections.md) | Todo | Connection lifecycle commands and authority queries. |
| 008. [Controls](./tickets/008-controls.md) | Todo | Control aggregate commands and catalog queries. |
| 009. [Evidence](./tickets/009-evidence.md) | Todo | Evidence lifecycle, mappings, and read models. |
| 010. [Policies](./tickets/010-policies.md) | Todo | Policy lifecycle, mappings, and read models. |
| 011. [Submissions and documents](./tickets/011-submissions-and-documents.md) | Todo | Document transitions and atomic follow-up commands. |
| 012. [OAuth authorization](./tickets/012-oauth-authorization.md) | Todo | OAuth flow aggregate and client-facing queries. |
| 013. [Adapter cutover and cleanup](./tickets/013-adapter-cutover-and-cleanup.md) | Todo | Final callers, façade removal, review, and checks. |

## Sequencing

- **001–003** establish the domain, application, and messaging foundations.
- **004–006** may proceed in parallel after the foundations.
- **007** follows workspace identity; **008** precedes mappings in **009–010**.
- **011** follows its owning concepts and typed messaging; **012** follows
  agent connections.
- **013** completes adapter migration only after every preceding handler is
  available.
