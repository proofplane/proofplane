# Audit Trail Epic

Add one consistent audit system for human, actor, REST, and MCP activity. The
core principle is transactional truth: an event describing a mutation cannot
survive if the mutation rolls back.

Full event, transaction, and query decisions live in [spec.md](./spec.md).

## Tickets

| Ticket | Status | Notes |
| --- | --- | --- |
| 001. [Audit Schema And Writer](./tickets/001-audit-schema-and-writer.md) | Todo | Shared foundation for all emitters. |
| 002. [Compliance Audit Events](./tickets/002-compliance-audit-events.md) | Todo | Instrument data-plane mutations and meaningful reads. |
| 003. [Audit History API](./tickets/003-audit-history-api.md) | Todo | Add authorized filtered history queries. |

## Sequencing

- **001** is foundational for this epic and Auth Hierarchy API ticket 004.
- **002** can land incrementally by service after 001.
- **003** depends on 001; it can proceed in parallel with later emission work.
