# 004 - Idempotent Upload Completion

**Status:** Todo · **Depends on:** 003 · **Spec:** [spec.md](../spec.md#atomicity-retries-and-cleanup)

**Summary** - Harden grant completion so ambiguous retries and concurrent
transfers converge on one durable submission without leaking quarantine
objects or producing duplicate scan messages.

**Acceptance criteria**

- [ ] Given a completed grant and matching retry, when it is replayed before
  expiry, then it returns `200` with the original submission and document.
- [ ] Given two concurrent valid attempts, when both reach completion, then
  exactly one submission, document, and scan event commit and the losing object
  is deleted.
- [ ] Given an interrupted stream or database rollback, when the attempt ends,
  then no partial submission exists and a safe retry remains possible until
  expiry.
- [ ] Given a replay with mismatched metadata or an expired incomplete grant,
  when it is attempted, then it is rejected without changing durable state.
- [ ] Given cleanup fails, when the primary operation resolves, then its stable
  result is preserved and the cleanup failure is observable without sensitive
  fields.

**Tasks**

- [ ] Implement row-locked, single-winner completion.
- [ ] Implement matching completed-result replay.
- [ ] Separate winning-object ownership from losing-attempt cleanup.
- [ ] Make interrupted and failed attempts retry-safe.
- [ ] Add concrete Postgres tests for replay, races, rollback, outbox
  uniqueness, and object cleanup.
- [ ] Add storage and cleanup failure coverage.
- [ ] Search modified runtime paths for `.expect(` and remove every occurrence.
