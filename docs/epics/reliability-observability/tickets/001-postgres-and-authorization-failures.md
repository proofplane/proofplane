# 001 - Postgres And Authorization Failures

**Status:** Todo · **Depends on:** none · **Spec:** [spec.md](../spec.md#failure-contracts)

**Summary** - Prove readiness, request recovery, and fail-closed authorization
behavior when Postgres or SpiceDB is interrupted.

**Acceptance criteria**

- [ ] Given healthy then unavailable Postgres, when readiness and a data request
  run, then stable unavailable/internal responses are returned and later
  requests recover after connectivity returns.
- [ ] Given unavailable SpiceDB, when an authenticated data request runs, then it
  fails closed without leaking the target resource.
- [ ] Given missing or invalid credentials while SpiceDB is unavailable, when a
  request runs, then authentication still returns `401` first.

**Tasks**

- [ ] Add deterministic dependency interruption helpers.
- [ ] Add Postgres readiness/request/recovery integration tests.
- [ ] Add SpiceDB failure and auth-ordering integration tests.
- [ ] Assert stable envelopes and secret-free captured logs.
