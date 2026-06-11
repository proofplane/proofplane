# 001 - Audit Schema And Writer

**Status:** Todo · **Depends on:** none · **Spec:** [spec.md](../spec.md#transactional-writer)

**Summary** - Evolve the dormant audit table and provide the transaction-scoped
append primitive used by identity, compliance, packet, and MCP operations.

**Acceptance criteria**

- [ ] Given a state mutation and audit append in one transaction, when commit
  succeeds, then exactly one attributable event remains.
- [ ] Given the same operation rolls back, when storage is inspected, then
  neither the state change nor audit event remains.
- [ ] Given payloads containing secret-like fields, when an event is built, then
  prohibited credential and object-byte fields are rejected or omitted.

**Tasks**

- [ ] Add migration columns, indexes, and attribution constraints.
- [ ] Add domain event input types and transaction-context append.
- [ ] Add shared metadata construction for request and client context.
- [ ] Add repository integration tests for append, rollback, and secret safety.
