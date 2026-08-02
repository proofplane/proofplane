# 011 - Submissions and Documents

**Status:** Todo · **Triage:** ready-for-agent · **Depends on:** 004, 009, 010 · **Spec:** [spec.md](../spec.md#domain-transition-events)

**Summary** - Move submission and document creation, upload processing, failure,
and archival into aggregates that return explicit events and enqueue required
follow-up commands atomically.

**Acceptance criteria**

- [ ] Given each successful lifecycle transition, when handled, then the complete snapshot and only required follow-up messages commit together.
- [ ] Given duplicate, stale, raced, or rejected work, when handled, then no duplicate event or side effect occurs.
- [ ] Given upload failures or compensation, when handled, then database state and object cleanup preserve current guarantees.

**Tasks**

- [ ] Complete submission and document aggregate transitions and events.
- [ ] Add repositories and create/archive/scan/finalize handlers.
- [ ] Translate scan-passed reactions into typed finalization commands.
- [ ] Migrate upload services and workers.
- [ ] Add race, rollback, replay, compensation, and cleanup tests.
