# 001 - Authorization Dependency Failures

**Status:** Todo · **Depends on:** none · **Spec:** [spec.md](../spec.md#failure-contracts)

**Summary** - Prove Proofplane rejects operations and preserves authentication ordering
when SpiceDB returns dependency errors.

**Acceptance criteria**

- [ ] Given unavailable SpiceDB, when an authenticated data request runs, then it
  rejects operations without leaking the target resource.
- [ ] Given missing or invalid credentials while SpiceDB is unavailable, when a
  request runs, then authentication still returns `401` first.
- [ ] Given an authorization dependency error, when it is logged and returned,
  then the API uses a stable error envelope without exposing credentials or
  dependency internals.

**Tasks**

- [ ] Add SpiceDB failure and auth-ordering integration tests.
- [ ] Assert stable envelopes and secret-free captured logs.
- [ ] Reuse an adapter-boundary failure fixture without testing SpiceDB's own
  interruption or recovery behavior.

**Notes**

- Live Postgres interruption/recovery tests are intentionally excluded. Existing
  concrete-Postgres rollback tests cover application-owned database behavior;
  connectivity semantics belong to the pool and database dependencies.
- See the 2026-06-11 failure-contract revision in the spec.
