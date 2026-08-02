# 004 - Human Upload Grants

**Status:** Todo · **Triage:** ready-for-agent · **Depends on:** 003 · **Spec:** [spec.md](../spec.md#aggregate-boundaries)

**Summary** - Convert evidence and policy human grant issue/redemption flows
into snapshot aggregates, command handlers, and dedicated authority queries.

**Acceptance criteria**

- [ ] Given eligible authority, when a grant is issued or redeemed, then its aggregate enforces lifecycle and expiry invariants.
- [ ] Given a replay, mismatch, or expired grant, when redemption is attempted, then no duplicate transition or event occurs.
- [ ] Given existing browser upload clients, when cut over, then tokens, status codes, and concealment remain unchanged.

**Tasks**

- [ ] Model evidence and policy human-grant aggregates.
- [ ] Add complete-snapshot repositories.
- [ ] Add issue/redeem command handlers and authority queries.
- [ ] Migrate callers without changing transport DTOs.
- [ ] Add lifecycle, authorization, concurrency, and rollback tests.
