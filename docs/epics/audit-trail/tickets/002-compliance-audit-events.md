# 002 - Compliance Audit Events

**Status:** Todo · **Depends on:** 001 · **Spec:** [spec.md](../spec.md#mvp-event-set)

**Summary** - Emit audit events from compliance services so the evidence and
control history can be reconstructed without relying on request logs.

**Acceptance criteria**

- [ ] Given an instrumented create, update, mapping, submission, or upload
  operation, when it commits, then its stable event is recorded with actor,
  workspace, object, and request context.
- [ ] Given an instrumented operation fails or rolls back, when history is
  queried, then no success event exists.
- [ ] Given attachment acceptance, when the event payload is inspected, then it
  contains metadata and checksums but no file bytes or credentials.
- [ ] Given unchanged API behavior, when instrumentation ships, then response
  contracts and authorization decisions remain unchanged.

**Tasks**

- [ ] Instrument Evidence Request and control services.
- [ ] Instrument mapping, submission, and attachment acceptance transactions.
- [ ] Add event-contract and rollback integration tests.
- [ ] Document event names and payload versions in the spec.
