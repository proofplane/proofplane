# 003 - Audit History API

**Status:** Todo · **Depends on:** 001 · **Spec:** [spec.md](../spec.md#query-api)

**Summary** - Add an authorized, paginated audit query API for operators and
agents that need provenance and accountability.

**Acceptance criteria**

- [ ] Given authorized filters and a cursor, when history is requested, then
  matching events return newest first with stable pagination.
- [ ] Given an invalid time range, cursor, or unsupported filter, when requested,
  then a stable validation error is returned.
- [ ] Given an unauthorized or cross-workspace actor, when history is requested,
  then `404` is returned and no event data leaks.
- [ ] Given a successful history read, when the transaction completes, then one
  non-recursive `audit_history.read` event is appended.

**Tasks**

- [ ] Add filtered repository query and cursor type.
- [ ] Add `read_audit_events` authorization permission.
- [ ] Add service and REST route.
- [ ] Add pagination, authorization, and self-audit integration tests.
